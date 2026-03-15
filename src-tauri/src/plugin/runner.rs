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

#[tauri::command]
pub async fn load_plugin(
    plugin_id: String,
    feature_id: String,
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
