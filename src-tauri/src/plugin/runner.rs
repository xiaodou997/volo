//! 插件运行器

use std::path::PathBuf;
use tauri::AppHandle;
use crate::plugin::manager::{Plugin, Feature};
use crate::error::{Result, VoloError};

/// 插件运行时信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginRuntime {
    pub plugin: Plugin,
    pub feature: Feature,
    pub html_path: PathBuf,
}

/// 插件运行器
pub struct PluginRunner;

impl PluginRunner {
    /// 获取插件运行时信息
    pub fn get_runtime(plugin: &Plugin, feature: &Feature) -> Result<PluginRuntime> {
        let html_path = plugin.path.join(&plugin.main);
        if !html_path.exists() {
            return Err(VoloError::Plugin(format!(
                "Plugin main file not found: {:?}",
                html_path
            )));
        }

        Ok(PluginRuntime {
            plugin: plugin.clone(),
            feature: feature.clone(),
            html_path,
        })
    }
}

#[tauri::command]
pub fn get_plugin_runtime(
    plugin_id: String,
    feature_id: String,
    state: tauri::State<'_, crate::plugin::manager::PluginState>,
) -> Result<PluginRuntime> {
    let plugin = state.get_plugin(&plugin_id)
        .ok_or_else(|| VoloError::NotFound(plugin_id.clone()))?;

    let feature = plugin.features.iter()
        .find(|f| f.id == feature_id)
        .cloned()
        .ok_or_else(|| VoloError::NotFound(feature_id.clone()))?;

    PluginRunner::get_runtime(&plugin, &feature)
}

/// 获取插件 HTML 内容
#[tauri::command]
pub fn get_plugin_html(
    plugin_id: String,
    state: tauri::State<'_, crate::plugin::manager::PluginState>,
) -> Result<String> {
    let plugin = state.get_plugin(&plugin_id)
        .ok_or_else(|| VoloError::NotFound(plugin_id.clone()))?;

    let html_path = plugin.path.join(&plugin.main);
    if !html_path.exists() {
        return Err(VoloError::Plugin(format!(
            "Plugin main file not found: {:?}",
            html_path
        )));
    }

    std::fs::read_to_string(&html_path)
        .map_err(|e| VoloError::Other(format!("Failed to read plugin HTML: {}", e)))
}

/// 获取插件资源路径
#[tauri::command]
pub fn get_plugin_asset_path(
    plugin_id: String,
    asset_name: String,
    state: tauri::State<'_, crate::plugin::manager::PluginState>,
) -> Result<String> {
    let plugin = state.get_plugin(&plugin_id)
        .ok_or_else(|| VoloError::NotFound(plugin_id.clone()))?;

    let asset_path = plugin.path.join(&asset_name);
    if !asset_path.exists() {
        return Err(VoloError::NotFound(asset_name));
    }

    Ok(asset_path.to_string_lossy().to_string())
}

/// 解析命令源码路径，防止路径穿越脱出插件目录
fn resolve_command_source(plugin: &Plugin, command_id: &str) -> Result<PathBuf> {
    let command = plugin.contributes.commands.iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| VoloError::NotFound(command_id.to_string()))?;

    let base = plugin.path.canonicalize()
        .map_err(|e| VoloError::Plugin(format!("Failed to resolve plugin dir: {}", e)))?;
    let source_path = base.join(&command.run).canonicalize()
        .map_err(|_| VoloError::NotFound(command.run.clone()))?;

    if !source_path.starts_with(&base) {
        return Err(VoloError::Plugin(format!(
            "Command source path escapes plugin directory: {}",
            command.run
        )));
    }

    Ok(source_path)
}

/// 获取命令（no-view）扩展的源码
#[tauri::command]
pub fn get_plugin_command_source(
    plugin_id: String,
    command_id: String,
    state: tauri::State<'_, crate::plugin::manager::PluginState>,
) -> Result<String> {
    let plugin = state.get_plugin(&plugin_id)
        .ok_or_else(|| VoloError::NotFound(plugin_id.clone()))?;

    let source_path = resolve_command_source(&plugin, &command_id)?;

    std::fs::read_to_string(&source_path)
        .map_err(|e| VoloError::Other(format!("Failed to read command source: {}", e)))
}

#[tauri::command]
pub async fn load_plugin(
    _plugin_id: String,
    _feature_id: String,
    _state: tauri::State<'_, crate::plugin::manager::PluginState>,
    _app: AppHandle,
) -> Result<()> {
    // 插件加载逻辑现在由前端处理
    // 这里只做验证
    Ok(())
}

#[tauri::command]
pub async fn unload_plugin() -> Result<()> {
    // 插件卸载逻辑现在由前端处理
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manager::{CommandSpec, Contributes};

    fn temp_plugin_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("volo_runner_test_{}_{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_plugin(path: PathBuf, run: &str) -> Plugin {
        Plugin {
            id: "test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            main: "index.html".to_string(),
            path,
            features: vec![],
            permissions: vec![],
            description: None,
            icon: None,
            contributes: Contributes {
                commands: vec![CommandSpec {
                    id: "cmd".to_string(),
                    name: "Command".to_string(),
                    keywords: vec![],
                    description: None,
                    run: run.to_string(),
                    icon: None,
                }],
            },
        }
    }

    #[test]
    fn test_resolve_command_source_ok() {
        let dir = temp_plugin_dir("source_ok");
        std::fs::write(dir.join("command.js"), "console.log('hi');").unwrap();

        let plugin = make_plugin(dir.clone(), "command.js");
        let path = resolve_command_source(&plugin, "cmd").unwrap();
        assert!(path.ends_with("command.js"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_command_source_unknown_command() {
        let dir = temp_plugin_dir("unknown_cmd");
        let plugin = make_plugin(dir.clone(), "command.js");
        let result = resolve_command_source(&plugin, "nope");
        assert!(matches!(result, Err(VoloError::NotFound(_))));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_command_source_missing_file() {
        let dir = temp_plugin_dir("missing_file");
        let plugin = make_plugin(dir.clone(), "command.js");
        let result = resolve_command_source(&plugin, "cmd");
        assert!(matches!(result, Err(VoloError::NotFound(_))));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_command_source_path_traversal_rejected() {
        let dir = temp_plugin_dir("traversal");
        let plugin_dir = dir.join("plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        // 在插件目录外放置一个文件
        std::fs::write(dir.join("evil.js"), "evil();").unwrap();

        let plugin = make_plugin(plugin_dir, "../evil.js");
        let result = resolve_command_source(&plugin, "cmd");
        assert!(matches!(result, Err(VoloError::Plugin(_))));

        std::fs::remove_dir_all(&dir).ok();
    }
}
