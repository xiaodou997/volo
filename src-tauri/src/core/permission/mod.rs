//! 权限模块
//! Capability 声明检查 + 运行时审批引擎 + 审计

pub mod audit;
pub mod background;
pub mod engine;
pub mod store;

pub use background::enforce_background;
pub use engine::{Decision, Grant, GrantInfo, PermissionEngine, Scope};

use tauri::{AppHandle, State};

use crate::core::capability::{capability_meta, RiskLevel};
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

/// 权限守卫：插件面 API 命令的统一入口
///
/// - `plugin_id = None`：主窗口/系统自用，直接放行
/// - 插件不存在 → NotFound（禁用插件也不会出现在 Runtime 中）
/// - 未声明 capability / resource → 审计 + PermissionDenied
/// - 已声明 → 交由 PermissionEngine 裁决（资源级授权查询 / 运行时审批）
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

    if !PermissionEngine::declared(&plugin.permissions, capability, resource) {
        engine.audit(pid, capability, resource, "deny", None);
        return Err(VoloError::PermissionDenied(format!(
            "Plugin '{}' does not declare permission '{}' for resource {:?}",
            pid, capability, resource
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

/// 为后台 Workflow 主动发起一次标准权限审批。
///
/// 这个命令不会直接写授权表，也不会绕过现有审批 UI：Medium / High / Critical
/// 仍通过 PermissionEngine::enforce 发出 permission-request。审批完成后再用
/// enforce_background 校验结果，只有用户选择 Always 才算真正满足后台执行要求。
#[tauri::command]
pub async fn permission_request_workflow_always(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    workflow_id: String,
    capability: String,
    resource: Option<String>,
) -> Result<()> {
    let workflow_id = workflow_id.trim();
    if workflow_id.is_empty() {
        return Err(VoloError::Other("workflow_id cannot be empty".to_string()));
    }

    let capability = capability.trim();
    if capability.is_empty() {
        return Err(VoloError::Other("capability cannot be empty".to_string()));
    }

    let meta = capability_meta(capability);
    if meta.id == "unknown" {
        return Err(VoloError::Other(format!(
            "Unknown capability '{}' cannot be pre-granted for background execution",
            capability
        )));
    }
    if meta.risk == RiskLevel::Low {
        return Err(VoloError::Other(format!(
            "Low-risk capability '{}' does not require a background Always grant",
            capability
        )));
    }

    let principal = format!("workflow:{}", workflow_id);
    engine
        .enforce(&app, &principal, capability, resource.as_deref())
        .await?;

    // Session / Once 可以放行前台审批本身，但不能成为无人值守授权。
    // 再走一次后台裁决，确保 UI 最终只把 Always 视为成功。
    enforce_background(&engine, &principal, capability, resource.as_deref())
}

/// 撤销授权。resource 为空时撤销该 capability 的全部资源授权，以兼容旧前端。
#[tauri::command]
pub fn permission_revoke(
    engine: State<'_, PermissionEngine>,
    plugin_id: String,
    capability: String,
    resource: Option<String>,
) -> Result<()> {
    engine.revoke(&plugin_id, &capability, resource.as_deref())
}
