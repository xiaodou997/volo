//! 快捷键管理模块

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
use crate::error::{Result, VoloError};

/// 快捷键管理器
pub struct ShortcutManager;

impl ShortcutManager {
    /// 注册默认快捷键 (Alt+R)
    pub fn register_default(app: &AppHandle) -> Result<()> {
        let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyR);
        
        let app_handle = app.clone();
        app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, _event| {
            // 切换主窗口
            if let Some(win) = app_handle.get_webview_window("main") {
                if win.is_visible().unwrap_or(false) {
                    let _ = win.hide();
                } else {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        }).map_err(|e| VoloError::Other(e.to_string()))?;
        
        Ok(())
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn register_shortcut(app: AppHandle, shortcut: String) -> Result<()> {
    let _app = app;
    let _shortcut_str = shortcut;
    // TODO: 实现自定义快捷键注册
    Ok(())
}

#[tauri::command]
pub fn unregister_shortcut(app: AppHandle, shortcut: String) -> Result<()> {
    let _app = app;
    let _shortcut_str = shortcut;
    // TODO: 实现快捷键注销
    Ok(())
}