//! Agent 会话原型
//! 会话循环：chat → 有 tool_calls 则经 ToolRegistry 执行并回填 → 继续，
//! 无 tool_calls 则结束；全程向主窗口 emit `agent-event`

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use super::llm::{ChatBackend, Message, OpenAiBackend};
use super::plugin_tools::{collect_specs, PluginToolExecutor, PluginToolState};
use super::session::{cleanup_old_sessions, sessions_dir, SessionLog, SESSION_RETENTION_DAYS};
use super::tools::{ToolRegistry, ToolSpec};
use crate::core::config::Config;
use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

/// 最大对话轮数，防止失控循环
pub const MAX_ROUNDS: usize = 8;

const SYSTEM_PROMPT: &str = "你是 Volo 启动器的内置助手，\
可以调用工具帮用户完成任务：内置工具有 clipboard_read 读取剪贴板、\
fs_read 读取文本文件、notification_show 发送系统通知；\
此外还有插件贡献的工具（名字形如 plugin__tool），以请求中携带的 tools 列表为准。\
原则：用户请求与某个工具能力匹配时，必须调用工具获取真实结果，不要凭记忆编造；\
谨慎行事，先读后写；涉及用户数据的操作说明理由；\
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
    /// 流式增量标记：true 表示 content 是增量片段，前端应追加到当前气泡
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

    /// 流式增量消息
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
/// - 模型回复走 chat_stream：content 增量实时 emit `message`（delta: true），
///   一轮结束后不再重复 emit 该轮完整 message（前端把 delta 拼成完整气泡）
/// - log 为可选的会话日志回调（kind + payload），关键节点：
///   user_input / model_response / tool_call / tool_result / error / done
/// - tools 为本次会话可见的工具规格（内置 + 插件贡献），每轮原样传给模型
/// - 所有退出路径最后都会 emit `done`；致命错误在此之前 emit `error`
pub async fn run_agent_loop(
    backend: &dyn ChatBackend,
    executor: &dyn ToolExecutor,
    query: &str,
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

    log_event("user_input", json!({ "query": query }));

    let mut messages = vec![Message::system(SYSTEM_PROMPT), Message::user(query)];

    for _round in 0..MAX_ROUNDS {
        if cancel.load(Ordering::Relaxed) {
            log_event("done", json!({ "reason": "cancelled" }));
            emit(AgentEvent::simple(AgentEventKind::Done));
            return;
        }

        // 流式请求：content 增量实时 emit，本轮是否有增量决定结尾是否补发完整消息
        let mut streamed = false;
        let response = match backend
            .chat_stream(&messages, tools, &mut |delta| {
                streamed = true;
                emit(AgentEvent::delta_message(delta));
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

        log_event(
            "model_response",
            json!({
                "content": response.content.as_deref().map(truncate_summary),
                "tool_calls": response.tool_calls.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
            }),
        );

        if response.tool_calls.is_empty() {
            // 非流式回退路径（未收到任何增量）才补发完整消息，避免与 delta 重复
            let answer = response.content.unwrap_or_default();
            if !streamed && !answer.is_empty() {
                emit(AgentEvent::message(answer));
            }
            log_event("done", json!({ "reason": "completed" }));
            emit(AgentEvent::simple(AgentEventKind::Done));
            return;
        }

        // assistant 消息（含 tool_calls）入历史，再逐个执行工具
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

            // 出错/权限被拒不终止会话：把错误文本作为 tool 结果回填给 LLM
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

/// 日志摘要截断（500 字符）
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

    // 会话日志：顺手清理 30 天前的旧日志；日志创建失败不阻断会话
    let sessions_dir = sessions_dir(&app)?;
    if let Err(e) = cleanup_old_sessions(&sessions_dir, SESSION_RETENTION_DAYS) {
        tracing::warn!("cleanup old session logs failed: {}", e);
    }
    let mut session_log = match SessionLog::create(&sessions_dir) {
        Ok(log) => Some(log),
        Err(e) => {
            tracing::warn!("create session log failed: {}", e);
            None
        }
    };

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let engine = app_handle.state::<PermissionEngine>();
        let plugins = app_handle.state::<PluginState>();
        let tool_state = app_handle.state::<PluginToolState>();
        // 工具规格 = 内置 + 插件贡献（contributes.tools）
        let mut tools = ToolRegistry::specs();
        tools.extend(collect_specs(&plugins));
        let executor = PluginToolExecutor {
            app: &app_handle,
            engine: &engine,
            plugins: &plugins,
            tool_state: &tool_state,
        };
        let emit = |event: AgentEvent| {
            let _ = app_handle.emit("agent-event", &event);
        };
        if let Some(log) = session_log.as_mut() {
            let mut log_cb = |kind: &str, payload: &Value| {
                let _ = log.log(kind, payload);
            };
            run_agent_loop(&backend, &executor, &query, &tools, emit, &cancel, Some(&mut log_cb)).await;
        } else {
            run_agent_loop(&backend, &executor, &query, &tools, emit, &cancel, None).await;
        }
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
        run_agent_loop(&backend, &executor, "剪贴板里有什么", &ToolRegistry::specs(), |e| events.push(e), &cancel, None).await;

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
        run_agent_loop(&backend, &executor, "读一下密码文件", &ToolRegistry::specs(), |e| events.push(e), &cancel, None).await;

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
        run_agent_loop(&backend, &executor, "循环", &ToolRegistry::specs(), |e| events.push(e), &cancel, None).await;

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
        run_agent_loop(&backend, &executor, "q", &ToolRegistry::specs(), |e| events.push(e), &cancel, None).await;

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
        run_agent_loop(&backend, &executor, "q", &ToolRegistry::specs(), |e| events.push(e), &cancel, None).await;

        assert_eq!(
            kinds(&events),
            vec![AgentEventKind::Error, AgentEventKind::Done]
        );
    }

    /// 流式后端：chat_stream 分三次发 delta
    struct StreamBackend {
        deltas: Vec<String>,
    }

    impl ChatBackend for StreamBackend {
        fn chat<'a>(
            &'a self,
            _messages: &'a [Message],
            _tools: &'a [ToolSpec],
        ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
            Box::pin(async move {
                Ok(ChatResponse {
                    content: Some(self.deltas.concat()),
                    tool_calls: vec![],
                })
            })
        }

        fn chat_stream<'a>(
            &'a self,
            messages: &'a [Message],
            tools: &'a [ToolSpec],
            on_delta: &'a mut (dyn FnMut(String) + Send),
        ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
            Box::pin(async move {
                for delta in &self.deltas {
                    on_delta(delta.clone());
                }
                self.chat(messages, tools).await
            })
        }
    }

    /// 流式路径：3 个 delta 事件（delta=true），不再补发完整 message；日志回调被调用
    #[tokio::test]
    async fn test_loop_streaming_deltas_and_log() {
        let backend = StreamBackend {
            deltas: vec!["你".to_string(), "好".to_string(), "！".to_string()],
        };
        let executor = MockExecutor::ok(json!("x"));
        let cancel = AtomicBool::new(false);

        let mut events = Vec::new();
        let mut log_entries: Vec<(String, Value)> = Vec::new();
        {
            let mut log_cb = |kind: &str, payload: &Value| {
                log_entries.push((kind.to_string(), payload.clone()));
            };
            run_agent_loop(
                &backend,
                &executor,
                "打个招呼",
                &ToolRegistry::specs(),
                |e| events.push(e),
                &cancel,
                Some(&mut log_cb),
            )
            .await;
        }

        // 3 个 delta message + done，没有重复的完整 message
        assert_eq!(
            kinds(&events),
            vec![
                AgentEventKind::Message,
                AgentEventKind::Message,
                AgentEventKind::Message,
                AgentEventKind::Done,
            ]
        );
        for (i, expected) in ["你", "好", "！"].iter().enumerate() {
            assert_eq!(events[i].content.as_deref(), Some(*expected));
            assert_eq!(events[i].delta, Some(true));
        }

        // 日志覆盖 user_input / model_response / done
        let log_kinds: Vec<&str> = log_entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(log_kinds, vec!["user_input", "model_response", "done"]);
        assert_eq!(log_entries[0].1["query"], "打个招呼");
        assert_eq!(log_entries[1].1["content"], "你好！");
        assert_eq!(log_entries[1].1["tool_calls"], json!([]));
        assert_eq!(log_entries[2].1["reason"], "completed");
    }

    /// 带工具调用的流式会话：tool_call/tool_result 也落日志，结果摘要截断
    #[tokio::test]
    async fn test_loop_logs_tool_call_and_truncates_result() {
        let backend = MockBackend::new(vec![
            tool_call("call_1", "clipboard_read", json!({})),
            final_answer("读完了"),
        ]);
        let long_result = "x".repeat(600);
        let executor = MockExecutor::ok(json!(long_result));
        let cancel = AtomicBool::new(false);

        let mut events = Vec::new();
        let mut log_entries: Vec<(String, Value)> = Vec::new();
        {
            let mut log_cb = |kind: &str, payload: &Value| {
                log_entries.push((kind.to_string(), payload.clone()));
            };
            run_agent_loop(
                &backend,
                &executor,
                "q",
                &ToolRegistry::specs(),
                |e| events.push(e),
                &cancel,
                Some(&mut log_cb),
            )
            .await;
        }

        let log_kinds: Vec<&str> = log_entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            log_kinds,
            vec![
                "user_input",
                "model_response",
                "tool_call",
                "tool_result",
                "model_response",
                "done",
            ]
        );

        let tool_call_log = &log_entries[2].1;
        assert_eq!(tool_call_log["name"], "clipboard_read");

        // 结果摘要截断到 500 字符 + 省略号
        let result_log = log_entries[3].1["result"].as_str().unwrap();
        assert!(result_log.ends_with('…'));
        assert_eq!(result_log.chars().count(), 501);
        assert_eq!(log_entries[3].1["name"], "clipboard_read");

        // 完整结果不受影响地回填给 LLM
        let seen = backend.seen.lock().unwrap();
        assert_eq!(seen[1][3].content.as_deref(), Some(long_result.as_str()));
    }
}
