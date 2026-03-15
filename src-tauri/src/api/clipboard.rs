//! 剪贴板 API

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use crate::error::Result;

#[tauri::command]
pub async fn clipboard_read_text(app: AppHandle) -> Result<String> {
    app.clipboard().read_text()
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))
}

#[tauri::command]
pub async fn clipboard_write_text(app: AppHandle, text: String) -> Result<()> {
    app.clipboard().write_text(&text)
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))
}
