//! 截图 API

use tauri::{AppHandle, State};
use crate::core::permission::{require, PermissionEngine};
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;
use base64::Engine;

/// 截取屏幕
#[tauri::command]
pub async fn screen_capture(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
) -> Result<String> {
    require(&app, &engine, &plugins, plugin_id.as_deref(), "screen.capture", None).await?;

    #[cfg(target_os = "macos")]
    {
        // 使用 screencapture 命令
        let output = std::process::Command::new("screencapture")
            .arg("-x")  // 不播放声音
            .arg("-c")  // 截图到剪贴板
            .output()
            .map_err(|e| VoloError::Other(format!("Failed to capture screen: {}", e)))?;

        if !output.status.success() {
            return Err(VoloError::Other("Screen capture failed".to_string()));
        }

        // 从剪贴板获取图片
        let clipboard_output = std::process::Command::new("pngpaste")
            .output();

        match clipboard_output {
            Ok(output) if output.status.success() => {
                let base64 = base64::engine::general_purpose::STANDARD.encode(&output.stdout);
                Ok(format!("data:image/png;base64,{}", base64))
            }
            _ => {
                // 如果 pngpaste 不可用，使用另一种方式
                // 创建临时文件
                let temp_dir = std::env::temp_dir();
                let temp_file = temp_dir.join(format!("volo_capture_{}.png", uuid::Uuid::new_v4()));

                let output = std::process::Command::new("screencapture")
                    .arg("-x")
                    .arg(&temp_file)
                    .output()
                    .map_err(|e| VoloError::Other(format!("Failed to capture screen: {}", e)))?;

                if !output.status.success() {
                    return Err(VoloError::Other("Screen capture failed".to_string()));
                }

                // 读取文件并转换为 base64
                let image_data = std::fs::read(&temp_file)?;
                let _ = std::fs::remove_file(&temp_file);  // 删除临时文件

                let base64 = base64::engine::general_purpose::STANDARD.encode(&image_data);
                Ok(format!("data:image/png;base64,{}", base64))
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(VoloError::Other("Screen capture not supported on this platform".to_string()))
    }
}

/// 截取选定区域
#[tauri::command]
pub async fn screen_capture_area(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
) -> Result<String> {
    require(&app, &engine, &plugins, plugin_id.as_deref(), "screen.capture", None).await?;

    #[cfg(target_os = "macos")]
    {
        // 使用 screencapture -s 让用户选择区域
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("volo_capture_{}.png", uuid::Uuid::new_v4()));

        let output = std::process::Command::new("screencapture")
            .arg("-x")
            .arg("-s")  // 选择区域模式
            .arg(&temp_file)
            .output()
            .map_err(|e| VoloError::Other(format!("Failed to capture screen: {}", e)))?;

        if !output.status.success() {
            return Err(VoloError::Other("Screen capture cancelled or failed".to_string()));
        }

        // 检查文件是否存在
        if !temp_file.exists() {
            return Err(VoloError::Other("Screen capture failed: no file created".to_string()));
        }

        // 读取文件并转换为 base64
        let image_data = std::fs::read(&temp_file)?;
        let _ = std::fs::remove_file(&temp_file);

        let base64 = base64::engine::general_purpose::STANDARD.encode(&image_data);
        Ok(format!("data:image/png;base64,{}", base64))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(VoloError::Other("Screen capture not supported on this platform".to_string()))
    }
}