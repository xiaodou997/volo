use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::warn;

use crate::error::{Result, VoloError};

use super::protocol::{
    extract_tool_text, parse_tools_list, rpc_outcome, McpToolInfo, PROTOCOL_VERSION,
};
use super::{CALL_TIMEOUT, CONNECT_TIMEOUT};

/// 一条 MCP stdio 连接：写端 + pending 分发表；读端由后台 task 消费。
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
    /// 建立连接：initialize 握手 → notifications/initialized → tools/list（默认 10s 超时）。
    pub async fn connect(reader: R, writer: W) -> Result<Self> {
        Self::connect_with_timeout(reader, writer, CONNECT_TIMEOUT).await
    }

    /// 同 connect，超时可注入（测试用）。
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
                            continue;
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
                    Ok(None) => break,
                    Err(e) => {
                        warn!("MCP: 读 stdio 失败: {}", e);
                        break;
                    }
                }
            }

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

        conn.notify("notifications/initialized", json!({})).await?;

        let result = conn.request("tools/list", json!({})).await?;
        let tools = parse_tools_list(&result);

        Ok(Self { tools, ..conn })
    }

    /// 调用工具：tools/call（默认 30s 超时）。
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        self.call_tool_with_timeout(name, arguments, CALL_TIMEOUT)
            .await
    }

    /// 同 call_tool，超时可注入（测试用）。
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

    /// 发一个 JSON-RPC request 并等响应（按自增 id 匹配）。
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

    /// 发一个 JSON-RPC notification（无 id，无响应）。
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

    /// 已握手拿到的工具列表。
    pub fn tools(&self) -> &[McpToolInfo] {
        &self.tools
    }
}
