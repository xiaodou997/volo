use chrono::Utc;
use tauri::{AppHandle, Manager};

use crate::core::capability::{capability_meta, RiskLevel};
use crate::core::permission::{enforce_background, PermissionEngine};
use crate::error::{Result, VoloError};

use super::{storage, AutomationRecord, WorkflowAutomation};

#[tauri::command]
pub fn automation_list(app: AppHandle) -> Result<Vec<AutomationRecord>> {
    storage::list_automations(&storage::automations_dir(&app)?)
}

/// 保存 renderer 可编辑的 Automation definition。
/// `nextRunAt` 由后端根据旧记录和当前时间计算，renderer 无法直接覆盖。
#[tauri::command]
pub fn automation_save(
    app: AppHandle,
    automation: WorkflowAutomation,
) -> Result<AutomationRecord> {
    storage::save_automation(&storage::automations_dir(&app)?, automation, Utc::now())
}

#[tauri::command]
pub fn automation_delete(app: AppHandle, automation_id: String) -> Result<()> {
    storage::delete_automation(&storage::automations_dir(&app)?, &automation_id)
}


/// 为已保存 Workflow 主动发起一次标准后台权限审批。
///
/// 不直接写授权表：Medium / High / Critical 仍走现有 permission-request UI。
/// 审批完成后再做 background 裁决，因此只有 Always 能满足无人值守执行。
#[tauri::command]
pub async fn permission_request_workflow_always(
    app: AppHandle,
    workflow_id: String,
    capability: String,
    resource: Option<String>,
) -> Result<()> {
    let workflow_id = workflow_id.trim();
    if workflow_id.is_empty() {
        return Err(VoloError::Other("workflow_id cannot be empty".to_string()));
    }

    // 只允许给真实、当前可加载的已保存 Workflow 预授权，避免产生孤儿 principal grant。
    crate::ai::workflow::commands::load_saved_workflow(&app, workflow_id)?;

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

    let principal = crate::ai::workflow::commands::workflow_principal(workflow_id);
    let engine = app.state::<PermissionEngine>();
    engine
        .enforce(&app, &principal, capability, resource.as_deref())
        .await?;

    // Once / Session 仍不具备后台语义；必须最终存在匹配的 Always grant。
    enforce_background(&engine, &principal, capability, resource.as_deref())
}
