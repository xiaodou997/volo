//! Agent 会话入口。
//!
//! 本模块负责 system prompt、会话状态与 Tauri commands；纯模型/工具循环位于 `runtime`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use super::llm::{Message, OpenAiBackend};
use super::mcp::McpRegistry;
use super::plugin_tools::{collect_specs, AgentToolExecutor, PluginToolState};
use super::session::{cleanup_old_sessions, sessions_dir, SessionLog, SESSION_RETENTION_DAYS};
use super::tools::{ToolRegistry, AGENT_PRINCIPAL};
use crate::core::config::Config;
use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

mod runtime;
pub use super::tool_executor::ToolExecutor;
pub use runtime::{run_agent_loop, AgentEvent, AgentEventKind, MAX_ROUNDS};

#[cfg(test)]
mod tests;

pub(crate) const SYSTEM_PROMPT: &str = "你是 Volo 启动器的内置助手，\
可以调用工具帮用户完成任务：内置工具有 clipboard_read 读取剪贴板、\
fs_read 读取文本文件、notification_show 发送系统通知；\
此外还有插件贡献的工具（名字形如 plugin__tool），以请求中携带的 tools 列表为准。\
原则：用户请求与某个工具能力匹配时，必须调用工具获取真实结果，不要凭记忆编造；\
谨慎行事，先读后写；涉及用户数据的操作说明理由；\
工具返回错误时向用户解释原因并给出替代建议。";

/// 组装 system prompt：基础提示 + 可用技能目录。
/// 渐进披露：目录只列 name + description，正文由模型经 skill_load 按需加载。
pub fn build_system_prompt(skills: &[super::skill::SkillMeta]) -> String {
    if skills.is_empty() {
        return SYSTEM_PROMPT.to_string();
    }

    let catalog = skills
        .iter()
        .map(|skill| {
            if skill.description.is_empty() {
                skill.name.clone()
            } else {
                format!("{}（{}）", skill.name, skill.description)
            }
        })
        .collect::<Vec<_>>()
        .join("、");

    format!(
        "{}\n可用技能：{}。用户意图与某个技能匹配时，先调用 skill_load 加载该技能的完整指令，再严格按指令执行。",
        SYSTEM_PROMPT, catalog
    )
}

/// 显式技能注入（启动器 @技能名 触发）。
fn append_explicit_skill(prompt: String, name: &str, body: &str) -> String {
    format!(
        "{}\n用户已显式指定技能 {}。其完整指令如下，本次会话严格按此执行：\n{}",
        prompt, name, body
    )
}

/// Agent 会话管理器（Tauri managed state）。
pub struct AgentManager {
    cancel: Arc<AtomicBool>,
    /// 多轮对话历史（含首条 system），每轮 agent_ask 结束后回写。
    history: Mutex<Vec<Message>>,
    /// 会话进行中标记：防止并发 agent_ask 打乱共享历史。
    busy: AtomicBool,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            history: Mutex::new(Vec::new()),
            busy: AtomicBool::new(false),
        }
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// 开启新会话：清空历史并复位取消标志。
    pub fn new_session(&self) {
        self.history.lock().unwrap().clear();
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// 尝试开始一轮对话；返回脱离锁的历史副本供异步运行时使用。
    pub fn begin_turn(
        &self,
        query: &str,
        system_prompt: &str,
        images: Vec<String>,
    ) -> Result<Vec<Message>> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(VoloError::Other("上一个会话还在进行中".to_string()));
        }

        self.cancel.store(false, Ordering::Relaxed);
        let mut history = self.history.lock().unwrap();
        if history.is_empty() {
            history.push(Message::system(system_prompt));
        }
        history.push(if images.is_empty() {
            Message::user(query)
        } else {
            Message::user_with_images(query, images)
        });
        Ok(history.clone())
    }

    /// 一轮对话结束（正常/取消/出错）：回写历史并解除 busy。
    pub fn finish_turn(&self, messages: Vec<Message>) {
        *self.history.lock().unwrap() = messages;
        self.busy.store(false, Ordering::SeqCst);
    }

    /// 从回放恢复历史。busy 时拒绝，避免覆盖正在执行的会话。
    pub fn load_history(&self, messages: Vec<Message>) -> Result<()> {
        if self.busy.load(Ordering::SeqCst) {
            return Err(VoloError::Other("上一个会话还在进行中".to_string()));
        }
        self.cancel.store(false, Ordering::Relaxed);
        *self.history.lock().unwrap() = messages;
        Ok(())
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 发起一次 Agent 会话：立即返回，进度经 `agent-event` 事件推送。
#[tauri::command]
pub fn agent_ask(
    app: AppHandle,
    manager: State<'_, AgentManager>,
    config: State<'_, Config>,
    query: String,
    skill: Option<String>,
    images: Option<Vec<String>>,
) -> Result<()> {
    let llm = config.get().llm;
    if llm.model.trim().is_empty() {
        return Err(VoloError::Other(
            "请先在设置中配置 LLM（模型未填写）".to_string(),
        ));
    }
    if llm.api_key.trim().is_empty() {
        return Err(VoloError::Other("请先在设置中配置 LLM API Key".to_string()));
    }

    let backend = OpenAiBackend::new(llm.base_url, llm.model, llm.api_key.clone());
    let cancel = manager.cancel_flag();

    // 会话日志：顺手清理旧日志；日志创建失败不阻断会话。
    let sessions_dir = sessions_dir(&app)?;
    if let Err(error) = cleanup_old_sessions(&sessions_dir, SESSION_RETENTION_DAYS) {
        tracing::warn!("cleanup old session logs failed: {}", error);
    }
    let mut session_log = match SessionLog::create(&sessions_dir) {
        Ok(log) => Some(log),
        Err(error) => {
            tracing::warn!("create session log failed: {}", error);
            None
        }
    };

    // 首轮组装 system prompt；@技能名触发时直接注入完整技能正文。
    let skills_dir = super::skill::skills_dir(&app)?;
    let skills = super::skill::scan_skills(&skills_dir);
    let mut system_prompt = build_system_prompt(&skills);
    if let Some(name) = skill.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        let body = super::skill::load_skill_body(&skills_dir, name)?;
        system_prompt = append_explicit_skill(system_prompt, name, &body);
    }

    let mut messages = manager.begin_turn(
        &query,
        &system_prompt,
        images.clone().unwrap_or_default(),
    )?;
    if let Some(log) = session_log.as_mut() {
        let image_count = images.as_ref().map(|items| items.len()).unwrap_or(0);
        let _ = log.log(
            "user_input",
            &json!({ "query": query, "imageCount": image_count }),
        );
    }

    let app_handle = app.clone();
    let finish_handle = app.clone();
    let mcp_servers = config.get().mcp_servers;
    tauri::async_runtime::spawn(async move {
        let engine = app_handle.state::<PermissionEngine>();
        let plugins = app_handle.state::<PluginState>();
        let tool_state = app_handle.state::<PluginToolState>();
        let mcp = app_handle.state::<McpRegistry>();

        // MCP 连接幂等；失败由 registry 告警并跳过，不阻断其他工具。
        mcp.connect_all(&mcp_servers).await;

        let mut tools = ToolRegistry::specs();
        tools.extend(collect_specs(&plugins));
        tools.extend(mcp.specs());

        let executor = AgentToolExecutor {
            app: &app_handle,
            engine: &engine,
            plugins: &plugins,
            tool_state: &tool_state,
            mcp: &mcp,
            principal: AGENT_PRINCIPAL,
        };
        let emit = |event: AgentEvent| {
            let _ = app_handle.emit("agent-event", &event);
        };

        if let Some(log) = session_log.as_mut() {
            let mut log_cb = |kind: &str, payload: &Value| {
                let _ = log.log(kind, payload);
            };
            run_agent_loop(
                &backend,
                &executor,
                &mut messages,
                &tools,
                emit,
                &cancel,
                Some(&mut log_cb),
            )
            .await;
        } else {
            run_agent_loop(
                &backend,
                &executor,
                &mut messages,
                &tools,
                emit,
                &cancel,
                None,
            )
            .await;
        }

        if let Some(log) = session_log.as_mut() {
            let _ = log.log("history", &json!({ "messages": &messages }));
        }
        finish_handle.state::<AgentManager>().finish_turn(messages);
    });

    Ok(())
}

/// 开启新会话：清空多轮历史并复位取消标志。
#[tauri::command]
pub fn agent_new_session(manager: State<'_, AgentManager>) {
    manager.new_session();
}

/// 从会话日志恢复历史，继续该会话；仅载入历史，不发起请求。
#[tauri::command]
pub fn agent_resume_session(
    app: AppHandle,
    manager: State<'_, AgentManager>,
    session_id: String,
) -> Result<()> {
    let messages = super::session::rebuild_history(&sessions_dir(&app)?, &session_id)?;
    manager.load_history(messages)
}

/// 取消当前 Agent 会话。
#[tauri::command]
pub fn agent_cancel(manager: State<'_, AgentManager>) {
    manager.cancel_flag().store(true, Ordering::Relaxed);
}
