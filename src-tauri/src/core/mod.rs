//! 核心模块
//! 包含窗口管理、快捷键、托盘、配置等核心功能

pub mod window;
pub mod shortcut;
pub mod tray;
pub mod config;
pub mod clipboard_history;

pub use window::WindowManager;
pub use shortcut::ShortcutManager;
pub use tray::create_tray;
pub use config::Config;
pub use clipboard_history::ClipboardHistory;
