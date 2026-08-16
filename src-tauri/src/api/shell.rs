//! Shell API

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use crate::core::permission::{require, PermissionEngine};
use crate::error::Result;
use crate::plugin::manager::PluginState;

#[tauri::command]
pub async fn shell_open(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    url: String,
) -> Result<()> {
    require(&app, &engine, &plugins, plugin_id.as_deref(), "shell.open", Some(&url)).await?;

    app.opener().open_url(&url, None::<String>)
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn shell_open_path(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
) -> Result<()> {
    require(&app, &engine, &plugins, plugin_id.as_deref(), "shell.open", Some(&path)).await?;

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
