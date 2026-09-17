use chrono::Utc;
use tauri::AppHandle;

use crate::error::Result;

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
