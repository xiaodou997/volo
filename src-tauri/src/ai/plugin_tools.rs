//! 插件工具桥
//! Agent（Rust 侧）调用插件声明的 contributes.tools：
//! emit `plugin-tool-call` 事件 → 前端沙箱执行插件 JS → `plugin_tool_result` 命令回传 →
//! oneshot 唤醒等待中的工具调用。模式与 PermissionEngine 的审批往返一致。

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
use super::tools::{ToolRegistry, ToolSpec};

/// 前端执行插件工具的超时时间
pub const PLUGIN_TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// LLM 工具名分隔符：{sanitized_plugin_id}__{sanitized_tool_id}
const NAME_SEPARATOR: &str = "__";

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

/// 插件工具的 LLM 侧命名空间名：{sanitized_plugin_id}__{sanitized_tool_id}
pub fn to_llm_name(plugin_id: &str, tool_id: &str) -> String {
    format!(
        "{}{}{}",
        sanitize(plugin_id),
        NAME_SEPARATOR,
        sanitize(tool_id)
    )
}

/// 反解 LLM 工具名；不含 `__` 分隔符返回 None（即非插件工具）。
///
/// 清洗后的 plugin_id 可能含单下划线，分隔符取第一个 `__`
pub fn parse_llm_name(name: &str) -> Option<(String, String)> {
    let (plugin_id, tool_id) = name.split_once(NAME_SEPARATOR)?;
    if plugin_id.is_empty() || tool_id.is_empty() {
        return None;
    }
    Some((plugin_id.to_string(), tool_id.to_string()))
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

/// 按清洗后的 LLM 名找回 manifest 里的原始 plugin_id / tool_id
fn lookup_tool(
    plugins: &PluginState,
    sanitized_plugin_id: &str,
    sanitized_tool_id: &str,
) -> Option<(String, String)> {
    let plugins = plugins.plugins.lock().ok()?;
    for plugin in plugins.values() {
        if sanitize(&plugin.id) != sanitized_plugin_id {
            continue;
        }
        for tool in &plugin.contributes.tools {
            if sanitize(&tool.id) == sanitized_tool_id {
                return Some((plugin.id.clone(), tool.id.clone()));
            }
        }
    }
    None
}

/// 聚合执行器：命名空间名（含 `__`）走插件工具桥，其余走内置 ToolRegistry
pub struct PluginToolExecutor<'a> {
    pub app: &'a AppHandle,
    pub engine: &'a PermissionEngine,
    pub plugins: &'a PluginState,
    pub tool_state: &'a PluginToolState,
}

impl ToolExecutor for PluginToolExecutor<'_> {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            match parse_llm_name(name) {
                Some((plugin_id, tool_id)) => {
                    self.execute_plugin_tool(&plugin_id, &tool_id, args).await
                }
                None => ToolRegistry::execute(self.app, self.engine, name, &args).await,
            }
        })
    }
}

impl PluginToolExecutor<'_> {
    /// 插件工具路径：查 manifest 确认工具存在 → 挂 oneshot → emit 事件 → 超时等待
    ///
    /// 出错/超时统一返回 Err，agent 循环会把错误文本作为 tool 结果回喂 LLM
    async fn execute_plugin_tool(
        &self,
        plugin_id: &str,
        tool_id: &str,
        args: Value,
    ) -> Result<Value> {
        let (orig_plugin_id, orig_tool_id) = lookup_tool(self.plugins, plugin_id, tool_id)
            .ok_or_else(|| {
                VoloError::NotFound(format!("plugin tool: {}{}{}", plugin_id, NAME_SEPARATOR, tool_id))
            })?;

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
        PluginState {
            plugins: Mutex::new(
                plugins.into_iter().map(|p| (p.id.clone(), p)).collect(),
            ),
            plugins_dir: PathBuf::new(),
        }
    }

    // ---- 命名空间清洗 / 反解 ----

    #[test]
    fn test_sanitize() {
        assert_eq!(sanitize("uuid-gen"), "uuid-gen");
        assert_eq!(sanitize("gen_uuid"), "gen_uuid");
        assert_eq!(sanitize("my.plugin"), "my_plugin");
        assert_eq!(sanitize("生成 UUID"), "___UUID");
        assert_eq!(sanitize("a b/c.d"), "a_b_c_d");
    }

    #[test]
    fn test_llm_name_roundtrip() {
        let name = to_llm_name("uuid-gen", "gen_uuid");
        assert_eq!(name, "uuid-gen__gen_uuid");
        assert_eq!(
            parse_llm_name(&name),
            Some(("uuid-gen".to_string(), "gen_uuid".to_string()))
        );
    }

    #[test]
    fn test_parse_llm_name_with_single_underscores() {
        // 清洗后的 plugin_id 可能含单下划线，分隔符必须是第一个双下划线
        let name = to_llm_name("my.plugin", "gen_uuid");
        assert_eq!(name, "my_plugin__gen_uuid");
        assert_eq!(
            parse_llm_name(&name),
            Some(("my_plugin".to_string(), "gen_uuid".to_string()))
        );

        // tool_id 含双下划线时归 tool_id 一侧
        let name = to_llm_name("p", "a__b");
        assert_eq!(parse_llm_name(&name), Some(("p".to_string(), "a__b".to_string())));
    }

    #[test]
    fn test_parse_llm_name_rejects_builtin_and_empty() {
        assert!(parse_llm_name("clipboard_read").is_none());
        assert!(parse_llm_name("__tool").is_none());
        assert!(parse_llm_name("plugin__").is_none());
        assert!(parse_llm_name("").is_none());
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

        let mut specs = collect_specs(&state);
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(specs.len(), 2);

        assert_eq!(specs[0].name, "uuid-gen__gen_uuid");
        assert_eq!(specs[0].description, "生成指定数量的 UUID v4");
        assert_eq!(specs[0].parameters["type"], "object");

        // description 缺省回退工具 name
        assert_eq!(specs[1].name, "uuid-gen__no_desc");
        assert_eq!(specs[1].description, "无描述工具");
    }

    #[test]
    fn test_lookup_tool_matches_sanitized_ids() {
        let state = make_plugin_state(vec![make_plugin(
            "my.plugin",
            vec![make_tool("gen-uuid", "生成 UUID", None)],
        )]);

        assert_eq!(
            lookup_tool(&state, "my_plugin", "gen-uuid"),
            Some(("my.plugin".to_string(), "gen-uuid".to_string()))
        );
        assert!(lookup_tool(&state, "my_plugin", "nope").is_none());
        assert!(lookup_tool(&state, "other", "gen-uuid").is_none());
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
