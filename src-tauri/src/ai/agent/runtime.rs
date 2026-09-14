//! Agent 纯运行时：事件协议与 tool-calling loop。
//!
//! 本模块刻意不依赖 Tauri State / 会话持久化，让运行循环可以独立测试。

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use serde_json::{json, Value};

use crate::ai::llm::{ChatBackend, Message};
use crate::ai::tool_executor::ToolExecutor;
use crate::ai::tools::ToolSpec;

/// 最大对话轮数，防止失控循环。
pub const MAX_ROUNDS: usize = 8;

/// 事件类型（serde snake_case：message/tool_call/tool_result/done/error）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventKind {
    Message,
    ToolCall,
    ToolResult,
    Done,
    Error,
}

/// `agent-event` 事件 payload（camelCase）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub kind: AgentEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// true 表示 content 是流式增量片段，前端应追加到当前气泡。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<bool>,
}

impl AgentEvent {
    fn simple(kind: AgentEventKind) -> Self {
        Self {
            kind,
            content: None,
            name: None,
            args: None,
            result: None,
            delta: None,
        }
    }

    fn message(content: String) -> Self {
        Self {
            kind: AgentEventKind::Message,
            content: Some(content),
            ..Self::simple(AgentEventKind::Message)
        }
    }

    fn delta_message(delta: String) -> Self {
        Self {
            kind: AgentEventKind::Message,
            content: Some(delta),
            delta: Some(true),
            ..Self::simple(AgentEventKind::Message)
        }
    }

    fn error(content: String) -> Self {
        Self {
            kind: AgentEventKind::Error,
            content: Some(content),
            ..Self::simple(AgentEventKind::Error)
        }
    }
}

/// Agent 会话循环。
///
/// 调用方维护 messages / logging / event emission；本函数只负责模型与工具之间的循环。
pub async fn run_agent_loop(
    backend: &dyn ChatBackend,
    executor: &dyn ToolExecutor,
    messages: &mut Vec<Message>,
    tools: &[ToolSpec],
    mut emit: impl FnMut(AgentEvent) + Send,
    cancel: &AtomicBool,
    mut log: Option<&mut (dyn FnMut(&str, &Value) + Send)>,
) {
    let mut log_event = |kind: &str, payload: Value| {
        if let Some(log) = log.as_deref_mut() {
            log(kind, &payload);
        }
    };

    for _round in 0..MAX_ROUNDS {
        if cancel.load(Ordering::Relaxed) {
            log_event("done", json!({ "reason": "cancelled" }));
            emit(AgentEvent::simple(AgentEventKind::Done));
            return;
        }

        let mut streamed = false;
        let response = match backend
            .chat_stream(messages, tools, &mut |delta| {
                streamed = true;
                emit(AgentEvent::delta_message(delta));
                !cancel.load(Ordering::Relaxed)
            })
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                log_event("error", json!({ "message": e.to_string() }));
                emit(AgentEvent::error(e.to_string()));
                log_event("done", json!({ "reason": "error" }));
                emit(AgentEvent::simple(AgentEventKind::Done));
                return;
            }
        };

        if cancel.load(Ordering::Relaxed) {
            log_event("done", json!({ "reason": "cancelled" }));
            emit(AgentEvent::simple(AgentEventKind::Done));
            return;
        }

        log_event(
            "model_response",
            json!({
                "content": response.content,
                "tool_calls": response
                    .tool_calls
                    .iter()
                    .map(|call| call.name.clone())
                    .collect::<Vec<_>>(),
            }),
        );

        if response.tool_calls.is_empty() {
            let answer = response.content.unwrap_or_default();
            if !answer.is_empty() {
                messages.push(Message::assistant(Some(answer.clone()), vec![]));
            }
            if !streamed && !answer.is_empty() {
                emit(AgentEvent::message(answer));
            }
            log_event("done", json!({ "reason": "completed" }));
            emit(AgentEvent::simple(AgentEventKind::Done));
            return;
        }

        messages.push(Message::assistant(
            response.content.clone(),
            response.tool_calls.clone(),
        ));

        for call in &response.tool_calls {
            log_event(
                "tool_call",
                json!({ "name": call.name, "args": call.arguments }),
            );
            emit(AgentEvent {
                kind: AgentEventKind::ToolCall,
                name: Some(call.name.clone()),
                args: Some(call.arguments.clone()),
                ..AgentEvent::simple(AgentEventKind::ToolCall)
            });

            let result_text = match executor.execute(&call.name, call.arguments.clone()).await {
                Ok(value) => value_to_text(&value),
                Err(e) => format!("Error: {}", e),
            };

            log_event(
                "tool_result",
                json!({ "name": call.name, "result": truncate_summary(&result_text) }),
            );
            emit(AgentEvent {
                kind: AgentEventKind::ToolResult,
                name: Some(call.name.clone()),
                result: Some(result_text.clone()),
                ..AgentEvent::simple(AgentEventKind::ToolResult)
            });

            messages.push(Message::tool_result(call.id.clone(), result_text));
        }
    }

    let message = format!("已达到最大轮数（{}），会话终止", MAX_ROUNDS);
    log_event("error", json!({ "message": message }));
    emit(AgentEvent::error(message));
    log_event("done", json!({ "reason": "max_rounds" }));
    emit(AgentEvent::simple(AgentEventKind::Done));
}

fn truncate_summary(text: &str) -> String {
    const MAX: usize = 500;
    if text.chars().count() <= MAX {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(MAX).collect::<String>())
    }
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}
