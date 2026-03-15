//! 剪贴板 API

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use crate::error::Result;
use base64::Engine;

/// 读取剪贴板文本
#[tauri::command]
pub async fn clipboard_read_text(app: AppHandle) -> Result<String> {
    app.clipboard().read_text()
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))
}

/// 写入剪贴板文本
#[tauri::command]
pub async fn clipboard_write_text(app: AppHandle, text: String) -> Result<()> {
    app.clipboard().write_text(&text)
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))
}

/// 读取剪贴板图片（返回 base64）
#[tauri::command]
pub async fn clipboard_read_image(app: AppHandle) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        // 使用 pngpaste 读取剪贴板图片
        let output = std::process::Command::new("pngpaste")
            .output();

        match output {
            Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                let base64 = base64::engine::general_purpose::STANDARD.encode(&output.stdout);
                Ok(Some(format!("data:image/png;base64,{}", base64)))
            }
            _ => Ok(None),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        // 其他平台暂不支持
        Ok(None)
    }
}

/// 写入图片到剪贴板（从 base64）
#[tauri::command]
pub async fn clipboard_write_image(app: AppHandle, base64: String) -> Result<()> {
    // 移除 data:image/xxx;base64, 前缀
    let base64_data = if base64.contains(",") {
        base64.split(",").nth(1).unwrap_or(&base64)
    } else {
        &base64
    };

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;

    #[cfg(target_os = "macos")]
    {
        // 写入临时文件
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("volo_clipboard_{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&temp_file, &bytes)
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;

        // 使用 osascript 设置剪贴板图片
        let script = format!(
            r#"
            set theImageFile to POSIX file "{}" as alias
            set theData to read theImageFile as «class PNGf»
            set the clipboard to theData
            "#,
            temp_file.to_string_lossy()
        );

        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output();

        // 删除临时文件
        let _ = std::fs::remove_file(&temp_file);
    }

    Ok(())
}

/// 读取剪贴板文件列表
#[tauri::command]
pub async fn clipboard_read_files() -> Result<Vec<String>> {
    #[cfg(target_os = "macos")]
    {
        // 使用 osascript 读取剪贴板文件
        let script = r#"
            use AppleScript version "2.4"
            use scripting additions
            use framework "Foundation"
            use framework "AppKit"

            set thePasteboard to current application's NSPasteboard's generalPasteboard()
            set theURLs to thePasteboard's readObjectsForClasses:{current application's NSURL} options:(missing value)

            if theURLs is missing value then
                return ""
            end if

            set thePaths to {}
            repeat with aURL in theURLs
                set end of thePaths to (aURL's |path|() as text)
            end repeat

            return thePaths as text
        "#;

        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output();

        match output {
            Ok(output) if output.status.success() => {
                let result = String::from_utf8_lossy(&output.stdout);
                let paths: Vec<String> = result
                    .split(", ")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                Ok(paths)
            }
            _ => Ok(Vec::new()),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(Vec::new())
    }
}
