//! LLM provider 抽象
//! OpenAI 兼容协议（/chat/completions + tools），一套代码覆盖
//! OpenAI / DeepSeek / 通义 / 本地 Ollama 等

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::tools::ToolSpec;
use crate::error::{Result, VoloError};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// 对话消息（内部表示，OpenAI 兼容语义的超集）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 图片附件（data URL，如 data:image/png;base64,...）。仅 user 消息使用；
    /// 空数组不序列化，旧会话日志兼容
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl Message {
    pub fn system(content: &str) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    pub fn user(content: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    /// 带图片附件的 user 消息（粘贴截图等场景）
    pub fn user_with_images(content: &str, images: Vec<String>) -> Self {
        Self {
            images,
            ..Self::user(content)
        }
    }

    pub fn assistant(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    pub fn tool_result(tool_call_id: String, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
            images: Vec::new(),
        }
    }

    /// 转换为 OpenAI wire 格式（tool_calls 的 arguments 需序列化为 JSON 字符串；
    /// user 消息带图片时 content 转多模态 parts 数组，其余情况保持纯文本字段）
    pub fn to_wire(&self) -> Value {
        let mut msg = json!({ "role": self.role });

        if self.role == "user" && !self.images.is_empty() {
            // vision 格式：[{type: text}, {type: image_url}...]
            let mut parts = vec![json!({
                "type": "text",
                "text": self.content.clone().unwrap_or_default(),
            })];
            for image in &self.images {
                parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": image },
                }));
            }
            msg["content"] = Value::Array(parts);
        } else if self.role == "tool" {
            // tool 角色必须带 content（哪怕是空串），其余角色 content 为 None 时不带该字段
            msg["content"] = Value::String(self.content.clone().unwrap_or_default());
        } else if let Some(content) = &self.content {
            msg["content"] = Value::String(content.clone());
        }

        if let Some(tool_calls) = &self.tool_calls {
            msg["tool_calls"] = Value::Array(
                tool_calls
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments.to_string(),
                            },
                        })
                    })
                    .collect(),
            );
        }

        if let Some(tool_call_id) = &self.tool_call_id {
            msg["tool_call_id"] = Value::String(tool_call_id.clone());
        }

        msg
    }
}

/// 一次工具调用（内部表示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// LLM 一轮的回复
#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

/// 聊天后端抽象（不引入 async-trait，返回 boxed future）
pub trait ChatBackend: Send + Sync {
    fn chat<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [ToolSpec],
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>>;

    /// 流式聊天：content 增量通过 on_delta 实时回调，返回完整 ChatResponse。
    /// on_delta 返回 false 表示调用方要求中断（如用户取消），实现应尽快停止
    /// 读取流并返回；此时返回的 ChatResponse 是中断前已累积的部分内容。
    /// 默认实现回退为非流式：chat() 完成后一次性回调全部 content。
    fn chat_stream<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [ToolSpec],
        on_delta: &'a mut (dyn FnMut(String) -> bool + Send),
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
        Box::pin(async move {
            let resp = self.chat(messages, tools).await?;
            if let Some(content) = &resp.content {
                if !content.is_empty() {
                    on_delta(content.clone());
                }
            }
            Ok(resp)
        })
    }
}

/// SSE 流解析器：跨 chunk 维护行缓冲与 tool_calls 分片累积状态。
///
/// 逐次 feed() 字节流文本（允许在行中间切断），返回每段产出的 content deltas；
/// 流结束后 finish() 产出完整 ChatResponse。
#[derive(Default)]
pub struct SseAccumulator {
    /// 未遇到换行符的半行缓冲
    line_buf: String,
    /// 累积的完整 content
    content: String,
    /// 按 index 累积的 tool_calls 分片
    tool_calls: Vec<PartialToolCall>,
    /// 收到 `data: [DONE]`
    done: bool,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl SseAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一段文本，返回本次解析出的 content deltas（保持到达顺序）
    pub fn feed(&mut self, text: &str) -> Vec<String> {
        self.line_buf.push_str(text);
        let mut deltas = Vec::new();
        while let Some(pos) = self.line_buf.find('\n') {
            let line: String = self.line_buf.drain(..=pos).collect();
            self.handle_line(line.trim_end_matches(['\n', '\r']), &mut deltas);
        }
        deltas
    }

    /// 流结束：处理尾部半行，产出完整 ChatResponse
    pub fn finish(mut self) -> Result<ChatResponse> {
        if !self.line_buf.trim().is_empty() {
            let mut deltas = Vec::new();
            let line = std::mem::take(&mut self.line_buf);
            self.handle_line(line.trim_end_matches('\r'), &mut deltas);
            self.content.push_str(&deltas.concat());
        }

        let tool_calls = self
            .tool_calls
            .into_iter()
            .map(|call| ToolCall {
                id: call.id,
                name: call.name,
                arguments: serde_json::from_str(&call.arguments).unwrap_or(Value::Null),
            })
            .collect();

        let content = if self.content.is_empty() {
            None
        } else {
            Some(self.content)
        };

        Ok(ChatResponse {
            content,
            tool_calls,
        })
    }

    fn handle_line(&mut self, line: &str, deltas: &mut Vec<String>) {
        // 已收到 [DONE]，后续行忽略
        if self.done {
            return;
        }
        // 空行、注释行（`: ` 前缀）跳过
        let payload = match line.strip_prefix("data:") {
            Some(p) => p.trim_start(),
            None => return,
        };
        if payload == "[DONE]" {
            self.done = true;
            return;
        }

        let Ok(chunk) = serde_json::from_str::<Value>(payload) else {
            return; // 无法解析的行静默跳过（心跳等）
        };
        let Some(delta) = chunk
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
        else {
            return;
        };

        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                self.content.push_str(content);
                deltas.push(content.to_string());
            }
        }

        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                while self.tool_calls.len() <= index {
                    self.tool_calls.push(PartialToolCall::default());
                }
                let slot = &mut self.tool_calls[index];
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    slot.id.push_str(id);
                }
                if let Some(function) = call.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        slot.name.push_str(name);
                    }
                    if let Some(args) = function.get("arguments").and_then(Value::as_str) {
                        slot.arguments.push_str(args);
                    }
                }
            }
        }
    }
}

/// OpenAI 兼容后端
pub struct OpenAiBackend {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl OpenAiBackend {
    /// base_url 为空则使用 OpenAI 官方地址
    pub fn new(base_url: String, model: String, api_key: String) -> Self {
        let base_url = if base_url.trim().is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        Self {
            client: reqwest::Client::new(),
            base_url,
            model,
            api_key,
        }
    }

    fn request_body(&self, messages: &[Message], tools: &[ToolSpec], stream: bool) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": messages.iter().map(Message::to_wire).collect::<Vec<_>>(),
        });

        if !tools.is_empty() {
            body["tools"] = Value::Array(
                tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.parameters,
                            },
                        })
                    })
                    .collect(),
            );
        }

        if stream {
            body["stream"] = Value::Bool(true);
        }

        body
    }

    async fn send_request(&self, body: &Value) -> Result<reqwest::Response> {
        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await
            .map_err(|e| VoloError::Other(format!("LLM request failed: {}", e)))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(300).collect();
            return Err(VoloError::Other(format!(
                "LLM request failed ({}): {}",
                status, snippet
            )));
        }

        Ok(resp)
    }
}

impl ChatBackend for OpenAiBackend {
    fn chat<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [ToolSpec],
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
        Box::pin(async move {
            let body = self.request_body(messages, tools, false);
            let resp = self.send_request(&body).await?;

            let payload: Value = resp
                .json()
                .await
                .map_err(|e| VoloError::Other(format!("LLM response parse failed: {}", e)))?;

            let message = payload
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .ok_or_else(|| VoloError::Other("LLM response missing choices[0].message".into()))?;

            let content = message
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_string);

            let tool_calls = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| {
                            let function = call.get("function")?;
                            let name = function.get("name")?.as_str()?.to_string();
                            let arguments = function
                                .get("arguments")
                                .and_then(Value::as_str)
                                .and_then(|s| serde_json::from_str(s).ok())
                                .unwrap_or(Value::Null);
                            Some(ToolCall {
                                id: call.get("id")?.as_str()?.to_string(),
                                name,
                                arguments,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Ok(ChatResponse {
                content,
                tool_calls,
            })
        })
    }

    fn chat_stream<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [ToolSpec],
        on_delta: &'a mut (dyn FnMut(String) -> bool + Send),
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
        use futures_util::StreamExt;

        Box::pin(async move {
            let body = self.request_body(messages, tools, true);
            let resp = self.send_request(&body).await?;

            let mut acc = SseAccumulator::new();
            let mut stream = resp.bytes_stream();
            // on_delta 返回 false（如用户取消）则停止读取，返回已累积的部分内容
            'outer: while let Some(chunk) = stream.next().await {
                let bytes = chunk
                    .map_err(|e| VoloError::Other(format!("LLM stream read failed: {}", e)))?;
                let text = String::from_utf8_lossy(&bytes);
                for delta in acc.feed(&text) {
                    if !on_delta(delta) {
                        break 'outer;
                    }
                }
            }

            acc.finish()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// user 消息带图片时 content 走多模态 parts；不带则保持纯文本字段
    #[test]
    fn test_message_wire_format_images() {
        let msg = Message::user_with_images(
            "这张图里有什么",
            vec!["data:image/png;base64,AAAA".to_string()],
        );
        let wire = msg.to_wire();
        assert_eq!(wire["role"], "user");
        let parts = wire["content"].as_array().expect("应为 parts 数组");
        assert_eq!(parts[0], json!({ "type": "text", "text": "这张图里有什么" }));
        assert_eq!(
            parts[1],
            json!({ "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } })
        );

        // 无图片：仍是纯文本 content，wire 格式不变
        let plain = Message::user("纯文本").to_wire();
        assert_eq!(plain["content"], Value::String("纯文本".to_string()));

        // serde 兼容：images 空数组不序列化，旧日志（无 images 字段）可正常读回
        let log_json = serde_json::to_string(&Message::user("x")).unwrap();
        assert!(!log_json.contains("images"));
        let old: Message = serde_json::from_str(r#"{"role":"user","content":"旧格式"}"#).unwrap();
        assert!(old.images.is_empty());
        assert_eq!(old.content.as_deref(), Some("旧格式"));
    }

    #[test]
    fn test_message_wire_format_tool_calls() {
        let msg = Message::assistant(
            None,
            vec![ToolCall {
                id: "call_1".to_string(),
                name: "fs_read".to_string(),
                arguments: json!({"path": "/tmp/a.txt"}),
            }],
        );
        let wire = msg.to_wire();
        assert_eq!(wire["role"], "assistant");
        assert!(wire.get("content").is_none());
        assert_eq!(wire["tool_calls"][0]["type"], "function");
        assert_eq!(wire["tool_calls"][0]["function"]["name"], "fs_read");
        // arguments 必须是 JSON 字符串
        assert_eq!(
            wire["tool_calls"][0]["function"]["arguments"],
            r#"{"path":"/tmp/a.txt"}"#
        );
    }

    #[test]
    fn test_tool_result_wire_format() {
        let msg = Message::tool_result("call_1".to_string(), "文件内容".to_string());
        let wire = msg.to_wire();
        assert_eq!(wire["role"], "tool");
        assert_eq!(wire["tool_call_id"], "call_1");
        assert_eq!(wire["content"], "文件内容");
    }

    #[test]
    fn test_default_base_url() {
        let backend = OpenAiBackend::new(String::new(), "gpt-4o".to_string(), "k".to_string());
        assert_eq!(backend.base_url, DEFAULT_BASE_URL);

        let backend =
            OpenAiBackend::new("https://api.deepseek.com/v1/".to_string(), "m".into(), "k".into());
        assert_eq!(backend.base_url, "https://api.deepseek.com/v1");
    }

    // ============ SSE 解析 ============

    fn sse_content_chunk(content: &str) -> String {
        format!(
            "data: {}\n\n",
            json!({"choices": [{"delta": {"content": content}}]})
        )
    }

    #[test]
    fn test_sse_content_deltas_in_order() {
        let mut acc = SseAccumulator::new();
        let mut deltas = Vec::new();
        deltas.extend(acc.feed(&sse_content_chunk("你好")));
        deltas.extend(acc.feed(&sse_content_chunk("，")));
        deltas.extend(acc.feed(&sse_content_chunk("世界")));
        deltas.extend(acc.feed("data: [DONE]\n\n"));

        assert_eq!(deltas, vec!["你好", "，", "世界"]);
        let resp = acc.finish().unwrap();
        assert_eq!(resp.content.as_deref(), Some("你好，世界"));
        assert!(resp.tool_calls.is_empty());
    }

    #[test]
    fn test_sse_chunk_split_mid_line() {
        let mut acc = SseAccumulator::new();
        let chunk = sse_content_chunk("abc");
        // 在行中间切断喂入
        let (a, b) = chunk.split_at(chunk.len() / 2);
        assert!(acc.feed(a).is_empty());
        assert_eq!(acc.feed(b), vec!["abc"]);
        assert_eq!(acc.finish().unwrap().content.as_deref(), Some("abc"));
    }

    #[test]
    fn test_sse_tool_calls_fragmented_merge() {
        let mut acc = SseAccumulator::new();
        // 第一个分片：id + name；后续分片：arguments 分段到达
        let chunks = [
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "call_1", "function": {"name": "fs_read", "arguments": ""}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "{\"path\":"}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "\"/tmp/a.txt\"}"}}
            ]}}]}),
        ];
        for chunk in chunks {
            assert!(acc.feed(&format!("data: {}\n\n", chunk)).is_empty());
        }
        acc.feed("data: [DONE]\n\n");

        let resp = acc.finish().unwrap();
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_1");
        assert_eq!(resp.tool_calls[0].name, "fs_read");
        assert_eq!(resp.tool_calls[0].arguments, json!({"path": "/tmp/a.txt"}));
        assert!(resp.content.is_none());
    }

    #[test]
    fn test_sse_done_terminates_and_skips_following() {
        let mut acc = SseAccumulator::new();
        acc.feed("data: [DONE]\n\n");
        // [DONE] 之后的行被忽略
        assert!(acc.feed(&sse_content_chunk("不应出现")).is_empty());
        assert!(acc.finish().unwrap().content.is_none());
    }

    #[test]
    fn test_sse_skips_comments_empty_and_garbage_lines() {
        let mut acc = SseAccumulator::new();
        let deltas = acc.feed(": keep-alive\n\nevent: message\nnot json at all\n");
        assert!(deltas.is_empty());
        let deltas = acc.feed(&sse_content_chunk("ok"));
        assert_eq!(deltas, vec!["ok"]);
    }

    /// 默认 chat_stream：回退为一次性回调完整 content
    #[tokio::test]
    async fn test_default_chat_stream_fallback() {
        struct EchoBackend;
        impl ChatBackend for EchoBackend {
            fn chat<'a>(
                &'a self,
                _messages: &'a [Message],
                _tools: &'a [ToolSpec],
            ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
                Box::pin(async move {
                    Ok(ChatResponse {
                        content: Some("完整回答".to_string()),
                        tool_calls: vec![],
                    })
                })
            }
        }

        let backend = EchoBackend;
        let mut deltas = Vec::new();
        let resp = backend
            .chat_stream(&[], &[], &mut |d| {
                deltas.push(d);
                true
            })
            .await
            .unwrap();
        assert_eq!(deltas, vec!["完整回答"]);
        assert_eq!(resp.content.as_deref(), Some("完整回答"));
    }
}
