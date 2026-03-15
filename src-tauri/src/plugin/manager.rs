//! 插件管理器

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tracing::{info, warn};
use crate::error::{Result, VoloError};
use crate::search::FeatureInfo;

/// 插件定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub main: String,
    pub path: PathBuf,
    #[serde(default)]
    pub features: Vec<Feature>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// 插件功能
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl From<Feature> for FeatureInfo {
    fn from(f: Feature) -> Self {
        FeatureInfo {
            id: f.id,
            name: f.name,
            keywords: f.keywords,
        }
    }
}

/// 插件状态
pub struct PluginState {
    pub plugins: Mutex<HashMap<String, Plugin>>,
    pub plugins_dir: PathBuf,
}

impl PluginState {
    pub fn new(app: &AppHandle) -> Result<Self> {
        let data_dir = app.path().app_data_dir()?;
        let plugins_dir = data_dir.join("plugins");
        std::fs::create_dir_all(&plugins_dir)?;

        let state = Self {
            plugins: Mutex::new(HashMap::new()),
            plugins_dir,
        };

        // 扫描已安装的插件
        if let Err(e) = state.scan_plugins() {
            warn!("Failed to scan plugins: {}", e);
        }

        Ok(state)
    }

    /// 扫描插件目录
    pub fn scan_plugins(&self) -> Result<()> {
        let mut plugins = self.plugins.lock()
            .map_err(|_| VoloError::Other("Lock error".to_string()))?;

        plugins.clear();

        if !self.plugins_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&self.plugins_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(plugin) = load_plugin_from_dir(&path) {
                    info!("Loaded plugin: {} ({})", plugin.name, plugin.id);
                    plugins.insert(plugin.id.clone(), plugin);
                }
            }
        }

        info!("Loaded {} plugins", plugins.len());
        Ok(())
    }

    /// 获取插件
    pub fn get_plugin(&self, id: &str) -> Option<Plugin> {
        let plugins = self.plugins.lock().ok()?;
        plugins.get(id).cloned()
    }

    /// 获取所有插件
    pub fn get_all_plugins(&self) -> Vec<Plugin> {
        let plugins = self.plugins.lock().ok();
        match plugins {
            Some(p) => p.values().cloned().collect(),
            None => Vec::new(),
        }
    }

    /// 安装插件（从本地目录）
    pub fn install_from_dir(&self, source_dir: &PathBuf) -> Result<Plugin> {
        // 验证源目录
        if !source_dir.exists() {
            return Err(VoloError::Other(format!("Source directory not found: {:?}", source_dir)));
        }

        // 加载插件信息
        let plugin = load_plugin_from_dir(source_dir)?;

        // 检查是否已安装
        let target_dir = self.plugins_dir.join(&plugin.id);
        if target_dir.exists() {
            // 删除旧版本
            fs::remove_dir_all(&target_dir)?;
        }

        // 创建目标目录
        fs::create_dir_all(&target_dir)?;

        // 复制所有文件
        copy_dir_all(source_dir, &target_dir)?;

        // 更新内存缓存
        let mut plugins = self.plugins.lock()
            .map_err(|_| VoloError::Other("Lock error".to_string()))?;

        let installed_plugin = Plugin {
            path: target_dir.clone(),
            ..plugin
        };

        plugins.insert(installed_plugin.id.clone(), installed_plugin.clone());

        info!("Installed plugin: {} ({})", installed_plugin.name, installed_plugin.id);
        Ok(installed_plugin)
    }

    /// 卸载插件
    pub fn uninstall(&self, id: &str) -> Result<()> {
        let plugin = self.get_plugin(id)
            .ok_or_else(|| VoloError::NotFound(id.to_string()))?;

        // 删除插件目录
        if plugin.path.exists() {
            fs::remove_dir_all(&plugin.path)?;
        }

        // 从内存缓存移除
        let mut plugins = self.plugins.lock()
            .map_err(|_| VoloError::Other("Lock error".to_string()))?;
        plugins.remove(id);

        info!("Uninstalled plugin: {}", id);
        Ok(())
    }
}

/// 从目录加载插件
fn load_plugin_from_dir(dir: &PathBuf) -> Result<Plugin> {
    let plugin_json_path = dir.join("plugin.json");
    if !plugin_json_path.exists() {
        return Err(VoloError::Plugin(format!(
            "plugin.json not found in {:?}",
            dir
        )));
    }

    let content = std::fs::read_to_string(&plugin_json_path)?;
    let mut plugin: Plugin = serde_json::from_str(&content)?;
    plugin.path = dir.clone();

    // 验证必要字段
    if plugin.id.is_empty() {
        return Err(VoloError::Plugin("Plugin id is required".to_string()));
    }
    if plugin.name.is_empty() {
        return Err(VoloError::Plugin("Plugin name is required".to_string()));
    }
    if plugin.main.is_empty() {
        plugin.main = "index.html".to_string();
    }

    Ok(plugin)
}

/// 递归复制目录
fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[tauri::command]
pub fn list_plugins(state: tauri::State<'_, PluginState>) -> Vec<Plugin> {
    let plugins = state.plugins.lock().unwrap();
    plugins.values().cloned().collect()
}

#[tauri::command]
pub fn get_plugin(id: String, state: tauri::State<'_, PluginState>) -> Result<Plugin> {
    state.get_plugin(&id)
        .ok_or_else(|| VoloError::NotFound(id))
}

#[tauri::command]
pub fn scan_plugins(state: tauri::State<'_, PluginState>) -> Result<()> {
    state.scan_plugins()
}

#[tauri::command]
pub fn install_plugin_from_dir(
    source_dir: String,
    state: tauri::State<'_, PluginState>,
) -> Result<Plugin> {
    let source_path = PathBuf::from(&source_dir);
    state.install_from_dir(&source_path)
}

#[tauri::command]
pub fn uninstall_plugin(
    id: String,
    state: tauri::State<'_, PluginState>,
) -> Result<()> {
    state.uninstall(&id)
}

// 保留旧的 install_plugin 以兼容
#[tauri::command]
pub async fn install_plugin(
    _source: String,
    _state: tauri::State<'_, PluginState>,
) -> Result<Plugin> {
    Err(VoloError::Plugin("Use install_plugin_from_dir instead".to_string()))
}