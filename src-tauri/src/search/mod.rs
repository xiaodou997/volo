//! 搜索模块

pub mod app_cache;
pub mod app_search;
pub mod file_search;
pub mod file_index;
pub mod history;
pub mod plugin_search;

pub use app_cache::{AppCache, AppInfo};
pub use app_search::{SearchResult, FeatureInfo, search};
pub use file_search::{FileSearcher, FileInfo as FileSearchInfo};
pub use file_index::{FileIndex, FileInfo as FileIndexInfo, IndexStats};
pub use history::SearchHistoryManager;
pub use plugin_search::{PluginSearchResult, PluginInfo};