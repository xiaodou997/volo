//! 快捷键管理模块

use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
use crate::error::{Result, VoloError};

/// 快捷键管理器
pub struct ShortcutManager {
    pub current_shortcut: Mutex<String>,
}

impl ShortcutManager {
    pub fn new() -> Self {
        Self {
            current_shortcut: Mutex::new("Alt+R".to_string()),
        }
    }

    /// 解析快捷键字符串
    fn parse_shortcut(shortcut: &str) -> Result<(Option<Modifiers>, Code)> {
        let parts: Vec<&str> = shortcut.split('+').collect();

        let mut modifiers = None;
        let mut key = None;

        for part in parts.iter() {
            match part.trim().to_lowercase().as_str() {
                "cmd" | "command" => modifiers = Some(modifiers.unwrap_or(Modifiers::empty()) | Modifiers::SUPER),
                "ctrl" | "control" => modifiers = Some(modifiers.unwrap_or(Modifiers::empty()) | Modifiers::CONTROL),
                "alt" | "option" => modifiers = Some(modifiers.unwrap_or(Modifiers::empty()) | Modifiers::ALT),
                "shift" => modifiers = Some(modifiers.unwrap_or(Modifiers::empty()) | Modifiers::SHIFT),
                "space" => key = Some(Code::Space),
                "a" => key = Some(Code::KeyA),
                "b" => key = Some(Code::KeyB),
                "c" => key = Some(Code::KeyC),
                "d" => key = Some(Code::KeyD),
                "e" => key = Some(Code::KeyE),
                "f" => key = Some(Code::KeyF),
                "g" => key = Some(Code::KeyG),
                "h" => key = Some(Code::KeyH),
                "i" => key = Some(Code::KeyI),
                "j" => key = Some(Code::KeyJ),
                "k" => key = Some(Code::KeyK),
                "l" => key = Some(Code::KeyL),
                "m" => key = Some(Code::KeyM),
                "n" => key = Some(Code::KeyN),
                "o" => key = Some(Code::KeyO),
                "p" => key = Some(Code::KeyP),
                "q" => key = Some(Code::KeyQ),
                "r" => key = Some(Code::KeyR),
                "s" => key = Some(Code::KeyS),
                "t" => key = Some(Code::KeyT),
                "u" => key = Some(Code::KeyU),
                "v" => key = Some(Code::KeyV),
                "w" => key = Some(Code::KeyW),
                "x" => key = Some(Code::KeyX),
                "y" => key = Some(Code::KeyY),
                "z" => key = Some(Code::KeyZ),
                "0" => key = Some(Code::Digit0),
                "1" => key = Some(Code::Digit1),
                "2" => key = Some(Code::Digit2),
                "3" => key = Some(Code::Digit3),
                "4" => key = Some(Code::Digit4),
                "5" => key = Some(Code::Digit5),
                "6" => key = Some(Code::Digit6),
                "7" => key = Some(Code::Digit7),
                "8" => key = Some(Code::Digit8),
                "9" => key = Some(Code::Digit9),
                "f1" => key = Some(Code::F1),
                "f2" => key = Some(Code::F2),
                "f3" => key = Some(Code::F3),
                "f4" => key = Some(Code::F4),
                "f5" => key = Some(Code::F5),
                "f6" => key = Some(Code::F6),
                "f7" => key = Some(Code::F7),
                "f8" => key = Some(Code::F8),
                "f9" => key = Some(Code::F9),
                "f10" => key = Some(Code::F10),
                "f11" => key = Some(Code::F11),
                "f12" => key = Some(Code::F12),
                _ => {}
            }
        }

        let code = key.ok_or_else(|| VoloError::Other("Invalid shortcut key".to_string()))?;
        Ok((modifiers, code))
    }

    /// 注册默认快捷键
    pub fn register_default(app: &AppHandle) -> Result<()> {
        let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyR);

        let app_handle = app.clone();
        app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
            // 只在按键按下时触发
            use tauri_plugin_global_shortcut::ShortcutState;
            if event.state != ShortcutState::Pressed {
                return;
            }

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

impl Default for ShortcutManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn register_shortcut(
    app: AppHandle,
    manager: tauri::State<'_, ShortcutManager>,
    shortcut: String,
) -> Result<()> {
    // 解析快捷键
    let (modifiers, code) = ShortcutManager::parse_shortcut(&shortcut)?;
    let new_shortcut = Shortcut::new(modifiers, code);

    // 获取当前快捷键
    let current = manager.current_shortcut.lock()
        .map_err(|_| VoloError::Other("Failed to lock shortcut manager".to_string()))?
        .clone();

    // 注销旧快捷键
    if let Ok((old_modifiers, old_code)) = ShortcutManager::parse_shortcut(&current) {
        let old_shortcut = Shortcut::new(old_modifiers, old_code);
        let _ = app.global_shortcut().unregister(old_shortcut);
    }

    // 注册新快捷键
    let app_handle = app.clone();
    app.global_shortcut().on_shortcut(new_shortcut, move |_app, _shortcut, event| {
        // 只在按键按下时触发
        use tauri_plugin_global_shortcut::ShortcutState;
        if event.state != ShortcutState::Pressed {
            return;
        }

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

    // 更新当前快捷键
    if let Ok(mut current) = manager.current_shortcut.lock() {
        *current = shortcut;
    }

    Ok(())
}

#[tauri::command]
pub fn unregister_shortcut(app: AppHandle, shortcut: String) -> Result<()> {
    let (modifiers, code) = ShortcutManager::parse_shortcut(&shortcut)?;
    let shortcut = Shortcut::new(modifiers, code);

    app.global_shortcut().unregister(shortcut)
        .map_err(|e| VoloError::Other(e.to_string()))?;

    Ok(())
}