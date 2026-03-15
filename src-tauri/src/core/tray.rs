//! 系统托盘模块

use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Manager,
};
use crate::error::Result;

/// 创建系统托盘
pub fn create_tray(app: &AppHandle) -> Result<TrayIcon> {
    // 创建托盘菜单
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    
    let menu = Menu::with_items(app, &[&show, &settings, &quit])?;
    
    // 创建托盘图标
    let tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "show" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
                "settings" => {
                    // TODO: 打开设置页面
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // 左键点击托盘图标
            if let tauri::tray::TrayIconEvent::Click { button, .. } = event {
                if button == tauri::tray::MouseButton::Left {
                    let app = tray.app_handle();
                    if let Some(win) = app.get_webview_window("main") {
                        if win.is_visible().unwrap_or(false) {
                            let _ = win.hide();
                        } else {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                }
            }
        })
        .build(app)?;
    
    Ok(tray)
}