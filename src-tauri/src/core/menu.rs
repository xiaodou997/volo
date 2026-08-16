//! 原生菜单模块
//!
//! macOS 的 WKWebView 依赖应用菜单里的 Edit 项来响应 Cmd+C/V/X/A 等快捷键，
//! 没有菜单时 WebView 内无法粘贴/复制。这里构建最小可用的原生菜单。

use tauri::menu::{MenuBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::AppHandle;
use crate::error::Result;

/// 创建并设置应用菜单（仅 macOS 需要；其他平台跳过）
pub fn setup_menu(app: &AppHandle) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let app_menu = SubmenuBuilder::new(app, "Volo")
            .item(&PredefinedMenuItem::about(app, Some("关于 Volo"), None)?)
            .separator()
            .item(&PredefinedMenuItem::hide(app, Some("隐藏 Volo"))?)
            .item(&PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?)
            .separator()
            .item(&PredefinedMenuItem::quit(app, Some("退出 Volo"))?)
            .build()?;

        let edit_menu = SubmenuBuilder::new(app, "编辑")
            .item(&PredefinedMenuItem::undo(app, Some("撤销"))?)
            .item(&PredefinedMenuItem::redo(app, Some("重做"))?)
            .separator()
            .item(&PredefinedMenuItem::cut(app, Some("剪切"))?)
            .item(&PredefinedMenuItem::copy(app, Some("拷贝"))?)
            .item(&PredefinedMenuItem::paste(app, Some("粘贴"))?)
            .item(&PredefinedMenuItem::select_all(app, Some("全选"))?)
            .build()?;

        let menu = MenuBuilder::new(app)
            .items(&[&app_menu, &edit_menu])
            .build()?;

        app.set_menu(menu)?;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = app;

    Ok(())
}
