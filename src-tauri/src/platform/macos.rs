//! macOS 平台特定功能

use crate::error::{Result, VoloError};
use std::path::Path;
use std::process::Command;
use tracing::{debug, warn};

/// 获取应用图标（返回 base64 编码的 PNG）
pub fn get_app_icon(app_path: &str) -> Result<Option<String>> {
    let app_path = Path::new(app_path);

    // 检查是否是 .app 包
    if app_path.extension().map_or(true, |ext| ext != "app") {
        return Ok(None);
    }

    // 读取 Info.plist
    let info_plist_path = app_path.join("Contents").join("Info.plist");
    if !info_plist_path.exists() {
        debug!("Info.plist not found: {:?}", info_plist_path);
        return Ok(None);
    }

    // 解析 plist 获取图标文件名
    let icon_filename = get_icon_filename(&info_plist_path)?;

    // 查找图标文件
    let resources_dir = app_path.join("Contents").join("Resources");

    // 尝试多种可能的图标文件
    let icon_candidates = vec![
        icon_filename.clone(),
        "AppIcon.icns".to_string(),
        "app.icns".to_string(),
        format!("{}.icns", app_path.file_stem().unwrap_or_default().to_string_lossy()),
    ];

    for candidate in icon_candidates {
        if candidate.is_empty() {
            continue;
        }

        let icon_path = resources_dir.join(&candidate);
        if icon_path.exists() {
            // 使用 sips 转换为 PNG 并获取 base64
            return convert_icns_to_base64(&icon_path);
        }
    }

    debug!("No icon file found for: {:?}", app_path);
    Ok(None)
}

/// 从 Info.plist 获取图标文件名
fn get_icon_filename(plist_path: &Path) -> Result<String> {
    let plist_content = std::fs::read(plist_path)?;
    let plist: plist::Value = plist::from_bytes(&plist_content)
        .map_err(|e| VoloError::Other(format!("Failed to parse plist: {}", e)))?;

    // 获取 CFBundleIconFile
    if let Some(plist::Value::String(icon_file)) = plist.as_dictionary()
        .and_then(|d| d.get("CFBundleIconFile"))
    {
        // 如果没有扩展名，添加 .icns
        if icon_file.ends_with(".icns") {
            Ok(icon_file.clone())
        } else {
            Ok(format!("{}.icns", icon_file))
        }
    } else {
        Ok(String::new())
    }
}

/// 使用 sips 将 .icns 转换为 base64 PNG
fn convert_icns_to_base64(icns_path: &Path) -> Result<Option<String>> {
    // 创建临时文件
    let temp_dir = std::env::temp_dir();
    let temp_png = temp_dir.join(format!("volo_icon_{}.png", uuid::Uuid::new_v4()));

    // 使用 sips 转换
    let output = Command::new("sips")
        .arg("-s")
        .arg("format")
        .arg("png")
        .arg("--resampleWidth")
        .arg("64")  // 64x64 图标
        .arg(icns_path)
        .arg("--out")
        .arg(&temp_png)
        .output();

    match output {
        Ok(output) => {
            if !output.status.success() {
                warn!("sips failed: {}", String::from_utf8_lossy(&output.stderr));
                return Ok(None);
            }

            // 读取 PNG 文件
            let png_data = std::fs::read(&temp_png)?;
            let base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_data);

            // 删除临时文件
            let _ = std::fs::remove_file(&temp_png);

            Ok(Some(format!("data:image/png;base64,{}", base64)))
        }
        Err(e) => {
            warn!("Failed to run sips: {}", e);
            Ok(None)
        }
    }
}

/// 在 Finder 中显示
pub fn show_in_finder(path: &str) -> Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()?;
    Ok(())
}
