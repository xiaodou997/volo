//! 文件系统 API

use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;
use tauri::AppHandle;
use crate::error::Result;

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    #[serde(rename = "type")]
    pub file_type: String,
}

#[tauri::command]
pub async fn fs_read(path: String) -> Result<String> {
    let content = tokio::fs::read_to_string(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(content)
}

#[tauri::command]
pub async fn fs_write(path: String, content: String) -> Result<()> {
    tokio::fs::write(&path, &content).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn fs_exists(path: String) -> Result<bool> {
    let exists = tokio::fs::try_exists(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(exists)
}

#[tauri::command]
pub async fn fs_pick_file(app: AppHandle) -> Result<Option<String>> {
    let file_path = app.dialog()
        .file()
        .blocking_pick_file();
    
    Ok(file_path.map(|p| p.to_string()))
}

#[tauri::command]
pub async fn fs_pick_folder(app: AppHandle) -> Result<Option<String>> {
    let folder_path = app.dialog()
        .file()
        .blocking_pick_folder();
    
    Ok(folder_path.map(|p| p.to_string()))
}