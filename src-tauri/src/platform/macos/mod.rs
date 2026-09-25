//! macOS native platform layer.
//!
//! Objective-C / AppKit / UserNotifications bindings must be contained here.
//! Higher-level API and core modules consume these wrappers instead of talking
//! to Objective-C frameworks directly.

pub mod app;
pub mod notification;
mod workspace;

pub use workspace::{get_app_icon, show_in_finder};
