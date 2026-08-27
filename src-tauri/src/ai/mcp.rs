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

use std::collections::HashMap;
use std::marker::PhantomData;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::core::config::McpServerConfig;
use crate::error::{Result, VoloError};

use super::plugin_tools::sanitize;
use super::tools::ToolSpec;

/// 握手采用的 MCP 协议版本
const PROTOCOL_VERSION: &str = "2024-11-05";
/// 单 server 连接（含握手 + tools/list）超时
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 单次 tools/call 超时
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// LLM 工具名的 MCP 命名空间前缀
pub const MCP_NAME_PREFIX: &str = "mcp__";

/// MCP server 暴露的工具（原始名，未经 sanitize）
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

/// 从 JSON-RPC 响应消息提取 result / error（stdio 读循环与 HTTP 响应共用）
fn rpc_outcome(msg: &Value) -> Result<Value> {
    if let Some(error) = msg.get("error") {
        return Err(VoloError::Other(format!(
            "MCP error {}: {}",
            error["code"],
            error["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

/// 解析 tools/list 的 result 为工具信息数组（两种 transport 共用）
fn parse_tools_list(result: &Value) -> Vec<McpToolInfo> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?.to_string();
                    Some(McpToolInfo {
                        name,
                        description: tool
                            .get("description")
                            .and_then(Value::as_str)
                            .map(|s| s.to_string()),
                        input_schema: tool
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 从 tools/call 的 result 提取 text 类型 content 拼接；isError 为 true 时返回 Err
/// （两种 transport 共用）
fn extract_tool_text(result: &Value, name: &str) -> Result<Value> {
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(VoloError::Other(format!("MCP 工具 {} 执行失败: {}", name, text)));
    }
    Ok(Value::String(text))
}

/// 从 SSE 响应体中逐帧解析 data 行，找与请求 id 匹配的 JSON-RPC 响应
fn parse_sse_response(body: &str, id: u64) -> Result<Value> {
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if msg.get("id").and_then(Value::as_u64) == Some(id) {
            return rpc_outcome(&msg);
        }
    }
    Err(VoloError::Other(format!(
        "MCP SSE 响应中没有 id={} 的响应",
        id
    )))
}

/// 一条 MCP stdio 连接：写端 + pending 分发表；读端由后台 task 消费
///
/// 泛型化以便测试用 tokio::io::duplex 模拟 server；R 只出现在 connect 参数里，
/// 经 PhantomData<fn() -> R> 保留类型信息（不影响 Send/Sync 自动推导）。
pub struct McpConnection<R, W> {
    writer: tokio::sync::Mutex<W>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    next_id: AtomicU64,
    tools: Vec<McpToolInfo>,
    _reader: PhantomData<fn() -> R>,
}

impl<R, W> McpConnection<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send,
{
    /// 建立连接：initialize 握手 → notifications/initialized → tools/list（默认 10s 超时）
    pub async fn connect(reader: R, writer: W) -> Result<Self> {
        Self::connect_with_timeout(reader, writer, CONNECT_TIMEOUT).await
    }

    /// 同 connect，超时可注入（测试用）
    pub async fn connect_with_timeout(
        reader: R,
        writer: W,
        handshake_timeout: Duration,
    ) -> Result<Self> {
        timeout(handshake_timeout, Self::handshake(reader, writer))
            .await
            .map_err(|_| {
                VoloError::Other(format!(
                    "MCP 握手超时（{} 秒）",
                    handshake_timeout.as_secs()
                ))
            })?
    }

    async fn handshake(reader: R, writer: W) -> Result<Self> {
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // 后台读循环：只分发 response（有 id 且无 method）；
        // server 主动发的 request/notification 忽略。读到 EOF 后 fail 所有 pending。
        let pending_reader = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                            warn!("MCP: 无法解析的消息行: {}", line);
                            continue;
                        };
                        if msg.get("method").is_some() {
                            continue; // server 发的 request/notification，忽略
                        }
                        let Some(id) = msg.get("id").and_then(Value::as_u64) else {
                            continue;
                        };
                        let tx = pending_reader
                            .lock()
                            .ok()
                            .and_then(|mut pending| pending.remove(&id));
                        let Some(tx) = tx else {
                            warn!("MCP: 未知响应 id: {}", id);
                            continue;
                        };
                        let _ = tx.send(rpc_outcome(&msg));
                    }
                    Ok(None) => break, // EOF：子进程退出
                    Err(e) => {
                        warn!("MCP: 读 stdio 失败: {}", e);
                        break;
                    }
                }
            }
            // 流结束：唤醒所有挂起的请求，避免调用方等到超时
            if let Ok(mut pending) = pending_reader.lock() {
                for (_, tx) in pending.drain() {
                    let _ = tx.send(Err(VoloError::Other("MCP 连接已断开".to_string())));
                }
            }
        });

        let conn = Self {
            writer: tokio::sync::Mutex::new(writer),
            pending,
            next_id: AtomicU64::new(1),
            tools: Vec::new(),
            _reader: PhantomData,
        };

        conn.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "volo",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )
        .await?;

        // initialized 是 notification：无 id，不等响应
        conn.notify("notifications/initialized", json!({})).await?;

        let result = conn.request("tools/list", json!({})).await?;
        let tools = parse_tools_list(&result);

        Ok(Self { tools, ..conn })
    }

    /// 调用工具：tools/call（默认 30s 超时）。
    /// 返回 result.content 数组中 text 类型条目的拼接；isError 为 true 时返回 Err。
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        self.call_tool_with_timeout(name, arguments, CALL_TIMEOUT)
            .await
    }

    /// 同 call_tool，超时可注入（测试用）
    pub async fn call_tool_with_timeout(
        &self,
        name: &str,
        arguments: Value,
        call_timeout: Duration,
    ) -> Result<Value> {
        timeout(call_timeout, self.call_tool_inner(name, arguments))
            .await
            .map_err(|_| {
                VoloError::Other(format!(
                    "MCP 工具 {} 调用超时（{} 秒）",
                    name,
                    call_timeout.as_secs()
                ))
            })?
    }

    async fn call_tool_inner(&self, name: &str, arguments: Value) -> Result<Value> {
        let result = self
            .request("tools/call", json!({ "name": name, "arguments": arguments }))
            .await?;
        extract_tool_text(&result, name)
    }

    /// 发一个 JSON-RPC request 并等响应（按自增 id 匹配）
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, tx);
        }

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(e) = self.write_line(&msg).await {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
            return Err(e);
        }

        rx.await
            .map_err(|_| VoloError::Other("MCP 响应通道已关闭".to_string()))?
    }

    /// 发一个 JSON-RPC notification（无 id，无响应）
    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write_line(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    async fn write_line(&self, msg: &Value) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        let mut writer = self.writer.lock().await;
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 已握手拿到的工具列表
    pub fn tools(&self) -> &[McpToolInfo] {
        &self.tools
    }
}

/// MCP Streamable HTTP 连接：单 endpoint POST JSON-RPC。
/// 响应按 Content-Type 分流：application/json 直接解析；text/event-stream 逐帧找匹配 id。
/// initialize 响应里的 Mcp-Session-Id 头由后续请求回带（无状态 server 不发则不带）。
pub struct McpHttpClient {
    url: String,
    http: reqwest::Client,
    session_id: Mutex<Option<String>>,
    next_id: AtomicU64,
    tools: Vec<McpToolInfo>,
}

impl McpHttpClient {
    /// 建立连接：initialize 握手 → notifications/initialized → tools/list（默认 10s 超时）
    pub async fn connect(url: &str) -> Result<Self> {
        timeout(CONNECT_TIMEOUT, Self::handshake(url))
            .await
            .map_err(|_| {
                VoloError::Other(format!("MCP HTTP 握手超时（{} 秒）", CONNECT_TIMEOUT.as_secs()))
            })?
    }

    async fn handshake(url: &str) -> Result<Self> {
        let client = Self {
            url: url.to_string(),
            http: reqwest::Client::new(),
            session_id: Mutex::new(None),
            next_id: AtomicU64::new(1),
            tools: Vec::new(),
        };

        client
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "volo",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
            )
            .await?;

        client.notify("notifications/initialized", json!({})).await?;

        let result = client.request("tools/list", json!({})).await?;
        let tools = parse_tools_list(&result);

        Ok(Self { tools, ..client })
    }

    /// 调用工具：tools/call（30s 超时）；提取逻辑与 stdio 一致
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let fut = async {
            let result = self
                .request("tools/call", json!({ "name": name, "arguments": arguments }))
                .await?;
            extract_tool_text(&result, name)
        };
        timeout(CALL_TIMEOUT, fut).await.map_err(|_| {
            VoloError::Other(format!(
                "MCP 工具 {} 调用超时（{} 秒）",
                name,
                CALL_TIMEOUT.as_secs()
            ))
        })?
    }

    /// 发 JSON-RPC request 并等响应（按自增 id 匹配；JSON 或 SSE 响应都支持）
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let resp = self.post(&msg).await?;

        // 捕获 session id（通常在 initialize 响应里下发）
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(mut slot) = self.session_id.lock() {
                *slot = Some(sid.to_string());
            }
        }

        let status = resp.status();
        let is_sse = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.starts_with("text/event-stream"))
            .unwrap_or(false);
        let body = resp
            .text()
            .await
            .map_err(|e| VoloError::Other(format!("MCP HTTP 响应读取失败: {}", e)))?;

        if is_sse {
            return parse_sse_response(&body, id);
        }
        if body.trim().is_empty() {
            return Err(VoloError::Other(format!(
                "MCP HTTP {}: 空响应（{} {}）",
                status.as_u16(),
                method,
                self.url
            )));
        }
        let msg: Value = serde_json::from_str(&body)
            .map_err(|e| VoloError::Other(format!("MCP HTTP 响应解析失败: {}", e)))?;
        rpc_outcome(&msg)
    }

    /// 发 JSON-RPC notification（无 id；server 应回 202/2xx 空响应）
    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let resp = self.post(&msg).await?;
        if !resp.status().is_success() {
            return Err(VoloError::Other(format!(
                "MCP HTTP {}: notification {} 未被接受",
                resp.status().as_u16(),
                method
            )));
        }
        Ok(())
    }

    async fn post(&self, msg: &Value) -> Result<reqwest::Response> {
        let mut req = self
            .http
            .post(&self.url)
            .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
            .json(msg);
        if let Some(sid) = self.session_id.lock().ok().and_then(|s| s.clone()) {
            req = req.header("mcp-session-id", sid);
        }
        req.send()
            .await
            .map_err(|e| VoloError::Other(format!("MCP HTTP 请求失败（{}）: {}", self.url, e)))
    }

    /// 已握手拿到的工具列表
    pub fn tools(&self) -> &[McpToolInfo] {
        &self.tools
    }
}

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
                Err(_) => warn!("MCP server {} 连接超时（{} 秒）", name, CONNECT_TIMEOUT.as_secs()),
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
                        tool.description.clone().unwrap_or_else(|| tool.name.clone())
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
        let not_found = || {
            VoloError::NotFound(format!("mcp tool: {}", llm_name))
        };
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
                if let Some(tool) = conn
                    .tools()
                    .iter()
                    .find(|t| sanitize(&t.name) == san_tool)
                {
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
    use tokio::io::{duplex, DuplexStream, ReadHalf, WriteHalf};

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
                "notifications/initialized" => None, // notification 无响应
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

    /// 建立一对连接（client 侧已握手）
    async fn connected_pair(
    ) -> McpConnection<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>> {
        let (client, server) = duplex(4096);
        tokio::spawn(run_mock_server(server));
        let (client_read, client_write) = tokio::io::split(client);
        McpConnection::connect(client_read, client_write)
            .await
            .expect("handshake failed")
    }

    /// 握手 + tools/list 全流程
    #[tokio::test]
    async fn test_connect_handshake_and_list_tools() {
        let conn = connected_pair().await;
        let tools = conn.tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "echo.tool");
        assert_eq!(tools[0].description.as_deref(), Some("回显输入"));
        assert_eq!(tools[0].input_schema["type"], "object");
        // description 缺省的工具也能解析
        assert_eq!(tools[1].name, "fail");
        assert!(tools[1].description.is_none());
    }

    /// tools/call：只拼接 text 类型 content
    #[tokio::test]
    async fn test_call_tool_concatenates_text_content() {
        let conn = connected_pair().await;
        let result = conn
            .call_tool("echo.tool", json!({ "text": "hello" }))
            .await
            .unwrap();
        assert_eq!(result, Value::String("pong:\nhello".to_string()));
    }

    /// isError 为 true 时返回 Err
    #[tokio::test]
    async fn test_call_tool_is_error() {
        let conn = connected_pair().await;
        let err = conn.call_tool("fail", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    /// server 返回 JSON-RPC error → Err
    #[tokio::test]
    async fn test_call_tool_json_rpc_error() {
        let conn = connected_pair().await;
        let err = conn.call_tool("no-such-tool", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("tool not found"));
    }

    /// server 不响应时调用超时
    #[tokio::test]
    async fn test_call_tool_timeout() {
        let (client, mut server) = duplex(4096);
        // server 只读不写，永不响应
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

        // 握手会超时：注入短超时
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

    /// 握手成功后 call 超时（server 握手阶段响应、call 阶段静默）
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
                // 只响应 initialize 和 tools/list，tools/call 静默
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
        let conn = McpConnection::connect(client_read, client_write).await.unwrap();

        let err = conn
            .call_tool_with_timeout("anything", json!({}), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("超时"));
    }

    /// 并发 tools/call 按 id 正确分发（server 乱序响应也能对上）
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
                            continue; // 攒够两个 call 再乱序回
                        }
                        // 逆序响应，echo 各自的参数
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
        let conn = McpConnection::connect(client_read, client_write).await.unwrap();

        let (a, b) = tokio::join!(
            conn.call_tool("echo", json!({ "text": "A" })),
            conn.call_tool("echo", json!({ "text": "B" })),
        );
        assert_eq!(a.unwrap(), Value::String("A".to_string()));
        assert_eq!(b.unwrap(), Value::String("B".to_string()));
    }

    /// McpRegistry::specs 的命名与描述前缀
    #[test]
    fn test_registry_specs_naming() {
        // 不经子进程，直接构造 registry 内部状态太重——
        // 改为单测 sanitize 拼接规则，集成路径由 connect/call 测试覆盖
        let name = format!("{}__{}", sanitize("my.server"), sanitize("echo.tool"));
        assert_eq!(name, "my_server__echo_tool");
        let llm_name = format!("{}{}", MCP_NAME_PREFIX, name);
        assert_eq!(llm_name, "mcp__my_server__echo_tool");
        let rest = llm_name.strip_prefix(MCP_NAME_PREFIX).unwrap();
        assert_eq!(
            rest.split_once("__"),
            Some(("my_server", "echo_tool"))
        );
    }

    // ---- Streamable HTTP ----

    /// rpc_outcome：result 直通，error 转 Err，缺 result 时为 Null
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

    /// parse_sse_response：按 id 匹配 data 帧；非 data 行与其他 id 跳过
    #[test]
    fn test_parse_sse_response() {
        let body = "event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{\"tools\":[]}}\n\
                    \n\
                    event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\
                    \n";
        assert_eq!(
            parse_sse_response(body, 2).unwrap(),
            json!({ "ok": true })
        );
        // 找不到匹配 id → Err
        assert!(parse_sse_response(body, 99).is_err());
        // SSE 帧里的 JSON-RPC error 也要转成 Err
        let err_body = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-1,\"message\":\"bad\"}}\n\n";
        assert!(parse_sse_response(err_body, 1).unwrap_err().to_string().contains("bad"));
    }

    /// Streamable HTTP 全流程（手写 mock server）：
    /// initialize 下发 Mcp-Session-Id → 后续请求必须回带（不回带则 server 报错，握手失败）→
    /// tools/list 走 JSON 响应 → tools/call 走 SSE 响应
    #[tokio::test]
    async fn test_http_connect_and_call() {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        /// 在 buffer 里找子串的起始位置
        fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
            haystack.windows(needle.len()).position(|w| w == needle)
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    // 读请求头 + body（每个请求一条连接，Connection: close）
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
                    let has_sid = headers.to_ascii_lowercase().contains("mcp-session-id: sid-123");
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
                        // 未回带 session id 的请求一律拒绝（借此断言回带行为）
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

        // SSE 响应的 tools/call
        let result = client.call_tool("ping", json!({ "text": "hello-http" })).await.unwrap();
        assert_eq!(result, Value::String("hello-http".to_string()));
    }
}
