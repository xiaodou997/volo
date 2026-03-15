//! Shell API

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use crate::error::Result;

#[tauri::command]
pub async fn shell_open(app: AppHandle, url: String) -> Result<()> {
    app.opener().open_url(&url, None::<String>)
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn shell_open_path(_app: AppHandle, path: String) -> Result<()> {
    // 使用系统默认程序打开文件/文件夹
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    }
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    }
    
    Ok(())
}
