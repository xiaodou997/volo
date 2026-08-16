//! Agent 会话原型
//! 会话循环：chat → 有 tool_calls 则经 ToolRegistry 执行并回填 → 继续，
//! 无 tool_calls 则结束；全程向主窗口 emit `agent-event`

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use super::llm::{ChatBackend, Message, OpenAiBackend};
use super::tools::{RegistryExecutor, ToolRegistry};
use crate::core::config::Config;
use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};

/// 最大对话轮数，防止失控循环
pub const MAX_ROUNDS: usize = 8;

const SYSTEM_PROMPT: &str = "你是 Volo 启动器的内置助手，\
可以调用工具帮用户完成桌面操作：clipboard_read 读取剪贴板、\
fs_read 读取文本文件、notification_show 发送系统通知。\
原则：谨慎行事，先读后写；涉及用户数据的操作说明理由；\
工具返回错误时向用户解释原因并给出替代建议。";

/// 事件类型（serde snake_case：message/tool_call/tool_result/done/error）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventKind {
    Message,
    ToolCall,
    ToolResult,
    Done,
    Error,
}

/// `agent-event` 事件 payload（camelCase）
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
}

impl AgentEvent {
    fn simple(kind: AgentEventKind) -> Self {
        Self {
            kind,
            content: None,
            name: None,
            args: None,
            result: None,
        }
    }

    fn message(content: String) -> Self {
        Self {
            kind: AgentEventKind::Message,
            content: Some(content),
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

/// 工具执行器抽象：让会话循环不依赖 Tauri AppHandle，便于测试
pub trait ToolExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;
}

/// Agent 会话循环（纯异步函数，emit 以闭包注入）
///
/// 所有退出路径最后都会 emit `done`；致命错误在此之前 emit `error`。
pub async fn run_agent_loop(
    backend: &dyn ChatBackend,
    executor: &dyn ToolExecutor,
    query: &str,
    mut emit: impl FnMut(AgentEvent) + Send,
    cancel: &AtomicBool,
) {
    let mut messages = vec![Message::system(SYSTEM_PROMPT), Message::user(query)];
    let tools = ToolRegistry::specs();

    for _round in 0..MAX_ROUNDS {
        if cancel.load(Ordering::Relaxed) {
            emit(AgentEvent::simple(AgentEventKind::Done));
            return;
        }

        let response = match backend.chat(&messages, &tools).await {
            Ok(resp) => resp,
            Err(e) => {
                emit(AgentEvent::error(e.to_string()));
                emit(AgentEvent::simple(AgentEventKind::Done));
                return;
            }
        };

        if response.tool_calls.is_empty() {
            let answer = response.content.unwrap_or_default();
            if !answer.is_empty() {
                emit(AgentEvent::message(answer));
            }
            emit(AgentEvent::simple(AgentEventKind::Done));
            return;
        }

        // assistant 消息（含 tool_calls）入历史，再逐个执行工具
        messages.push(Message::assistant(
            response.content.clone(),
            response.tool_calls.clone(),
        ));

        for call in &response.tool_calls {
            emit(AgentEvent {
                kind: AgentEventKind::ToolCall,
                name: Some(call.name.clone()),
                args: Some(call.arguments.clone()),
                ..AgentEvent::simple(AgentEventKind::ToolCall)
            });

            // 出错/权限被拒不终止会话：把错误文本作为 tool 结果回填给 LLM
            let result_text = match executor.execute(&call.name, call.arguments.clone()).await {
                Ok(value) => value_to_text(&value),
                Err(e) => format!("Error: {}", e),
            };

            emit(AgentEvent {
                kind: AgentEventKind::ToolResult,
                name: Some(call.name.clone()),
                result: Some(result_text.clone()),
                ..AgentEvent::simple(AgentEventKind::ToolResult)
            });

            messages.push(Message::tool_result(call.id.clone(), result_text));
        }
    }

    emit(AgentEvent::error(format!(
        "已达到最大轮数（{}），会话终止",
        MAX_ROUNDS
    )));
    emit(AgentEvent::simple(AgentEventKind::Done));
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// Agent 会话管理器（Tauri managed state）
pub struct AgentManager {
    cancel: Arc<AtomicBool>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============ Tauri Commands ============

/// 发起一次 Agent 会话：立即返回，进度经 `agent-event` 事件推送
#[tauri::command]
pub fn agent_ask(
    app: AppHandle,
    manager: State<'_, AgentManager>,
    config: State<'_, Config>,
    query: String,
) -> Result<()> {
    let llm = config.get().llm;
    if llm.model.trim().is_empty() {
        return Err(VoloError::Other("请先在设置中配置 LLM（模型未填写）".to_string()));
    }
    if llm.api_key.trim().is_empty() {
        return Err(VoloError::Other("请先在设置中配置 LLM API Key".to_string()));
    }
    let api_key = llm.api_key.clone();

    let backend = OpenAiBackend::new(llm.base_url, llm.model, api_key);
    let cancel = manager.cancel_flag();
    // 新会话开始前复位取消标志
    cancel.store(false, Ordering::Relaxed);

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let engine = app_handle.state::<PermissionEngine>();
        let executor = RegistryExecutor {
            app: &app_handle,
            engine: &engine,
        };
        let emit = |event: AgentEvent| {
            let _ = app_handle.emit("agent-event", &event);
        };
        run_agent_loop(&backend, &executor, &query, emit, &cancel).await;
    });

    Ok(())
}

/// 取消当前 Agent 会话（下一轮循环前生效）
#[tauri::command]
pub fn agent_cancel(manager: State<'_, AgentManager>) {
    manager.cancel_flag().store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::llm::{ChatResponse, ToolCall};
    use super::super::tools::ToolSpec;
    use serde_json::json;
    use std::sync::Mutex;

    struct MockBackend {
        responses: Mutex<Vec<ChatResponse>>,
        seen: Mutex<Vec<Vec<Message>>>,
    }

    impl MockBackend {
        fn new(responses: Vec<ChatResponse>) -> Self {
            // 反序存放，pop 即按序取
            Self {
                responses: Mutex::new(responses.into_iter().rev().collect()),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl ChatBackend for MockBackend {
        fn chat<'a>(
            &'a self,
            messages: &'a [Message],
            _tools: &'a [ToolSpec],
        ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
            Box::pin(async move {
                self.seen.lock().unwrap().push(messages.to_vec());
                self.responses
                    .lock()
                    .unwrap()
                    .pop()
                    .ok_or_else(|| VoloError::Other("no more mock responses".to_string()))
            })
        }
    }

    struct MockExecutor {
        result: Result<Value>,
        calls: Mutex<Vec<(String, Value)>>,
    }

    impl MockExecutor {
        fn ok(value: Value) -> Self {
            Self {
                result: Ok(value),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn err(message: &str) -> Self {
            Self {
                result: Err(VoloError::PermissionDenied(message.to_string())),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl ToolExecutor for MockExecutor {
        fn execute<'a>(
            &'a self,
            name: &'a str,
            args: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((name.to_string(), args));
                match &self.result {
                    Ok(v) => Ok(v.clone()),
                    Err(e) => Err(VoloError::Other(e.to_string())),
                }
            })
        }
    }

    fn tool_call(id: &str, name: &str, args: Value) -> ChatResponse {
        ChatResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: args,
            }],
        }
    }

    fn final_answer(text: &str) -> ChatResponse {
        ChatResponse {
            content: Some(text.to_string()),
            tool_calls: vec![],
        }
    }

    fn kinds(events: &[AgentEvent]) -> Vec<AgentEventKind> {
        events.iter().map(|e| e.kind).collect()
    }

    /// 完整回路：第一轮 tool_call → 执行回填 → 第二轮最终回答
    #[tokio::test]
    async fn test_loop_tool_call_then_answer() {
        let backend = MockBackend::new(vec![
            tool_call("call_1", "clipboard_read", json!({})),
            final_answer("剪贴板里是：你好"),
        ]);
        let executor = MockExecutor::ok(json!("你好"));
        let cancel = AtomicBool::new(false);

        let mut events = Vec::new();
        run_agent_loop(&backend, &executor, "剪贴板里有什么", |e| events.push(e), &cancel).await;

        assert_eq!(
            kinds(&events),
            vec![
                AgentEventKind::ToolCall,
                AgentEventKind::ToolResult,
                AgentEventKind::Message,
                AgentEventKind::Done,
            ]
        );
        assert_eq!(events[0].name.as_deref(), Some("clipboard_read"));
        assert_eq!(events[1].result.as_deref(), Some("你好"));
        assert_eq!(events[2].content.as_deref(), Some("剪贴板里是：你好"));

        // 第二轮请求里 tool 结果已回填
        let seen = backend.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let second = &seen[1];
        // system + user + assistant(tool_calls) + tool
        assert_eq!(second.len(), 4);
        assert_eq!(second[2].role, "assistant");
        assert_eq!(second[3].role, "tool");
        assert_eq!(second[3].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(second[3].content.as_deref(), Some("你好"));

        // 工具确实被执行
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "clipboard_read");
    }

    /// 权限被拒：错误文本作为 tool 结果回填，会话继续
    #[tokio::test]
    async fn test_loop_permission_denied_backfills_error() {
        let backend = MockBackend::new(vec![
            tool_call("call_1", "fs_read", json!({"path": "/etc/passwd"})),
            final_answer("你没有批准读取该文件"),
        ]);
        let executor = MockExecutor::err("Permission 'fs.read' denied by user");
        let cancel = AtomicBool::new(false);

        let mut events = Vec::new();
        run_agent_loop(&backend, &executor, "读一下密码文件", |e| events.push(e), &cancel).await;

        assert_eq!(
            kinds(&events),
            vec![
                AgentEventKind::ToolCall,
                AgentEventKind::ToolResult,
                AgentEventKind::Message,
                AgentEventKind::Done,
            ]
        );

        // tool 结果里是错误文本，且已回填进第二轮消息
        let result = events[1].result.as_deref().unwrap();
        assert!(result.starts_with("Error:"));
        assert!(result.contains("denied"));

        let seen = backend.seen.lock().unwrap();
        assert_eq!(seen[1][3].role, "tool");
        assert!(seen[1][3].content.as_deref().unwrap().contains("denied"));
    }

    /// 8 轮上限：LLM 持续要求调工具时强制终止
    #[tokio::test]
    async fn test_loop_max_rounds_terminates() {
        let backend = MockBackend::new(
            (0..MAX_ROUNDS)
                .map(|i| tool_call(&format!("call_{}", i), "clipboard_read", json!({})))
                .collect(),
        );
        let executor = MockExecutor::ok(json!("x"));
        let cancel = AtomicBool::new(false);

        let mut events = Vec::new();
        run_agent_loop(&backend, &executor, "循环", |e| events.push(e), &cancel).await;

        // 每个 mock 响应恰用一轮，8 轮后强制结束
        assert_eq!(backend.seen.lock().unwrap().len(), MAX_ROUNDS);
        let event_kinds = kinds(&events);
        assert!(event_kinds.contains(&AgentEventKind::Error));
        assert_eq!(event_kinds.last(), Some(&AgentEventKind::Done));
        assert!(!event_kinds.contains(&AgentEventKind::Message));

        let error = events
            .iter()
            .find(|e| e.kind == AgentEventKind::Error)
            .unwrap();
        assert!(error.content.as_deref().unwrap().contains("最大轮数"));
    }

    /// 取消标志置位后，下一轮前退出并 emit done
    #[tokio::test]
    async fn test_loop_cancel_before_first_round() {
        let backend = MockBackend::new(vec![final_answer("不应到达")]);
        let executor = MockExecutor::ok(json!("x"));
        let cancel = AtomicBool::new(true);

        let mut events = Vec::new();
        run_agent_loop(&backend, &executor, "q", |e| events.push(e), &cancel).await;

        assert_eq!(kinds(&events), vec![AgentEventKind::Done]);
        assert!(backend.seen.lock().unwrap().is_empty());
    }

    /// 后端出错：emit error + done
    #[tokio::test]
    async fn test_loop_backend_error() {
        let backend = MockBackend::new(vec![]); // 第一轮就报错
        let executor = MockExecutor::ok(json!("x"));
        let cancel = AtomicBool::new(false);

        let mut events = Vec::new();
        run_agent_loop(&backend, &executor, "q", |e| events.push(e), &cancel).await;

        assert_eq!(
            kinds(&events),
            vec![AgentEventKind::Error, AgentEventKind::Done]
        );
    }
}
