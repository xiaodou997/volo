use super::*;
use crate::ai::llm::{ChatBackend, ChatResponse, Message, ToolCall};
use crate::ai::tools::{ToolRegistry, ToolSpec};
use crate::error::{Result, VoloError};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct MockBackend {
    responses: Mutex<Vec<ChatResponse>>,
    seen: Mutex<Vec<Vec<Message>>>,
}

impl MockBackend {
    fn new(responses: Vec<ChatResponse>) -> Self {
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
                Ok(value) => Ok(value.clone()),
                Err(error) => Err(VoloError::Other(error.to_string())),
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
    events.iter().map(|event| event.kind).collect()
}

fn start_messages(query: &str) -> Vec<Message> {
    vec![Message::system(SYSTEM_PROMPT), Message::user(query)]
}

#[tokio::test]
async fn test_loop_tool_call_then_answer() {
    let backend = MockBackend::new(vec![
        tool_call("call_1", "clipboard_read", json!({})),
        final_answer("剪贴板里是：你好"),
    ]);
    let executor = MockExecutor::ok(json!("你好"));
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("剪贴板里有什么");
    run_agent_loop(
        &backend,
        &executor,
        &mut messages,
        &ToolRegistry::specs(),
        |event| events.push(event),
        &cancel,
        None,
    )
    .await;

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

    let seen = backend.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].len(), 4);
    assert_eq!(seen[1][2].role, "assistant");
    assert_eq!(seen[1][3].role, "tool");
    assert_eq!(seen[1][3].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(seen[1][3].content.as_deref(), Some("你好"));

    let calls = executor.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "clipboard_read");

    assert_eq!(messages.len(), 5);
    assert_eq!(messages[4].role, "assistant");
    assert_eq!(messages[4].content.as_deref(), Some("剪贴板里是：你好"));
}

#[tokio::test]
async fn test_loop_permission_denied_backfills_error() {
    let backend = MockBackend::new(vec![
        tool_call("call_1", "fs_read", json!({"path": "/etc/passwd"})),
        final_answer("你没有批准读取该文件"),
    ]);
    let executor = MockExecutor::err("Permission 'fs.read' denied by user");
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("读一下密码文件");
    run_agent_loop(
        &backend,
        &executor,
        &mut messages,
        &ToolRegistry::specs(),
        |event| events.push(event),
        &cancel,
        None,
    )
    .await;

    assert_eq!(
        kinds(&events),
        vec![
            AgentEventKind::ToolCall,
            AgentEventKind::ToolResult,
            AgentEventKind::Message,
            AgentEventKind::Done,
        ]
    );
    let result = events[1].result.as_deref().unwrap();
    assert!(result.starts_with("Error:"));
    assert!(result.contains("denied"));
    let seen = backend.seen.lock().unwrap();
    assert!(seen[1][3].content.as_deref().unwrap().contains("denied"));
}

#[tokio::test]
async fn test_loop_max_rounds_terminates() {
    let backend = MockBackend::new(
        (0..MAX_ROUNDS)
            .map(|index| tool_call(&format!("call_{}", index), "clipboard_read", json!({})))
            .collect(),
    );
    let executor = MockExecutor::ok(json!("x"));
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("循环");
    run_agent_loop(
        &backend,
        &executor,
        &mut messages,
        &ToolRegistry::specs(),
        |event| events.push(event),
        &cancel,
        None,
    )
    .await;

    assert_eq!(backend.seen.lock().unwrap().len(), MAX_ROUNDS);
    let event_kinds = kinds(&events);
    assert!(event_kinds.contains(&AgentEventKind::Error));
    assert_eq!(event_kinds.last(), Some(&AgentEventKind::Done));
    assert!(!event_kinds.contains(&AgentEventKind::Message));
    let error = events
        .iter()
        .find(|event| event.kind == AgentEventKind::Error)
        .unwrap();
    assert!(error.content.as_deref().unwrap().contains("最大轮数"));
}

#[tokio::test]
async fn test_loop_cancel_before_first_round() {
    let backend = MockBackend::new(vec![final_answer("不应到达")]);
    let executor = MockExecutor::ok(json!("x"));
    let cancel = AtomicBool::new(true);

    let mut events = Vec::new();
    let mut messages = start_messages("q");
    run_agent_loop(
        &backend,
        &executor,
        &mut messages,
        &ToolRegistry::specs(),
        |event| events.push(event),
        &cancel,
        None,
    )
    .await;

    assert_eq!(kinds(&events), vec![AgentEventKind::Done]);
    assert!(backend.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_loop_backend_error() {
    let backend = MockBackend::new(vec![]);
    let executor = MockExecutor::ok(json!("x"));
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("q");
    run_agent_loop(
        &backend,
        &executor,
        &mut messages,
        &ToolRegistry::specs(),
        |event| events.push(event),
        &cancel,
        None,
    )
    .await;

    assert_eq!(
        kinds(&events),
        vec![AgentEventKind::Error, AgentEventKind::Done]
    );
}

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
        on_delta: &'a mut (dyn FnMut(String) -> bool + Send),
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
        Box::pin(async move {
            for delta in &self.deltas {
                if !on_delta(delta.clone()) {
                    break;
                }
            }
            self.chat(messages, tools).await
        })
    }
}

#[tokio::test]
async fn test_loop_streaming_deltas_and_log() {
    let backend = StreamBackend {
        deltas: vec!["你".to_string(), "好".to_string(), "！".to_string()],
    };
    let executor = MockExecutor::ok(json!("x"));
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("打个招呼");
    let mut log_entries: Vec<(String, Value)> = Vec::new();
    {
        let mut log_cb = |kind: &str, payload: &Value| {
            log_entries.push((kind.to_string(), payload.clone()));
        };
        run_agent_loop(
            &backend,
            &executor,
            &mut messages,
            &ToolRegistry::specs(),
            |event| events.push(event),
            &cancel,
            Some(&mut log_cb),
        )
        .await;
    }

    assert_eq!(
        kinds(&events),
        vec![
            AgentEventKind::Message,
            AgentEventKind::Message,
            AgentEventKind::Message,
            AgentEventKind::Done,
        ]
    );
    for (index, expected) in ["你", "好", "！"].iter().enumerate() {
        assert_eq!(events[index].content.as_deref(), Some(*expected));
        assert_eq!(events[index].delta, Some(true));
    }
    let log_kinds: Vec<&str> = log_entries.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(log_kinds, vec!["model_response", "done"]);
    assert_eq!(log_entries[0].1["content"], "你好！");
    assert_eq!(log_entries[0].1["tool_calls"], json!([]));
    assert_eq!(log_entries[1].1["reason"], "completed");
}

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
    let mut messages = start_messages("q");
    let mut log_entries: Vec<(String, Value)> = Vec::new();
    {
        let mut log_cb = |kind: &str, payload: &Value| {
            log_entries.push((kind.to_string(), payload.clone()));
        };
        run_agent_loop(
            &backend,
            &executor,
            &mut messages,
            &ToolRegistry::specs(),
            |event| events.push(event),
            &cancel,
            Some(&mut log_cb),
        )
        .await;
    }

    let log_kinds: Vec<&str> = log_entries.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(
        log_kinds,
        vec![
            "model_response",
            "tool_call",
            "tool_result",
            "model_response",
            "done",
        ]
    );
    assert_eq!(log_entries[1].1["name"], "clipboard_read");
    let result_log = log_entries[2].1["result"].as_str().unwrap();
    assert!(result_log.ends_with('…'));
    assert_eq!(result_log.chars().count(), 501);
    assert_eq!(log_entries[2].1["name"], "clipboard_read");
    let seen = backend.seen.lock().unwrap();
    assert_eq!(seen[1][3].content.as_deref(), Some(long_result.as_str()));
}

#[tokio::test]
async fn test_history_carries_across_turns() {
    let backend = MockBackend::new(vec![
        tool_call("call_1", "clipboard_read", json!({})),
        final_answer("剪贴板里是：你好"),
        final_answer("第二轮回答"),
    ]);
    let executor = MockExecutor::ok(json!("你好"));
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("剪贴板里有什么");
    run_agent_loop(
        &backend,
        &executor,
        &mut messages,
        &ToolRegistry::specs(),
        |event| events.push(event),
        &cancel,
        None,
    )
    .await;
    assert_eq!(messages.len(), 5);

    messages.push(Message::user("再说一遍"));
    run_agent_loop(
        &backend,
        &executor,
        &mut messages,
        &ToolRegistry::specs(),
        |event| events.push(event),
        &cancel,
        None,
    )
    .await;

    let seen = backend.seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    let second_turn = &seen[2];
    assert_eq!(second_turn.len(), 6);
    assert_eq!(second_turn[0].role, "system");
    assert_eq!(second_turn[1].content.as_deref(), Some("剪贴板里有什么"));
    assert_eq!(second_turn[2].role, "assistant");
    assert_eq!(second_turn[3].role, "tool");
    assert_eq!(second_turn[3].content.as_deref(), Some("你好"));
    assert_eq!(second_turn[4].content.as_deref(), Some("剪贴板里是：你好"));
    assert_eq!(second_turn[5].content.as_deref(), Some("再说一遍"));
}

#[tokio::test]
async fn test_loop_cancel_mid_stream() {
    let backend = StreamBackend {
        deltas: vec!["你".to_string(), "好".to_string(), "！".to_string()],
    };
    let executor = MockExecutor::ok(json!("x"));
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("打个招呼");
    let mut log_entries: Vec<(String, Value)> = Vec::new();
    {
        let mut log_cb = |kind: &str, payload: &Value| {
            log_entries.push((kind.to_string(), payload.clone()));
        };
        run_agent_loop(
            &backend,
            &executor,
            &mut messages,
            &ToolRegistry::specs(),
            |event| {
                if event.delta == Some(true) {
                    cancel.store(true, Ordering::Relaxed);
                }
                events.push(event);
            },
            &cancel,
            Some(&mut log_cb),
        )
        .await;
    }

    assert_eq!(
        kinds(&events),
        vec![AgentEventKind::Message, AgentEventKind::Done]
    );
    assert_eq!(events[0].content.as_deref(), Some("你"));
    assert_eq!(events[0].delta, Some(true));
    let last = log_entries.last().unwrap();
    assert_eq!(last.0, "done");
    assert_eq!(last.1["reason"], "cancelled");
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_loop_logs_full_model_response() {
    let long_answer = "答".repeat(600);
    let backend = MockBackend::new(vec![final_answer(&long_answer)]);
    let executor = MockExecutor::ok(json!("x"));
    let cancel = AtomicBool::new(false);

    let mut events = Vec::new();
    let mut messages = start_messages("q");
    let mut log_entries: Vec<(String, Value)> = Vec::new();
    {
        let mut log_cb = |kind: &str, payload: &Value| {
            log_entries.push((kind.to_string(), payload.clone()));
        };
        run_agent_loop(
            &backend,
            &executor,
            &mut messages,
            &ToolRegistry::specs(),
            |event| events.push(event),
            &cancel,
            Some(&mut log_cb),
        )
        .await;
    }

    assert_eq!(log_entries[0].0, "model_response");
    assert_eq!(
        log_entries[0].1["content"].as_str(),
        Some(long_answer.as_str())
    );
}

#[test]
fn test_begin_turn_busy_guard() {
    let manager = AgentManager::new();
    let messages = manager
        .begin_turn("第一个问题", SYSTEM_PROMPT, Vec::new())
        .unwrap();
    assert!(manager.is_busy());
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "system");
    assert_eq!(messages[1].role, "user");

    let error = manager
        .begin_turn("并发问题", SYSTEM_PROMPT, Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("还在进行中"));

    manager.finish_turn(messages);
    assert!(!manager.is_busy());
    let messages = manager
        .begin_turn("追问", SYSTEM_PROMPT, Vec::new())
        .unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[2].content.as_deref(), Some("追问"));
}

#[test]
fn test_new_session_clears_history() {
    let manager = AgentManager::new();
    let messages = manager
        .begin_turn("旧会话", SYSTEM_PROMPT, Vec::new())
        .unwrap();
    manager.finish_turn(messages);
    manager.cancel_flag().store(true, Ordering::Relaxed);

    manager.new_session();
    assert!(!manager.cancel_flag().load(Ordering::Relaxed));
    let messages = manager
        .begin_turn("新会话", SYSTEM_PROMPT, Vec::new())
        .unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "system");
    manager.finish_turn(messages);
}

#[test]
fn test_build_system_prompt() {
    assert_eq!(build_system_prompt(&[]), SYSTEM_PROMPT);

    let skills = vec![
        crate::ai::skill::SkillMeta {
            name: "weekly-report".to_string(),
            description: "生成结构化周报".to_string(),
            version: "1.0.0".to_string(),
        },
        crate::ai::skill::SkillMeta {
            name: "bare".to_string(),
            description: String::new(),
            version: String::new(),
        },
    ];
    let prompt = build_system_prompt(&skills);
    assert!(prompt.starts_with(SYSTEM_PROMPT));
    assert!(prompt.contains("weekly-report（生成结构化周报）"));
    assert!(prompt.contains("、"));
    assert!(prompt.contains("skill_load"));
    assert!(prompt.contains("bare"));
    assert!(!prompt.contains("bare（）"));
}

#[test]
fn test_append_explicit_skill() {
    let prompt = append_explicit_skill(
        SYSTEM_PROMPT.to_string(),
        "weekly-report",
        "按此结构输出周报",
    );
    assert!(prompt.starts_with(SYSTEM_PROMPT));
    assert!(prompt.contains("weekly-report"));
    assert!(prompt.contains("按此结构输出周报"));
}

#[test]
fn test_load_history_busy_guard_and_continue() {
    let manager = AgentManager::new();

    let inflight = manager
        .begin_turn("进行中", SYSTEM_PROMPT, Vec::new())
        .unwrap();
    let restored = vec![
        Message::system(SYSTEM_PROMPT),
        Message::user("旧问题"),
        Message::assistant(Some("旧回答".to_string()), vec![]),
    ];
    assert!(manager.load_history(restored.clone()).is_err());
    manager.finish_turn(inflight);

    manager.cancel_flag().store(true, Ordering::Relaxed);
    manager.load_history(restored).unwrap();
    assert!(!manager.cancel_flag().load(Ordering::Relaxed));

    let messages = manager
        .begin_turn("追问", SYSTEM_PROMPT, Vec::new())
        .unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].role, "system");
    assert_eq!(messages[2].content.as_deref(), Some("旧回答"));
    assert_eq!(messages[3].content.as_deref(), Some("追问"));
    manager.finish_turn(messages);
}
