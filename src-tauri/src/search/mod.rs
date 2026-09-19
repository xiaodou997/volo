//! 搜索模块

pub mod app_cache;
pub mod app_search;
pub mod file_index;
pub mod file_search;
pub mod history;
pub mod plugin_search;

pub use app_cache::{AppCache, AppInfo};
pub use app_search::{search, CommandInfo, FeatureInfo, SearchResult};
pub use file_index::{FileIndex, FileInfo as FileIndexInfo, IndexStats};
pub use file_search::{FileInfo as FileSearchInfo, FileSearcher};
pub use history::SearchHistoryManager;
pub use plugin_search::{PluginInfo, PluginSearchResult};
