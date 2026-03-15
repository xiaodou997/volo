//! Windows 平台特定功能

use crate::error::{Result, VoloError};
use std::path::{Path, PathBuf};
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

/// 获取应用图标（返回 base64 编码的 PNG）
pub fn get_app_icon(app_path: &str) -> Result<Option<String>> {
    let app_path = Path::new(app_path);

    // 检查是否是 .exe 或 .lnk
    let ext = app_path.extension().map(|e| e.to_string_lossy().to_string());
    
    match ext.as_deref() {
        Some("exe") => get_exe_icon(app_path),
        Some("lnk") => get_lnk_icon(app_path),
        _ => Ok(None),
    }
}

/// 获取 exe 文件图标
fn get_exe_icon(exe_path: &Path) -> Result<Option<String>> {
    // 使用 PowerShell 提取图标
    let ps_script = format!(
        r#"
        Add-Type -AssemblyName System.Drawing
        $icon = [System.Drawing.Icon]::ExtractAssociatedIcon("{}")
        if ($icon) {{
            $bitmap = $icon.ToBitmap()
            $stream = New-Object System.IO.MemoryStream
            $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
            $bytes = $stream.ToArray()
            $stream.Close()
            [Convert]::ToBase64String($bytes)
        }}
        "#,
        exe_path.to_string_lossy().replace("\\", "\\\\")
    );

    let output = Command::new("powershell")
        .args(&["-Command", &ps_script])
        .output()
        .map_err(|e| VoloError::Other(format!("Failed to extract icon: {}", e)))?;

    if output.status.success() {
        let base64 = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !base64.is_empty() {
            return Ok(Some(format!("data:image/png;base64,{}", base64)));
        }
    }

    Ok(None)
}

/// 获取快捷方式图标
fn get_lnk_icon(lnk_path: &Path) -> Result<Option<String>> {
    // 先解析快捷方式获取目标路径
    let target = resolve_lnk_target(lnk_path)?;
    if let Some(target) = target {
        return get_exe_icon(&target);
    }
    Ok(None)
}

/// 解析快捷方式目标
fn resolve_lnk_target(lnk_path: &Path) -> Result<Option<PathBuf>> {
    let ps_script = format!(
        r#"
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut("{}")
        $shortcut.TargetPath
        "#,
        lnk_path.to_string_lossy()
    );

    let output = Command::new("powershell")
        .args(&["-Command", &ps_script])
        .output()
        .map_err(|e| VoloError::Other(format!("Failed to resolve lnk: {}", e)))?;

    if output.status.success() {
        let target = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !target.is_empty() && Path::new(&target).exists() {
            return Ok(Some(PathBuf::from(target)));
        }
    }

    Ok(None)
}

/// 扫描开始菜单应用
pub fn scan_start_menu() -> Result<Vec<(String, String)>> {
    let mut apps = Vec::new();

    // 获取开始菜单路径
    let start_menu_paths = get_start_menu_paths()?;

    for path in start_menu_paths {
        scan_directory(&path, &mut apps)?;
    }

    Ok(apps)
}

/// 扫描桌面应用
pub fn scan_desktop() -> Result<Vec<(String, String)>> {
    let mut apps = Vec::new();

    // 获取桌面路径
    let desktop_path = get_desktop_path()?;
    scan_directory(&desktop_path, &mut apps)?;

    Ok(apps)
}

/// 扫描目录中的快捷方式和 exe
fn scan_directory(dir: &Path, apps: &mut Vec<(String, String)>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // 递归扫描子目录
            scan_directory(&path, apps)?;
        } else {
            let ext = path.extension().map(|e| e.to_string_lossy().to_string());
            
            match ext.as_deref() {
                Some("lnk") | Some("exe") | Some("url") => {
                    let name = path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    
                    if !name.is_empty() {
                        apps.push((name, path.to_string_lossy().to_string()));
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// 获取开始菜单路径
fn get_start_menu_paths() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    // 系统开始菜单
    let sys_start_menu = PathBuf::from("C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs");
    if sys_start_menu.exists() {
        paths.push(sys_start_menu);
    }

    // 用户开始菜单
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        let user_start_menu = PathBuf::from(user_profile)
            .join("AppData")
            .join("Roaming")
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        if user_start_menu.exists() {
            paths.push(user_start_menu);
        }
    }

    Ok(paths)
}

/// 获取桌面路径
fn get_desktop_path() -> Result<PathBuf> {
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        let desktop = PathBuf::from(user_profile).join("Desktop");
        if desktop.exists() {
            return Ok(desktop);
        }
    }

    // 备用方案
    Ok(PathBuf::from("C:\\Users\\Public\\Desktop"))
}

/// 从注册表获取已安装应用
pub fn scan_registry_apps() -> Result<Vec<(String, String)>> {
    let mut apps = Vec::new();

    // 扫描卸载注册表项
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let uninstall = hklm.open_subkey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
    
    if let Ok(uninstall) = uninstall {
        for key in uninstall.enum_keys().filter_map(|k| k.ok()) {
            if let Ok(subkey) = uninstall.open_subkey(&key) {
                if let Ok(display_name) = subkey.get_value::<String, _>("DisplayName") {
                    if let Ok(install_location) = subkey.get_value::<String, _>("InstallLocation") {
                        if !install_location.is_empty() {
                            // 查找可执行文件
                            if let Some(exe) = find_executable_in_dir(&install_location) {
                                apps.push((display_name, exe));
                            }
                        }
                    }
                }
            }
        }
    }

    // 扫描用户卸载注册表项
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall = hkcu.open_subkey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
    
    if let Ok(uninstall) = uninstall {
        for key in uninstall.enum_keys().filter_map(|k| k.ok()) {
            if let Ok(subkey) = uninstall.open_subkey(&key) {
                if let Ok(display_name) = subkey.get_value::<String, _>("DisplayName") {
                    if let Ok(install_location) = subkey.get_value::<String, _>("InstallLocation") {
                        if !install_location.is_empty() {
                            if let Some(exe) = find_executable_in_dir(&install_location) {
                                apps.push((display_name, exe));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(apps)
}

/// 在目录中查找可执行文件
fn find_executable_in_dir(dir: &str) -> Option<String> {
    let path = Path::new(dir);
    if !path.exists() || !path.is_dir() {
        return None;
    }

    // 常见可执行文件名
    let common_names = ["app.exe", "launcher.exe", "start.exe", "run.exe"];

    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        
        if let Some(ext) = path.extension() {
            if ext == "exe" {
                let file_name = path.file_name()?.to_string_lossy().to_lowercase();
                // 优先返回常见名称
                if common_names.iter().any(|&name| file_name.contains(name)) {
                    return Some(path.to_string_lossy().to_string());
                }
            }
        }
    }

    // 返回第一个找到的 exe
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        
        if let Some(ext) = path.extension() {
            if ext == "exe" {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

/// 打开应用
pub fn open_app(app_path: &str) -> Result<()> {
    let path = Path::new(app_path);
    
    if path.extension().map_or(false, |e| e == "lnk") {
        // 使用快捷方式打开
        Command::new("cmd")
            .args(&["/c", "start", "", app_path])
            .spawn()
            .map_err(|e| VoloError::Other(format!("Failed to open app: {}", e)))?;
    } else {
        // 直接打开
        Command::new("cmd")
            .args(&["/c", "start", "", app_path])
            .spawn()
            .map_err(|e| VoloError::Other(format!("Failed to open app: {}", e)))?;
    }

    Ok(())
}