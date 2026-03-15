//! 插件搜索模块

use crate::plugin::manager::Plugin;
use crate::search::app_search::FeatureInfo;
use serde::{Deserialize, Serialize};

/// 插件搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSearchResult {
    pub plugin: PluginInfo,
    pub feature: FeatureInfo,
}

/// 简化的插件信息（用于搜索结果）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub description: Option<String>,
}

impl From<Plugin> for PluginInfo {
    fn from(p: Plugin) -> Self {
        Self {
            id: p.id,
            name: p.name,
            icon: p.icon,
            description: p.description,
        }
    }
}

/// 搜索插件功能
pub fn search_plugins(
    plugins: &[Plugin],
    query: &str,
) -> Vec<PluginSearchResult> {
    use crate::search::app_search::FeatureInfo;

    let query_lower = query.to_lowercase();

    let mut results: Vec<PluginSearchResult> = Vec::new();

    for plugin in plugins {
        // 检查每个功能
        for feature in &plugin.features {
            let mut matched = false;

            // 1. 功能名称匹配
            if feature.name.to_lowercase().contains(&query_lower) {
                matched = true;
            }

            // 2. 关键词匹配
            if !matched {
                for keyword in &feature.keywords {
                    if keyword.to_lowercase().contains(&query_lower) {
                        matched = true;
                        break;
                    }
                }
            }

            // 3. 功能 ID 匹配
            if !matched && feature.id.to_lowercase().contains(&query_lower) {
                matched = true;
            }

            if matched {
                let feature_info = FeatureInfo {
                    id: feature.id.clone(),
                    name: feature.name.clone(),
                    keywords: feature.keywords.clone(),
                };

                results.push(PluginSearchResult {
                    plugin: PluginInfo::from(plugin.clone()),
                    feature: feature_info,
                });
            }
        }
    }

    // 限制结果数量
    results.truncate(5);

    results
}