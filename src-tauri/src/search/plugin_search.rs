//! 插件搜索模块

use crate::plugin::manager::Plugin;
use crate::search::app_search::{CommandInfo, FeatureInfo};
use crate::search::history::SearchHistoryManager;
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

/// 名称/关键词/id 匹配评分（大小写不敏感），不匹配返回 None。
/// 分层：名称 > 关键词 > id；每层内精确 > 前缀 > 包含。frecency 作为加成计入总分
fn score_match(
    query_lower: &str,
    id: &str,
    name: &str,
    keywords: &[String],
    frecency: f64,
) -> Option<f64> {
    let name_lower = name.to_lowercase();
    let mut score = if name_lower == query_lower {
        Some(100.0)
    } else if name_lower.starts_with(query_lower) {
        Some(80.0)
    } else if name_lower.contains(query_lower) {
        Some(60.0)
    } else {
        None
    };

    if score.is_none() {
        for keyword in keywords {
            let kw = keyword.to_lowercase();
            score = if kw == query_lower {
                Some(50.0)
            } else if kw.starts_with(query_lower) {
                Some(40.0)
            } else if kw.contains(query_lower) {
                Some(30.0)
            } else {
                continue;
            };
            break;
        }
    }

    if score.is_none() && id.to_lowercase().contains(query_lower) {
        score = Some(20.0);
    }

    score.map(|s| s + frecency)
}

/// 搜索插件功能与命令，按匹配分 + frecency 降序，取前 5
pub fn search_plugins(
    plugins: &[Plugin],
    query: &str,
    history: &SearchHistoryManager,
) -> Vec<PluginSearchResult> {
    let query_lower = query.to_lowercase();

    let mut scored: Vec<(f64, PluginSearchResult)> = Vec::new();

    for plugin in plugins {
        // 检查每个功能
        for feature in &plugin.features {
            let key = format!("{}#{}", plugin.id, feature.id);
            if let Some(score) = score_match(
                &query_lower,
                &feature.id,
                &feature.name,
                &feature.keywords,
                history.get_frecency(&key),
            ) {
                scored.push((
                    score,
                    PluginSearchResult::Feature {
                        plugin: PluginInfo::from(plugin.clone()),
                        feature: FeatureInfo {
                            id: feature.id.clone(),
                            name: feature.name.clone(),
                            keywords: feature.keywords.clone(),
                        },
                    },
                ));
            }
        }

        // 检查每个命令
        for command in &plugin.contributes.commands {
            let key = format!("{}#{}", plugin.id, command.id);
            if let Some(score) = score_match(
                &query_lower,
                &command.id,
                &command.name,
                &command.keywords,
                history.get_frecency(&key),
            ) {
                scored.push((
                    score,
                    PluginSearchResult::Command {
                        plugin: PluginInfo::from(plugin.clone()),
                        command: CommandInfo {
                            id: command.id.clone(),
                            name: command.name.clone(),
                            keywords: command.keywords.clone(),
                            description: command.description.clone(),
                            mode: command.mode.clone(),
                        },
                    },
                ));
            }
        }
    }

    // 按评分降序（同分保持扫描顺序），取前 5
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(5).map(|(_, result)| result).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manager::{CommandSpec, Contributes, Feature};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DB_SEQ: AtomicU64 = AtomicU64::new(0);

    /// 每个测试用独立的临时历史库（测试并行运行，不能共享）
    fn temp_history() -> SearchHistoryManager {
        let seq = DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "volo-plugin-search-test-{}-{}.db",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_file(&path);
        SearchHistoryManager::new(&path).unwrap()
    }

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

    /// 构造只含一个命令的插件
    fn make_cmd_plugin(plugin_id: &str, cmd_id: &str, name: &str, keywords: Vec<String>) -> Plugin {
        Plugin {
            id: plugin_id.to_string(),
            name: plugin_id.to_string(),
            version: "1.0.0".to_string(),
            main: "index.html".to_string(),
            path: PathBuf::from("/tmp").join(plugin_id),
            features: vec![],
            permissions: vec![],
            description: None,
            icon: None,
            contributes: Contributes {
                commands: vec![CommandSpec {
                    id: cmd_id.to_string(),
                    name: name.to_string(),
                    keywords,
                    description: None,
                    run: "command.js".to_string(),
                    icon: None,
                    mode: "run".to_string(),
                }],
                tools: vec![],
            },
        }
    }

    #[test]
    fn test_search_command_by_name() {
        let history = temp_history();
        let results = search_plugins(&[make_plugin()], "generate", &history);
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
        let history = temp_history();
        let results = search_plugins(&[make_plugin()], "UUID", &history);
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], PluginSearchResult::Command { .. }));
    }

    #[test]
    fn test_search_command_by_id() {
        let history = temp_history();
        let results = search_plugins(&[make_plugin()], "gen-uuid", &history);
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], PluginSearchResult::Command { .. }));
    }

    #[test]
    fn test_search_feature_still_works() {
        let history = temp_history();
        let results = search_plugins(&[make_plugin()], "open", &history);
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], PluginSearchResult::Feature { .. }));
    }

    #[test]
    fn test_name_match_outranks_keyword_match() {
        let history = temp_history();
        // 两个插件都匹配 "json"：a 只在关键词里含，b 名称前缀命中 → b 排前
        let a = make_cmd_plugin("a", "a-cmd", "Format Tool", vec!["json".to_string()]);
        let b = make_cmd_plugin("b", "b-cmd", "JSON Viewer", vec![]);
        let results = search_plugins(&[a, b], "json", &history);
        assert_eq!(results.len(), 2);
        match &results[0] {
            PluginSearchResult::Command { plugin, .. } => assert_eq!(plugin.id, "b"),
            _ => panic!("expected command result"),
        }
    }

    #[test]
    fn test_frecency_breaks_ties() {
        let history = temp_history();
        // 两个名称同样前缀命中的命令，用过的排前面
        let a = make_cmd_plugin("a", "a-cmd", "UUID Alpha", vec![]);
        let b = make_cmd_plugin("b", "b-cmd", "UUID Beta", vec![]);
        history.record_usage("b#b-cmd").unwrap();
        let results = search_plugins(&[a, b], "uuid", &history);
        assert_eq!(results.len(), 2);
        match &results[0] {
            PluginSearchResult::Command { plugin, .. } => assert_eq!(plugin.id, "b"),
            _ => panic!("expected command result"),
        }
    }

    #[test]
    fn test_results_capped_at_five() {
        let history = temp_history();
        let plugins: Vec<Plugin> = (0..8)
            .map(|i| {
                make_cmd_plugin(
                    &format!("p{i}"),
                    &format!("c{i}"),
                    &format!("UUID Tool {i}"),
                    vec![],
                )
            })
            .collect();
        let results = search_plugins(&plugins, "uuid", &history);
        assert_eq!(results.len(), 5);
    }
}
