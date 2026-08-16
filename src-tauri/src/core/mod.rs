//! 核心模块
//! 包含窗口管理、快捷键、托盘、配置等核心功能

pub mod window;
pub mod shortcut;
pub mod tray;
pub mod menu;
pub mod config;
pub mod clipboard_history;
pub mod startup;
pub mod capability;
pub mod permission;

pub use window::WindowManager;
pub use shortcut::ShortcutManager;
pub use tray::create_tray;
pub use config::Config;
pub use clipboard_history::ClipboardHistory;
pub use startup::StartupManager;
pub use capability::{capability_meta, CapabilityMeta, RiskLevel};
pub use permission::PermissionEngine;
