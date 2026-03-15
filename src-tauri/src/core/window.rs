//! 窗口管理模块

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_positioner::{Position, WindowExt};
use crate::error::Result;

/// 窗口管理器
pub struct WindowManager;

impl WindowManager {
    /// 获取主窗口
    pub fn get_main_window(app: &AppHandle) -> Option<WebviewWindow> {
        app.get_webview_window("main")
    }

    /// 显示主窗口
    pub fn show_main_window(app: &AppHandle) -> Result<()> {
        if let Some(win) = Self::get_main_window(app) {
            // 定位到屏幕中央上方
            win.move_window(Position::Center)?;
            win.show()?;
            win.set_focus()?;
        }
        Ok(())
    }

    /// 隐藏主窗口
    pub fn hide_main_window(app: &AppHandle) -> Result<()> {
        if let Some(win) = Self::get_main_window(app) {
            win.hide()?;
        }
        Ok(())
    }

    /// 切换主窗口显示状态
    pub fn toggle_main_window(app: &AppHandle) -> Result<()> {
        if let Some(win) = Self::get_main_window(app) {
            if win.is_visible()? {
                win.hide()?;
            } else {
                Self::show_main_window(app)?;
            }
        }
        Ok(())
    }

    /// 设置窗口高度
    pub fn set_window_height(app: &AppHandle, height: u32) -> Result<()> {
        if let Some(win) = Self::get_main_window(app) {
            let size = win.inner_size()?;
            win.set_size(tauri::Size::Physical(tauri::PhysicalSize {
                width: size.width,
                height,
            }))?;
        }
        Ok(())
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<()> {
    WindowManager::show_main_window(&app)
}

#[tauri::command]
pub fn hide_main_window(app: AppHandle) -> Result<()> {
    WindowManager::hide_main_window(&app)
}

#[tauri::command]
pub fn toggle_main_window(app: AppHandle) -> Result<()> {
    WindowManager::toggle_main_window(&app)
}

#[tauri::command]
pub fn set_window_height(app: AppHandle, height: u32) -> Result<()> {
    WindowManager::set_window_height(&app, height)
}
