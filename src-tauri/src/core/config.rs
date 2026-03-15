//! 配置管理模块

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri::Manager;
use crate::error::Result;

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// 快捷键
    pub shortcut: String,
    /// 主题
    pub theme: Theme,
    /// 是否隐藏到托盘
    pub hide_on_blur: bool,
    /// 语言
    pub language: String,
    /// 是否显示引导
    pub show_guide: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut: "Alt+R".to_string(),
            theme: Theme::System,
            hide_on_blur: true,
            language: "zh-CN".to_string(),
            show_guide: true,
        }
    }
}

/// 主题
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// 配置管理器
pub struct Config {
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
}

impl Config {
    /// 初始化配置
    pub fn init(app: &AppHandle) -> Result<Self> {
        let config_dir = app.path().app_config_dir()?;
        std::fs::create_dir_all(&config_dir)?;
        
        let config_path = config_dir.join("config.json");
        
        let config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            let config = AppConfig::default();
            let content = serde_json::to_string_pretty(&config)?;
            std::fs::write(&config_path, content)?;
            config
        };
        
        Ok(Self {
            config: Mutex::new(config),
            config_path,
        })
    }

    /// 获取配置
    pub fn get(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    /// 保存配置
    pub fn save(&self, config: AppConfig) -> Result<()> {
        let content = serde_json::to_string_pretty(&config)?;
        std::fs::write(&self.config_path, content)?;
        *self.config.lock().unwrap() = config;
        Ok(())
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn get_config(config: tauri::State<'_, Config>) -> AppConfig {
    config.get()
}

#[tauri::command]
pub fn save_config(config: tauri::State<'_, Config>, new_config: AppConfig) -> Result<()> {
    config.save(new_config)
}
