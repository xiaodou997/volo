//! 应用搜索模块

use serde::{Deserialize, Serialize};
use crate::search::app_cache::{AppCache, AppInfo};
use crate::search::history::SearchHistoryManager;
use crate::search::plugin_search::{search_plugins, PluginInfo, PluginSearchResult};
use crate::search::file_index::FileIndex;
use crate::plugin::manager::PluginState;

/// 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SearchResult {
    App(AppInfo),
    Plugin { plugin: PluginInfo, feature: FeatureInfo },
    Command { plugin: PluginInfo, command: CommandInfo },
    File { path: String, name: String, file_type: String, extension: Option<String> },
}

/// 功能信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureInfo {
    pub id: String,
    pub name: String,
    pub keywords: Vec<String>,
}

/// 命令信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub id: String,
    pub name: String,
    pub keywords: Vec<String>,
    pub description: Option<String>,
    /// 命令模式："run" 或 "list"
    pub mode: String,
}

/// 带评分的应用信息
struct ScoredApp {
    app: AppInfo,
    score: f64,
}

/// 计算匹配评分
fn calculate_score(app: &AppInfo, query: &str, frecency: f64) -> f64 {
    let query_lower = query.to_lowercase();
    let name_lower = app.name.to_lowercase();
    let mut score = 0.0;

    // 1. 名称完全匹配（最高分）
    if name_lower == query_lower {
        score += 100.0;
    }
    // 2. 名称以查询开头
    else if name_lower.starts_with(&query_lower) {
        score += 80.0;
    }
    // 3. 名称包含查询
    else if name_lower.contains(&query_lower) {
        score += 60.0;
    }
    // 4. 拼音匹配
    else if let Some(ref pinyin) = app.pinyin {
        if pinyin.contains(&query_lower) {
            score += 50.0;
        }
    }
    // 5. 首字母匹配
    else if let Some(ref initials) = app.initials {
        if initials.starts_with(&query_lower) {
            score += 40.0;
        } else if initials.contains(&query_lower) {
            score += 30.0;
        }
    }

    // frecency 加成（频率封顶 20 + 近期使用最多 10）
    score += frecency;

    score
}

/// 搜索应用
/// 使用缓存进行搜索，支持名称、拼音、首字母匹配，按使用频率排序
pub fn search_apps(apps: &[AppInfo], query: &str, history: &SearchHistoryManager) -> Vec<AppInfo> {
    let query_lower = query.to_lowercase();
    let query_chars: Vec<char> = query_lower.chars().collect();

    // 计算每个应用的评分
    let mut scored_apps: Vec<ScoredApp> = apps
        .iter()
        .filter(|app| {
            let name_lower = app.name.to_lowercase();

            // 1. 名称包含匹配
            if name_lower.contains(&query_lower) {
                return true;
            }

            // 2. 拼音匹配
            if let Some(ref pinyin) = app.pinyin {
                if pinyin.contains(&query_lower) {
                    return true;
                }
            }

            // 3. 首字母匹配（如 "wx" 匹配 "微信"）
            if let Some(ref initials) = app.initials {
                if initials.starts_with(&query_lower) {
                    return true;
                }
            }

            // 4. 首字母子序列匹配
            if let Some(ref initials) = app.initials {
                let initials_chars: Vec<char> = initials.chars().collect();
                let mut initials_idx = 0;
                for c in &query_chars {
                    while initials_idx < initials_chars.len() {
                        if &initials_chars[initials_idx] == c {
                            initials_idx += 1;
                            break;
                        }
                        initials_idx += 1;
                    }
                    if initials_idx >= initials_chars.len() && initials_chars.last() != Some(c) {
                        return false;
                    }
                }
                if initials_idx > 0 {
                    return true;
                }
            }

            false
        })
        .map(|app| {
            let frecency = history.get_frecency(&app.path);
            let score = calculate_score(app, query, frecency);
            ScoredApp { app: app.clone(), score }
        })
        .collect();

    // 按评分排序
    scored_apps.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // 返回前 10 个结果
    scored_apps.into_iter().take(10).map(|s| s.app).collect()
}

#[tauri::command]
pub fn search(
    query: String,
    cache: tauri::State<'_, AppCache>,
    history: tauri::State<'_, SearchHistoryManager>,
    plugin_state: tauri::State<'_, PluginState>,
    file_index: tauri::State<'_, FileIndex>,
) -> Vec<SearchResult> {
    let mut results: Vec<SearchResult> = Vec::new();

    // 从缓存搜索应用
    let apps = cache.get_apps();
    for app in search_apps(&apps, &query, &history) {
        results.push(SearchResult::App(app));
    }

    // 搜索插件（功能与命令，按匹配分 + frecency 排序）
    let plugins = plugin_state.get_all_plugins();
    for plugin_result in search_plugins(&plugins, &query, history.inner()) {
        match plugin_result {
            PluginSearchResult::Feature { plugin, feature } => {
                results.push(SearchResult::Plugin { plugin, feature });
            }
            PluginSearchResult::Command { plugin, command } => {
                results.push(SearchResult::Command { plugin, command });
            }
        }
    }

    // 搜索文件（使用新的索引）
    if let Ok(files) = file_index.search(&query, 5) {
        for file in files {
            results.push(SearchResult::File {
                path: file.path,
                name: file.name,
                file_type: file.file_type,
                extension: file.extension,
            });
        }
    }

    results
}