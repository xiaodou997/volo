//! API 模块
//! 提供给插件的 API 接口

pub mod clipboard;
pub mod database;
pub mod notification;
pub mod screen;
pub mod shell;
pub mod fs;

// 重新导出所有 command
pub use clipboard::*;
pub use database::*;
pub use notification::*;
pub use screen::*;
pub use shell::*;
pub use fs::*;
