//! 插件工具桥
//! Agent（Rust 侧）调用插件声明的 contributes.tools：
//! emit `plugin-tool-call` 事件 → 前端沙箱执行插件 JS → `plugin_tool_result` 命令回传 →
//! oneshot 唤醒等待中的工具调用。模式与 PermissionEngine 的审批往返一致。
//!
//! 命名空间约定：
//! - 内置工具保持自身名字；
//! - MCP 工具使用 `mcp__...`；
//! - 插件工具统一使用 `plugin__...`，并带原始 plugin/tool id 的稳定哈希。
//! 这样插件不会再与 MCP / builtin 命名空间冲突，sanitize 后相同的 id 也不会互相覆盖。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;
use tracing::warn;

use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

use super::agent::ToolExecutor;
use super::mcp::{McpRegistry, MCP_NAME_PREFIX};
use super::tools::{ToolRegistry, ToolSpec};

/// 前端执行插件工具的超时时间
pub const PLUGIN_TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// 插件工具独立 LLM 命名空间。
pub const PLUGIN_NAME_PREFIX: &str = "plugin__";

// OpenAI function/tool name 通常限制在 64 字符内。固定布局：
// plugin__(8) + plugin(14) + __(2) + tool(22) + __(2) + hash(16) = 64。
const PLUGIN_SEGMENT_MAX: usize = 14;
const TOOL_SEGMENT_MAX: usize = 22;

/// `plugin-tool-call` 事件 payload（camelCase）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolCall {
    pub request_id: String,
    pub plugin_id: String,
    pub tool_id: String,
    pub args: Value,
}

/// 插件工具调用的挂起状态（Tauri managed state）
pub struct PluginToolState {
    /// request_id -> 等待前端回传结果的 channel
    pending: Mutex<HashMap<String, oneshot::Sender<std::result::Result<Value, String>>>>,
}

impl PluginToolState {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// 生成 request_id 并挂起等待通道（与发事件解耦，便于测试）
    pub fn begin_call(&self) -> (String, oneshot::Receiver<std::result::Result<Value, String>>) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(request_id.clone(), tx);
        }
        (request_id, rx)
    }

    /// 前端回传结果（plugin_tool_result 命令调用）；未知 request_id 静默忽略
    pub fn respond(
        &self,
        request_id: &str,
        ok: bool,
        result: Option<Value>,
        error: Option<String>,
    ) {
        let tx = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(request_id));

        let Some(tx) = tx else {
            warn!("plugin_tool_result for unknown request_id: {}", request_id);
            return;
        };

        let outcome = if ok {
            Ok(result.unwrap_or(Value::Null))
        } else {
            Err(error.unwrap_or_else(|| "插件工具执行失败（未提供错误信息）".to_string()))
        };
        // 接收端可能已超时关闭，发送失败忽略
        let _ = tx.send(outcome);
    }

    /// 清理挂起的调用（超时后清掉 stale sender，迟到的 respond 直接丢弃）
    pub fn cancel_call(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(request_id);
        }
    }

    /// 挂起中的请求数（测试用）
    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending.lock().map(|p| p.len()).unwrap_or(0)
    }
}

impl Default for PluginToolState {
    fn default() -> Self {
        Self::new()
    }
}

/// LLM 工具名清洗：非 [a-zA-Z0-9_-] 替换为 `_`（OpenAI function name 约束）
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn truncate_segment(value: &str, max: usize) -> String {
    sanitize(value).chars().take(max).collect()
}

/// FNV-1a 64-bit：不依赖随机 seed，保证同一 plugin/tool id 在各平台生成一致名字。
fn stable_tool_hash(plugin_id: &str, tool_id: &str) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let mut hash = OFFSET;
    for byte in plugin_id
        .as_bytes()
        .iter()
        .copied()
        .chain(std::iter::once(0))
        .chain(tool_id.as_bytes().iter().copied())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// 插件工具的 LLM 侧名字：
/// `plugin__{short_plugin}__{short_tool}__{stable_hash}`。
///
/// 可读片段用于调试，最终 hash 使用原始 id，负责消除 sanitize / 截断碰撞。
pub fn to_llm_name(plugin_id: &str, tool_id: &str) -> String {
    format!(
        "{}{}__{}__{:016x}",
        PLUGIN_NAME_PREFIX,
        truncate_segment(plugin_id, PLUGIN_SEGMENT_MAX),
        truncate_segment(tool_id, TOOL_SEGMENT_MAX),
        stable_tool_hash(plugin_id, tool_id)
    )
}

/// 聚合所有插件 contributes.tools 的 LLM 规格：
/// name 命名空间化、description 用工具 description（缺省回退 name）、parameters 直接透传
pub fn collect_specs(plugins: &PluginState) -> Vec<ToolSpec> {
    let Ok(plugins) = plugins.plugins.lock() else {
        warn!("collect_specs: plugin state lock poisoned");
        return Vec::new();
    };

    let mut specs = Vec::new();
    for plugin in plugins.values() {
        for tool in &plugin.contributes.tools {
            specs.push(ToolSpec {
                name: to_llm_name(&plugin.id, &tool.id),
                description: tool
                    .description
                    .clone()
                    .unwrap_or_else(|| tool.name.clone()),
                parameters: tool.parameters.clone(),
            });
        }
    }
    specs
}

/// 用完整 LLM 名反查 manifest 里的原始 plugin_id / tool_id。
/// 不再依赖 sanitize 后的 id 反解，因此 sanitize 碰撞不会导致“命中第一个插件”。
pub(crate) fn lookup_tool(plugins: &PluginState, llm_name: &str) -> Option<(String, String)> {
    if !llm_name.starts_with(PLUGIN_NAME_PREFIX) {
        return None;
    }

    let plugins = plugins.plugins.lock().ok()?;
    for plugin in plugins.values() {
        for tool in &plugin.contributes.tools {
            if to_llm_name(&plugin.id, &tool.id) == llm_name {
                return Some((plugin.id.clone(), tool.id.clone()));
            }
        }
    }
    None
}

/// 聚合执行器，dispatch 顺序：
/// `mcp__` → MCP；`plugin__` → 插件工具桥；其余 → 内置 ToolRegistry。
///
/// `principal` 由调用者提供：Agent 使用 `agent:builtin`，Workflow 使用
/// `workflow:<workflow-id>`，从而隔离 PermissionEngine 的 Session/Always grants。
pub struct AgentToolExecutor<'a> {
    pub app: &'a AppHandle,
    pub engine: &'a PermissionEngine,
    pub plugins: &'a PluginState,
    pub tool_state: &'a PluginToolState,
    pub mcp: &'a McpRegistry,
    pub principal: &'a str,
}

impl ToolExecutor for AgentToolExecutor<'_> {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            if name.starts_with(MCP_NAME_PREFIX) {
                // MCP 也必须经过统一权限管道。把具体工具名编码进 capability，
                // 这样 Session/Always 授权只覆盖当前 MCP tool，而不是一次放行所有 MCP。
                let capability = format!("mcp.call:{}", name);
                self.engine
                    .enforce(self.app, self.principal, &capability, Some(name))
                    .await?;
                return self.mcp.call(name, args).await;
            }

            if name.starts_with(PLUGIN_NAME_PREFIX) {
                return self.execute_plugin_tool(name, args).await;
            }

            ToolRegistry::execute_as(self.app, self.engine, self.principal, name, &args).await
        })
    }
}

impl AgentToolExecutor<'_> {
    /// 插件工具路径：按完整 namespace 名确认工具存在 → 挂 oneshot → emit 事件 → 超时等待
    ///
    /// 出错/超时统一返回 Err，agent 循环会把错误文本作为 tool 结果回喂 LLM
    async fn execute_plugin_tool(&self, llm_name: &str, args: Value) -> Result<Value> {
        let (orig_plugin_id, orig_tool_id) = lookup_tool(self.plugins, llm_name)
            .ok_or_else(|| VoloError::NotFound(format!("plugin tool: {}", llm_name)))?;

        let (request_id, rx) = self.tool_state.begin_call();
        let payload = PluginToolCall {
            request_id: request_id.clone(),
            plugin_id: orig_plugin_id.clone(),
            tool_id: orig_tool_id.clone(),
            args,
        };

        // 发不出去（如无窗口）直接视为失败
        if let Err(e) = self.app.emit("plugin-tool-call", &payload) {
            self.tool_state.cancel_call(&request_id);
            return Err(VoloError::Other(format!(
                "插件工具调用事件发送失败: {}",
                e
            )));
        }

        match tokio::time::timeout(PLUGIN_TOOL_TIMEOUT, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(err))) => Err(VoloError::Other(format!(
                "插件工具 {}/{} 执行失败: {}",
                orig_plugin_id, orig_tool_id, err
            ))),
            Ok(Err(_)) => Err(VoloError::Other(format!(
                "插件工具 {}/{} 调用通道已关闭",
                orig_plugin_id, orig_tool_id
            ))),
            Err(_) => {
                self.tool_state.cancel_call(&request_id);
                Err(VoloError::Other(format!(
                    "插件工具 {}/{} 执行超时（{} 秒）",
                    orig_plugin_id,
                    orig_tool_id,
                    PLUGIN_TOOL_TIMEOUT.as_secs()
                )))
            }
        }
    }
}

// ============ Tauri Commands ============

/// 前端执行完插件工具后回传结果，唤醒挂起的工具调用
#[tauri::command]
pub fn plugin_tool_result(
    state: State<'_, PluginToolState>,
    request_id: String,
    ok: bool,
    result: Option<Value>,
    error: Option<String>,
) {
    state.respond(&request_id, ok, result, error);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manager::{Contributes, Plugin, ToolManifestSpec};
    use serde_json::json;
    use std::path::PathBuf;

    fn make_tool(id: &str, name: &str, description: Option<&str>) -> ToolManifestSpec {
        ToolManifestSpec {
            id: id.to_string(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            parameters: json!({ "type": "object", "properties": {} }),
            run: "tool.js".to_string(),
            icon: None,
        }
    }

    fn make_plugin(id: &str, tools: Vec<ToolManifestSpec>) -> Plugin {
        Plugin {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            main: "index.html".to_string(),
            path: PathBuf::new(),
            features: vec![],
            permissions: vec![],
            description: None,
            icon: None,
            contributes: Contributes {
                commands: vec![],
                tools,
            },
        }
    }

    fn make_plugin_state(plugins: Vec<Plugin>) -> PluginState {
        PluginState::for_test(plugins)
    }

    // ---- 命名空间 / 清洗 / 碰撞 ----

    #[test]
    fn test_sanitize() {
        assert_eq!(sanitize("uuid-gen"), "uuid-gen");
        assert_eq!(sanitize("gen_uuid"), "gen_uuid");
        assert_eq!(sanitize("my.plugin"), "my_plugin");
        assert_eq!(sanitize("生成 UUID"), "___UUID");
        assert_eq!(sanitize("a b/c.d"), "a_b_c_d");
    }

    #[test]
    fn test_llm_name_has_isolated_namespace_and_length_limit() {
        let name = to_llm_name("uuid-gen", "gen_uuid");
        assert!(name.starts_with(PLUGIN_NAME_PREFIX));
        assert!(name.len() <= 64);
        assert_eq!(name, to_llm_name("uuid-gen", "gen_uuid"));
    }

    #[test]
    fn test_sanitize_collisions_are_disambiguated_by_hash() {
        // 可读段相同，但原始 id 不同，最终 hash 必须不同。
        assert_eq!(sanitize("a.b"), sanitize("a_b"));
        assert_ne!(
            to_llm_name("a.b", "tool"),
            to_llm_name("a_b", "tool")
        );

        assert_eq!(sanitize("a.b"), sanitize("a b"));
        assert_ne!(
            to_llm_name("plugin", "a.b"),
            to_llm_name("plugin", "a b")
        );
    }

    #[test]
    fn test_plugin_mcp_id_cannot_collide_with_mcp_namespace() {
        let plugin_name = to_llm_name("mcp", "server__tool");
        assert!(plugin_name.starts_with("plugin__"));
        assert!(!plugin_name.starts_with(MCP_NAME_PREFIX));
        assert_ne!(plugin_name, "mcp__server__tool");
    }

    // ---- 规格聚合 ----

    #[test]
    fn test_collect_specs_aggregates_tools() {
        let state = make_plugin_state(vec![
            make_plugin(
                "uuid-gen",
                vec![
                    make_tool("gen_uuid", "生成 UUID", Some("生成指定数量的 UUID v4")),
                    make_tool("no_desc", "无描述工具", None),
                ],
            ),
            make_plugin("empty-plugin", vec![]),
        ]);

        let specs = collect_specs(&state);
        assert_eq!(specs.len(), 2);

        let gen = specs
            .iter()
            .find(|spec| spec.name == to_llm_name("uuid-gen", "gen_uuid"))
            .unwrap();
        assert_eq!(gen.description, "生成指定数量的 UUID v4");
        assert_eq!(gen.parameters["type"], "object");

        let no_desc = specs
            .iter()
            .find(|spec| spec.name == to_llm_name("uuid-gen", "no_desc"))
            .unwrap();
        assert_eq!(no_desc.description, "无描述工具");
    }

    #[test]
    fn test_lookup_tool_uses_full_hashed_name() {
        let state = make_plugin_state(vec![
            make_plugin(
                "my.plugin",
                vec![make_tool("gen-uuid", "生成 UUID", None)],
            ),
            make_plugin(
                "my_plugin",
                vec![make_tool("gen-uuid", "另一工具", None)],
            ),
        ]);

        let dotted = to_llm_name("my.plugin", "gen-uuid");
        let underscored = to_llm_name("my_plugin", "gen-uuid");
        assert_ne!(dotted, underscored);
        assert_eq!(
            lookup_tool(&state, &dotted),
            Some(("my.plugin".to_string(), "gen-uuid".to_string()))
        );
        assert_eq!(
            lookup_tool(&state, &underscored),
            Some(("my_plugin".to_string(), "gen-uuid".to_string()))
        );
        assert!(lookup_tool(&state, "plugin__nope").is_none());
        assert!(lookup_tool(&state, "mcp__server__tool").is_none());
    }

    // ---- pending 唤醒 ----

    #[tokio::test]
    async fn test_respond_wakes_pending_ok() {
        let state = PluginToolState::new();
        let (request_id, rx) = state.begin_call();
        assert_eq!(state.pending_count(), 1);

        state.respond(&request_id, true, Some(json!({"uuids": ["a", "b"]})), None);
        assert_eq!(state.pending_count(), 0);

        let outcome = rx.await.unwrap();
        assert_eq!(outcome.unwrap(), json!({"uuids": ["a", "b"]}));
    }

    #[tokio::test]
    async fn test_respond_wakes_pending_err() {
        let state = PluginToolState::new();
        let (request_id, rx) = state.begin_call();

        state.respond(&request_id, false, None, Some("boom".to_string()));
        let outcome = rx.await.unwrap();
        assert_eq!(outcome.unwrap_err(), "boom");
    }

    #[tokio::test]
    async fn test_respond_ok_without_result_is_null() {
        let state = PluginToolState::new();
        let (request_id, rx) = state.begin_call();

        state.respond(&request_id, true, None, None);
        assert_eq!(rx.await.unwrap().unwrap(), Value::Null);
    }

    #[test]
    fn test_respond_unknown_request_id_ignored() {
        let state = PluginToolState::new();
        // 不 panic、不报错
        state.respond("no-such-request", true, Some(json!(1)), None);
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn test_cancel_call_cleans_pending() {
        let state = PluginToolState::new();
        let (request_id, _rx) = state.begin_call();
        assert_eq!(state.pending_count(), 1);

        state.cancel_call(&request_id);
        assert_eq!(state.pending_count(), 0);

        // 超时后的迟到 respond 静默丢弃
        state.respond(&request_id, true, Some(json!(1)), None);
        assert_eq!(state.pending_count(), 0);
    }
}
