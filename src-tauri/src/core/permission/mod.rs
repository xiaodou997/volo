//! 权限模块
//! Capability 声明检查 + 运行时审批引擎 + 审计

pub mod audit;
pub mod engine;
pub mod store;

pub use engine::{Decision, Grant, GrantInfo, PermissionEngine, Scope};

use tauri::{AppHandle, State};

use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

/// 权限守卫：插件面 API 命令的统一入口
///
/// - `plugin_id = None`：主窗口/系统自用，直接放行
/// - 插件不存在 → NotFound
/// - 未声明 capability → 审计 + PermissionDenied
/// - 已声明 → 交由 PermissionEngine 裁决（授权查询 / 运行时审批）
pub async fn require(
    app: &AppHandle,
    engine: &PermissionEngine,
    plugins: &PluginState,
    plugin_id: Option<&str>,
    capability: &str,
    resource: Option<&str>,
) -> Result<()> {
    let pid = match plugin_id {
        None => return Ok(()),
        Some(pid) => pid,
    };

    let plugin = plugins
        .get_plugin(pid)
        .ok_or_else(|| VoloError::NotFound(format!("plugin: {}", pid)))?;

    if !PermissionEngine::declared(&plugin.permissions, capability) {
        engine.audit(pid, capability, resource, "deny", None);
        return Err(VoloError::PermissionDenied(format!(
            "Plugin '{}' does not declare permission '{}'",
            pid, capability
        )));
    }

    engine.enforce(app, pid, capability, resource).await
}

// ============ Tauri Commands ============

/// 用户审批响应
#[tauri::command]
pub fn permission_respond(
    engine: State<'_, PermissionEngine>,
    request_id: String,
    allow: bool,
    scope: Option<String>,
) -> Result<()> {
    let scope = match scope.as_deref() {
        Some("session") => Scope::Session,
        Some("always") => Scope::Always,
        _ => Scope::Once,
    };
    engine.respond(&request_id, allow, scope)
}

/// 列出当前所有授权
#[tauri::command]
pub fn permission_list_grants(engine: State<'_, PermissionEngine>) -> Result<Vec<GrantInfo>> {
    engine.list_grants()
}

/// 撤销授权
#[tauri::command]
pub fn permission_revoke(
    engine: State<'_, PermissionEngine>,
    plugin_id: String,
    capability: String,
) -> Result<()> {
    engine.revoke(&plugin_id, &capability)
}
