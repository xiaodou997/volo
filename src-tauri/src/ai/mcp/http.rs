use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};
use tokio::time::timeout;

use crate::error::{Result, VoloError};

use super::protocol::{
    extract_tool_text, parse_sse_response, parse_tools_list, rpc_outcome, McpToolInfo,
    PROTOCOL_VERSION,
};
use super::{CALL_TIMEOUT, CONNECT_TIMEOUT};

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
    /// 建立连接：initialize 握手 → notifications/initialized → tools/list（默认 10s 超时）。
    pub async fn connect(url: &str) -> Result<Self> {
        timeout(CONNECT_TIMEOUT, Self::handshake(url))
            .await
            .map_err(|_| {
                VoloError::Other(format!(
                    "MCP HTTP 握手超时（{} 秒）",
                    CONNECT_TIMEOUT.as_secs()
                ))
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

        client
            .notify("notifications/initialized", json!({}))
            .await?;

        let result = client.request("tools/list", json!({})).await?;
        let tools = parse_tools_list(&result);

        Ok(Self { tools, ..client })
    }

    /// 调用工具：tools/call（30s 超时）；提取逻辑与 stdio 一致。
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let fut = async {
            let result = self
                .request(
                    "tools/call",
                    json!({ "name": name, "arguments": arguments }),
                )
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

    /// 发 JSON-RPC request 并等响应（按自增 id 匹配；JSON 或 SSE 响应都支持）。
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let resp = self.post(&msg).await?;

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

    /// 发 JSON-RPC notification（无 id；server 应回 202/2xx 空响应）。
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
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .json(msg);
        if let Some(sid) = self.session_id.lock().ok().and_then(|s| s.clone()) {
            req = req.header("mcp-session-id", sid);
        }
        req.send()
            .await
            .map_err(|e| VoloError::Other(format!("MCP HTTP 请求失败（{}）: {}", self.url, e)))
    }

    /// 已握手拿到的工具列表。
    pub fn tools(&self) -> &[McpToolInfo] {
        &self.tools
    }
}
