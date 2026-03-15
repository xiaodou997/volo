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
            // 先设置窗口大小为默认大小
            win.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 800.0,
                height: 60.0,
            }))?;
            // 定位到屏幕中央
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
            // 使用逻辑像素大小，宽度保持 800
            win.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 800.0,
                height: height as f64,
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
