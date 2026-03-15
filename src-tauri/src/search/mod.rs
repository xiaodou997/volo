//! 搜索模块

pub mod app_cache;
pub mod app_search;
pub mod history;
pub mod plugin_search;

pub use app_cache::{AppCache, AppInfo};
pub use app_search::{SearchResult, FeatureInfo, search};
pub use history::SearchHistoryManager;