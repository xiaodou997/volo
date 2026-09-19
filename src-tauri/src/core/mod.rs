//! 核心模块
//! 包含窗口管理、快捷键、托盘、配置等核心功能

pub mod capability;
pub mod clipboard_history;
pub mod config;
pub mod menu;
pub mod permission;
pub mod shortcut;
pub mod startup;
pub mod tray;
pub mod window;

pub use capability::{capability_meta, CapabilityMeta, RiskLevel};
pub use clipboard_history::ClipboardHistory;
pub use config::Config;
pub use permission::PermissionEngine;
pub use shortcut::ShortcutManager;
pub use startup::StartupManager;
pub use tray::create_tray;
pub use window::WindowManager;
