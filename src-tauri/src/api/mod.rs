//! API 模块
//! 提供给插件的 API 接口

pub mod clipboard;
pub mod database;
pub mod fs;
pub mod notification;
#[cfg(target_os = "macos")]
pub mod notification_macos;
pub mod screen;
pub mod shell;

// 重新导出所有 command
pub use clipboard::*;
pub use database::*;
pub use fs::*;
pub use notification::*;
pub use screen::*;
pub use shell::*;
