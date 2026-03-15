//! 插件系统模块

pub mod manager;
pub mod permission;
pub mod runner;

pub use manager::PluginState;
pub use permission::PermissionManager;
pub use runner::PluginRunner;