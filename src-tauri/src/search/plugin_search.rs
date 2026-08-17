//! 插件搜索模块

use crate::plugin::manager::Plugin;
use crate::search::app_search::{CommandInfo, FeatureInfo};
use serde::{Deserialize, Serialize};

/// 插件搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PluginSearchResult {
    Feature { plugin: PluginInfo, feature: FeatureInfo },
    Command { plugin: PluginInfo, command: CommandInfo },
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

/// 名称/关键词/id 包含匹配（大小写不敏感）
fn matches(query_lower: &str, id: &str, name: &str, keywords: &[String]) -> bool {
    // 1. 名称匹配
    if name.to_lowercase().contains(query_lower) {
        return true;
    }

    // 2. 关键词匹配
    for keyword in keywords {
        if keyword.to_lowercase().contains(query_lower) {
            return true;
        }
    }

    // 3. ID 匹配
    id.to_lowercase().contains(query_lower)
}

/// 搜索插件功能与命令
pub fn search_plugins(
    plugins: &[Plugin],
    query: &str,
) -> Vec<PluginSearchResult> {
    let query_lower = query.to_lowercase();

    let mut results: Vec<PluginSearchResult> = Vec::new();

    for plugin in plugins {
        // 检查每个功能
        for feature in &plugin.features {
            if matches(&query_lower, &feature.id, &feature.name, &feature.keywords) {
                results.push(PluginSearchResult::Feature {
                    plugin: PluginInfo::from(plugin.clone()),
                    feature: FeatureInfo {
                        id: feature.id.clone(),
                        name: feature.name.clone(),
                        keywords: feature.keywords.clone(),
                    },
                });
            }
        }

        // 检查每个命令
        for command in &plugin.contributes.commands {
            if matches(&query_lower, &command.id, &command.name, &command.keywords) {
                results.push(PluginSearchResult::Command {
                    plugin: PluginInfo::from(plugin.clone()),
                    command: CommandInfo {
                        id: command.id.clone(),
                        name: command.name.clone(),
                        keywords: command.keywords.clone(),
                        description: command.description.clone(),
                        mode: command.mode.clone(),
                    },
                });
            }
        }
    }

    // 限制结果数量（features 与 commands 合计）
    results.truncate(5);

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manager::{CommandSpec, Contributes, Feature};
    use std::path::PathBuf;

    fn make_plugin() -> Plugin {
        Plugin {
            id: "test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            main: "index.html".to_string(),
            path: PathBuf::from("/tmp/test-plugin"),
            features: vec![Feature {
                id: "open-view".to_string(),
                name: "Open View".to_string(),
                keywords: vec!["view".to_string()],
                icon: None,
                description: None,
            }],
            permissions: vec![],
            description: None,
            icon: None,
            contributes: Contributes {
                commands: vec![CommandSpec {
                    id: "gen-uuid".to_string(),
                    name: "Generate UUID".to_string(),
                    keywords: vec!["uuid".to_string()],
                    description: Some("Generate a UUID".to_string()),
                    run: "command.js".to_string(),
                    icon: None,
                    mode: "list".to_string(),
                }],
                tools: vec![],
            },
        }
    }

    #[test]
    fn test_search_command_by_name() {
        let results = search_plugins(&[make_plugin()], "generate");
        assert_eq!(results.len(), 1);
        match &results[0] {
            PluginSearchResult::Command { plugin, command } => {
                assert_eq!(plugin.id, "test-plugin");
                assert_eq!(command.id, "gen-uuid");
                assert_eq!(command.name, "Generate UUID");
                assert_eq!(command.description.as_deref(), Some("Generate a UUID"));
                // mode 从 CommandSpec 透传到搜索结果
                assert_eq!(command.mode, "list");
            }
            _ => panic!("expected command result"),
        }
    }

    #[test]
    fn test_search_command_by_keyword() {
        let results = search_plugins(&[make_plugin()], "UUID");
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], PluginSearchResult::Command { .. }));
    }

    #[test]
    fn test_search_command_by_id() {
        let results = search_plugins(&[make_plugin()], "gen-uuid");
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], PluginSearchResult::Command { .. }));
    }

    #[test]
    fn test_search_feature_still_works() {
        let results = search_plugins(&[make_plugin()], "open");
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], PluginSearchResult::Feature { .. }));
    }
}
