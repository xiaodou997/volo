//! 截图 API

use tauri::{AppHandle, State};

use crate::core::permission::{require, PermissionEngine};
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

#[cfg(target_os = "macos")]
use base64::Engine;

/// 非交互整屏截图 primitive。
///
/// 直接写入独立临时文件，不经过系统剪贴板，避免 screen.capture 隐式产生
/// clipboard.read / clipboard.write 副作用。可安全复用于前台命令和后台 Headless Tool。
pub(crate) fn capture_screen_image() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let temp_file =
            std::env::temp_dir().join(format!("volo_capture_{}.png", uuid::Uuid::new_v4()));

        let result = (|| -> Result<String> {
            let output = std::process::Command::new("screencapture")
                .arg("-x")
                .arg(&temp_file)
                .output()
                .map_err(|error| {
                    VoloError::Other(format!("Failed to capture screen: {}", error))
                })?;

            if !output.status.success() {
                return Err(VoloError::Other("Screen capture failed".to_string()));
            }

            let image_data = std::fs::read(&temp_file)?;
            let base64 = base64::engine::general_purpose::STANDARD.encode(image_data);
            Ok(format!("data:image/png;base64,{}", base64))
        })();

        let _ = std::fs::remove_file(&temp_file);
        result
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(VoloError::Other(
            "Screen capture not supported on this platform".to_string(),
        ))
    }
}

/// 截取屏幕
#[tauri::command]
pub async fn screen_capture(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
) -> Result<String> {
    require(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "screen.capture",
        None,
    )
    .await?;

    capture_screen_image()
}

/// 截取选定区域。
///
/// 这是交互式能力，需要用户现场框选区域，因此只保留前台命令，不提供给后台 Headless Tool。
#[tauri::command]
pub async fn screen_capture_area(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
) -> Result<String> {
    require(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "screen.capture",
        None,
    )
    .await?;

    #[cfg(target_os = "macos")]
    {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("volo_capture_{}.png", uuid::Uuid::new_v4()));

        let output = std::process::Command::new("screencapture")
            .arg("-x")
            .arg("-s")
            .arg(&temp_file)
            .output()
            .map_err(|error| VoloError::Other(format!("Failed to capture screen: {}", error)))?;

        if !output.status.success() {
            return Err(VoloError::Other(
                "Screen capture cancelled or failed".to_string(),
            ));
        }

        if !temp_file.exists() {
            return Err(VoloError::Other(
                "Screen capture failed: no file created".to_string(),
            ));
        }

        let image_data = std::fs::read(&temp_file)?;
        let _ = std::fs::remove_file(&temp_file);

        let base64 = base64::engine::general_purpose::STANDARD.encode(image_data);
        Ok(format!("data:image/png;base64,{}", base64))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(VoloError::Other(
            "Screen capture not supported on this platform".to_string(),
        ))
    }
}
