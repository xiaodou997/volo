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
}

impl Message {
    pub fn system(content: &str) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
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
        }
    }

    pub fn tool_result(tool_call_id: String, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        }
    }

    /// 转换为 OpenAI wire 格式（tool_calls 的 arguments 需序列化为 JSON 字符串）
    pub fn to_wire(&self) -> Value {
        let mut msg = json!({ "role": self.role });

        // tool 角色必须带 content（哪怕是空串），其余角色 content 为 None 时不带该字段
        if self.role == "tool" {
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
}

impl ChatBackend for OpenAiBackend {
    fn chat<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [ToolSpec],
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
        Box::pin(async move {
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

            let resp = self
                .client
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(&self.api_key)
                .json(&body)
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
