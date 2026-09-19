//! MCP client：stdio + Streamable HTTP 双 transport
//! stdio：手写 newline-delimited JSON-RPC 2.0 over stdio（无 Content-Length 头）：
//! 每条消息一行完整 JSON。连接流程：initialize 握手 → notifications/initialized →
//! tools/list。调用流程：tools/call。响应由后台 reader task 按 id 分发到
//! pending map（oneshot），与 PluginToolState 同一模式。
//! HTTP（Streamable）：单 endpoint POST JSON-RPC；响应按 Content-Type 分流——
//! application/json 直接解析，text/event-stream 逐帧解析 data 行取匹配 id 的响应；
//! initialize 响应的 Mcp-Session-Id 头在后续请求回带。
//!
//! LLM 侧工具命名空间：`mcp__{sanitize(server)}__{sanitize(tool)}`，
//! `mcp__` 为保留前缀（见 ai::plugin_tools 顶部注释）。

mod http;
mod protocol;
mod stdio;

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;
use tracing::{info, warn};

use crate::core::config::McpServerConfig;
use crate::error::{Result, VoloError};

use super::plugin_tools::sanitize;
use super::tools::ToolSpec;

pub use http::McpHttpClient;
pub use protocol::McpToolInfo;
#[cfg(test)]
use protocol::{parse_sse_response, rpc_outcome, PROTOCOL_VERSION};
pub use stdio::McpConnection;

/// 单 server 连接（含握手 + tools/list）超时
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 单次 tools/call 超时
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// LLM 工具名的 MCP 命名空间前缀
pub const MCP_NAME_PREFIX: &str = "mcp__";

/// 统一两种 transport 的连接句柄（注册表与调用侧不感知差异）
pub enum McpClient {
    Stdio(McpConnection<ChildStdout, ChildStdin>),
    Http(McpHttpClient),
}

impl McpClient {
    pub fn tools(&self) -> &[McpToolInfo] {
        match self {
            McpClient::Stdio(conn) => conn.tools(),
            McpClient::Http(client) => client.tools(),
        }
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        match self {
            McpClient::Stdio(conn) => conn.call_tool(name, arguments).await,
            McpClient::Http(client) => client.call_tool(name, arguments).await,
        }
    }
}

/// MCP 连接注册表（Tauri managed state）
pub struct McpRegistry {
    connections: Mutex<HashMap<String, Arc<McpClient>>>,
    children: Mutex<HashMap<String, Child>>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            children: Mutex::new(HashMap::new()),
        }
    }

    /// 连接所有 enabled 的 server。已连接的跳过（幂等）；
    /// url 非空走 Streamable HTTP，否则 stdio 子进程；
    /// 单 server 失败/超时 warn + skip，不阻断其余 server
    pub async fn connect_all(&self, servers: &HashMap<String, McpServerConfig>) {
        for (name, config) in servers {
            if !config.enabled {
                continue;
            }
            let already = self
                .connections
                .lock()
                .map(|c| c.contains_key(name))
                .unwrap_or(false);
            if already {
                continue;
            }

            match timeout(CONNECT_TIMEOUT, Self::connect_one(config)).await {
                Ok(Ok((client, child))) => {
                    info!(
                        "MCP server {} 已连接（{} 个工具）",
                        name,
                        client.tools().len()
                    );
                    if let Ok(mut connections) = self.connections.lock() {
                        connections.insert(name.clone(), Arc::new(client));
                    }
                    if let Some(child) = child {
                        if let Ok(mut children) = self.children.lock() {
                            children.insert(name.clone(), child);
                        }
                    }
                }
                Ok(Err(e)) => warn!("MCP server {} 连接失败: {}", name, e),
                Err(_) => warn!(
                    "MCP server {} 连接超时（{} 秒）",
                    name,
                    CONNECT_TIMEOUT.as_secs()
                ),
            }
        }
    }

    /// 按配置建连：url 非空 = Streamable HTTP（无子进程）；否则 stdio 子进程
    async fn connect_one(config: &McpServerConfig) -> Result<(McpClient, Option<Child>)> {
        if !config.url.trim().is_empty() {
            let client = McpHttpClient::connect(config.url.trim()).await?;
            return Ok((McpClient::Http(client), None));
        }
        let (conn, child) = Self::spawn_and_connect(config).await?;
        Ok((McpClient::Stdio(conn), Some(child)))
    }

    async fn spawn_and_connect(
        config: &McpServerConfig,
    ) -> Result<(McpConnection<ChildStdout, ChildStdin>, Child)> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // 丢弃 stderr，避免子进程因管道写满阻塞
            .spawn()
            .map_err(|e| {
                VoloError::Other(format!("启动 MCP 命令 {} 失败: {}", config.command, e))
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| VoloError::Other("MCP 子进程 stdout 不可用".to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| VoloError::Other("MCP 子进程 stdin 不可用".to_string()))?;

        let conn = McpConnection::connect(stdout, stdin).await?;
        Ok((conn, child))
    }

    /// 已连接 server 的工具聚合为 LLM 规格：
    /// name = `mcp__{sanitize(server)}__{sanitize(tool)}`，description 加 `[MCP:{server}] ` 前缀
    pub fn specs(&self) -> Vec<ToolSpec> {
        let Ok(connections) = self.connections.lock() else {
            warn!("McpRegistry::specs: lock poisoned");
            return Vec::new();
        };

        let mut specs = Vec::new();
        for (server, conn) in connections.iter() {
            for tool in conn.tools() {
                specs.push(ToolSpec {
                    name: format!(
                        "{}{}__{}",
                        MCP_NAME_PREFIX,
                        sanitize(server),
                        sanitize(&tool.name)
                    ),
                    description: format!(
                        "[MCP:{}] {}",
                        server,
                        tool.description
                            .clone()
                            .unwrap_or_else(|| tool.name.clone())
                    ),
                    parameters: tool.input_schema.clone(),
                });
            }
        }
        specs
    }

    /// 按 LLM 名调用：`mcp__` 前缀剥掉后按第一个 `__` 切分，
    /// 以 sanitize 后的名字反查原始 server / tool
    pub async fn call(&self, llm_name: &str, args: Value) -> Result<Value> {
        let not_found = || VoloError::NotFound(format!("mcp tool: {}", llm_name));
        let rest = llm_name
            .strip_prefix(MCP_NAME_PREFIX)
            .ok_or_else(not_found)?;
        let (san_server, san_tool) = rest.split_once("__").ok_or_else(not_found)?;
        if san_server.is_empty() || san_tool.is_empty() {
            return Err(not_found());
        }

        let (conn, orig_tool) = {
            let connections = self
                .connections
                .lock()
                .map_err(|_| VoloError::Other("McpRegistry lock poisoned".to_string()))?;
            let mut found = None;
            for (server, conn) in connections.iter() {
                if sanitize(server) != san_server {
                    continue;
                }
                if let Some(tool) = conn.tools().iter().find(|t| sanitize(&t.name) == san_tool) {
                    found = Some((conn.clone(), tool.name.clone()));
                    break;
                }
            }
            found.ok_or_else(not_found)?
        };

        conn.call_tool(&orig_tool, args).await
    }

    /// 杀掉所有 MCP 子进程并清空连接表（应用退出时调用）
    pub fn shutdown(&self) {
        if let Ok(mut children) = self.children.lock() {
            for (name, mut child) in children.drain() {
                if let Err(e) = child.start_kill() {
                    warn!("MCP server {} 终止失败: {}", name, e);
                }
            }
        }
        if let Ok(mut connections) = self.connections.lock() {
            connections.clear();
        }
    }

    /// 已连接 server 数（测试用）
    #[cfg(test)]
    pub fn connection_count(&self) -> usize {
        self.connections.lock().map(|c| c.len()).unwrap_or(0)
    }
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{
        duplex, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
    };

    /// 模拟 server 端：读一行、按 method 回一行响应
    async fn run_mock_server(io: DuplexStream) {
        let (reader, mut writer) = tokio::io::split(io);
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(method) = msg.get("method").and_then(Value::as_str) else {
                continue;
            };
            let id = msg.get("id").cloned();
            let response = match method {
                "initialize" => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": {},
                        "serverInfo": { "name": "mock", "version": "0.1.0" },
                    }
                })),
                "notifications/initialized" => None,
                "tools/list" => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [
                            {
                                "name": "echo.tool",
                                "description": "回显输入",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": { "text": { "type": "string" } },
                                },
                            },
                            { "name": "fail", "inputSchema": { "type": "object" } },
                        ]
                    }
                })),
                "tools/call" => {
                    let name = msg["params"]["name"].as_str().unwrap_or("");
                    match name {
                        "echo.tool" => Some(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    { "type": "text", "text": "pong:" },
                                    { "type": "image", "data": "ignored" },
                                    { "type": "text", "text": msg["params"]["arguments"]["text"] },
                                ]
                            }
                        })),
                        "fail" => Some(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "isError": true,
                                "content": [{ "type": "text", "text": "boom" }]
                            }
                        })),
                        _ => Some(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32601, "message": "tool not found" }
                        })),
                    }
                }
                _ => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": "method not found" }
                })),
            };
            if let Some(response) = response {
                let mut line = serde_json::to_string(&response).unwrap();
                line.push('\n');
                writer.write_all(line.as_bytes()).await.unwrap();
                writer.flush().await.unwrap();
            }
        }
    }

    async fn connected_pair() -> McpConnection<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>> {
        let (client, server) = duplex(4096);
        tokio::spawn(run_mock_server(server));
        let (client_read, client_write) = tokio::io::split(client);
        McpConnection::connect(client_read, client_write)
            .await
            .expect("handshake failed")
    }

    #[tokio::test]
    async fn test_connect_handshake_and_list_tools() {
        let conn = connected_pair().await;
        let tools = conn.tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "echo.tool");
        assert_eq!(tools[0].description.as_deref(), Some("回显输入"));
        assert_eq!(tools[0].input_schema["type"], "object");
        assert_eq!(tools[1].name, "fail");
        assert!(tools[1].description.is_none());
    }

    #[tokio::test]
    async fn test_call_tool_concatenates_text_content() {
        let conn = connected_pair().await;
        let result = conn
            .call_tool("echo.tool", json!({ "text": "hello" }))
            .await
            .unwrap();
        assert_eq!(result, Value::String("pong:\nhello".to_string()));
    }

    #[tokio::test]
    async fn test_call_tool_is_error() {
        let conn = connected_pair().await;
        let err = conn.call_tool("fail", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[tokio::test]
    async fn test_call_tool_json_rpc_error() {
        let conn = connected_pair().await;
        let err = conn.call_tool("no-such-tool", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("tool not found"));
    }

    #[tokio::test]
    async fn test_call_tool_timeout() {
        let (client, mut server) = duplex(4096);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1024];
            loop {
                match tokio::io::AsyncReadExt::read(&mut server, &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });
        let (client_read, client_write) = tokio::io::split(client);

        let result = McpConnection::connect_with_timeout(
            client_read,
            client_write,
            Duration::from_millis(100),
        )
        .await;
        match result {
            Err(e) => assert!(e.to_string().contains("握手超时")),
            Ok(_) => panic!("握手本应超时"),
        }
    }

    #[tokio::test]
    async fn test_call_tool_timeout_after_handshake() {
        let (client, server) = duplex(4096);
        tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(server);
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(method) = msg.get("method").and_then(Value::as_str) else {
                    continue;
                };
                let response = match method {
                    "initialize" => Some(json!({
                        "jsonrpc": "2.0", "id": msg["id"],
                        "result": { "protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "serverInfo": {"name":"mock","version":"0"} }
                    })),
                    "tools/list" => Some(json!({
                        "jsonrpc": "2.0", "id": msg["id"], "result": { "tools": [] }
                    })),
                    _ => None,
                };
                if let Some(response) = response {
                    let mut line = serde_json::to_string(&response).unwrap();
                    line.push('\n');
                    writer.write_all(line.as_bytes()).await.unwrap();
                    writer.flush().await.unwrap();
                }
            }
        });
        let (client_read, client_write) = tokio::io::split(client);
        let conn = McpConnection::connect(client_read, client_write)
            .await
            .unwrap();

        let err = conn
            .call_tool_with_timeout("anything", json!({}), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("超时"));
    }

    #[tokio::test]
    async fn test_concurrent_requests_match_by_id() {
        let (client, server) = duplex(4096);
        tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(server);
            let mut lines = BufReader::new(reader).lines();
            let mut buffered_calls: Vec<Value> = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let response = match msg.get("method").and_then(Value::as_str) {
                    Some("initialize") => Some(json!({
                        "jsonrpc": "2.0", "id": msg["id"],
                        "result": {
                            "protocolVersion": PROTOCOL_VERSION,
                            "capabilities": {},
                            "serverInfo": {"name":"mock","version":"0"}
                        }
                    })),
                    Some("tools/list") => Some(json!({
                        "jsonrpc": "2.0", "id": msg["id"], "result": { "tools": [] }
                    })),
                    Some("tools/call") => {
                        buffered_calls.push(msg);
                        if buffered_calls.len() < 2 {
                            continue;
                        }
                        let mut out = String::new();
                        for call in buffered_calls.drain(..).rev() {
                            let text = call["params"]["arguments"]["text"].clone();
                            out.push_str(
                                &serde_json::to_string(&json!({
                                    "jsonrpc": "2.0",
                                    "id": call["id"],
                                    "result": { "content": [{ "type": "text", "text": text }] }
                                }))
                                .unwrap(),
                            );
                            out.push('\n');
                        }
                        writer.write_all(out.as_bytes()).await.unwrap();
                        writer.flush().await.unwrap();
                        continue;
                    }
                    _ => None,
                };
                if let Some(response) = response {
                    let mut line = serde_json::to_string(&response).unwrap();
                    line.push('\n');
                    writer.write_all(line.as_bytes()).await.unwrap();
                    writer.flush().await.unwrap();
                }
            }
        });
        let (client_read, client_write) = tokio::io::split(client);
        let conn = McpConnection::connect(client_read, client_write)
            .await
            .unwrap();

        let (a, b) = tokio::join!(
            conn.call_tool("echo", json!({ "text": "A" })),
            conn.call_tool("echo", json!({ "text": "B" })),
        );
        assert_eq!(a.unwrap(), Value::String("A".to_string()));
        assert_eq!(b.unwrap(), Value::String("B".to_string()));
    }

    #[test]
    fn test_registry_specs_naming() {
        let name = format!("{}__{}", sanitize("my.server"), sanitize("echo.tool"));
        assert_eq!(name, "my_server__echo_tool");
        let llm_name = format!("{}{}", MCP_NAME_PREFIX, name);
        assert_eq!(llm_name, "mcp__my_server__echo_tool");
        let rest = llm_name.strip_prefix(MCP_NAME_PREFIX).unwrap();
        assert_eq!(rest.split_once("__"), Some(("my_server", "echo_tool")));
    }

    #[test]
    fn test_rpc_outcome() {
        assert_eq!(
            rpc_outcome(&json!({ "id": 1, "result": { "a": 1 } })).unwrap(),
            json!({ "a": 1 })
        );
        let err = rpc_outcome(&json!({ "id": 1, "error": { "code": -32601, "message": "nope" } }))
            .unwrap_err();
        assert!(err.to_string().contains("nope"));
        assert_eq!(rpc_outcome(&json!({ "id": 1 })).unwrap(), Value::Null);
    }

    #[test]
    fn test_parse_sse_response() {
        let body = "event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{\"tools\":[]}}\n\
                    \n\
                    event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\
                    \n";
        assert_eq!(parse_sse_response(body, 2).unwrap(), json!({ "ok": true }));
        assert!(parse_sse_response(body, 99).is_err());
        let err_body =
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-1,\"message\":\"bad\"}}\n\n";
        assert!(parse_sse_response(err_body, 1)
            .unwrap_err()
            .to_string()
            .contains("bad"));
    }

    #[tokio::test]
    async fn test_http_connect_and_call() {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
            haystack.windows(needle.len()).position(|w| w == needle)
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf: Vec<u8> = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let header_end = loop {
                        match sock.read(&mut tmp).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => {
                                buf.extend_from_slice(&tmp[..n]);
                                if let Some(pos) = find(&buf, b"\r\n\r\n") {
                                    break pos;
                                }
                            }
                        }
                    };
                    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let content_len = headers
                        .lines()
                        .find_map(|l| {
                            let lower = l.to_ascii_lowercase();
                            lower
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body = buf[header_end + 4..].to_vec();
                    while body.len() < content_len {
                        match sock.read(&mut tmp).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => body.extend_from_slice(&tmp[..n]),
                        }
                    }
                    let msg: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let has_sid = headers
                        .to_ascii_lowercase()
                        .contains("mcp-session-id: sid-123");
                    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
                    let id = msg.get("id").cloned().unwrap_or(Value::Null);

                    let (status, content_type, extra, resp_body) = match method {
                        "initialize" => (
                            "200 OK",
                            "application/json",
                            "Mcp-Session-Id: sid-123\r\n",
                            json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {
                                    "protocolVersion": PROTOCOL_VERSION,
                                    "capabilities": {},
                                    "serverInfo": { "name": "mock-http", "version": "0.1.0" },
                                }
                            })
                            .to_string(),
                        ),
                        "notifications/initialized" => {
                            ("202 Accepted", "text/plain", "", String::new())
                        }
                        "tools/list" if has_sid => (
                            "200 OK",
                            "application/json",
                            "",
                            json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": { "tools": [{
                                    "name": "ping",
                                    "description": "HTTP 回显",
                                    "inputSchema": { "type": "object" },
                                }] }
                            })
                            .to_string(),
                        ),
                        "tools/call" if has_sid => {
                            let text = msg["params"]["arguments"]["text"].clone();
                            let frame = json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": { "content": [{ "type": "text", "text": text }] }
                            });
                            (
                                "200 OK",
                                "text/event-stream",
                                "",
                                format!("event: message\ndata: {}\n\n", frame),
                            )
                        }
                        _ => (
                            "400 Bad Request",
                            "application/json",
                            "",
                            json!({
                                "jsonrpc": "2.0", "id": id,
                                "error": { "code": -32000, "message": "missing session id" }
                            })
                            .to_string(),
                        ),
                    };

                    let response = format!(
                        "HTTP/1.1 {}\r\nContent-Type: {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        content_type,
                        extra,
                        resp_body.len(),
                        resp_body
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                });
            }
        });

        let url = format!("http://{}/mcp", addr);
        let client = McpHttpClient::connect(&url).await.expect("HTTP 握手失败");

        let tools = client.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "ping");

        let result = client
            .call_tool("ping", json!({ "text": "hello-http" }))
            .await
            .unwrap();
        assert_eq!(result, Value::String("hello-http".to_string()));
    }
}
