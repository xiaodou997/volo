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
    /// LLM 配置（含 API key，明文存于本地配置文件）
    #[serde(default)]
    pub llm: LlmConfig,
}

/// LLM 配置；base_url/model/api_key 为空字符串表示未配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    /// OpenAI 兼容服务地址（空则使用官方默认）
    pub base_url: String,
    /// 模型名
    pub model: String,
    /// API key（明文存于 config.json，注意勿外泄该文件）
    #[serde(default)]
    pub api_key: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut: "Alt+R".to_string(),
            theme: Theme::System,
            hide_on_blur: true,
            language: "zh-CN".to_string(),
            show_guide: true,
            llm: LlmConfig::default(),
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

/// 获取 LLM 配置（含 API key 是否已配置的状态由 llm_has_api_key 单独查询；
/// 注意：llm_get_config 返回的 LlmConfig 含 api_key 字段，前端不应回显）
#[tauri::command]
pub fn llm_get_config(config: tauri::State<'_, Config>) -> LlmConfig {
    config.get().llm
}

/// 保存 LLM 配置（base_url/model），不影响已保存的 API key
#[tauri::command]
pub fn llm_set_config(
    config: tauri::State<'_, Config>,
    base_url: String,
    model: String,
) -> Result<()> {
    let mut app_config = config.get();
    app_config.llm.base_url = base_url;
    app_config.llm.model = model;
    config.save(app_config)
}

/// 保存 LLM API key（明文写入本地 config.json）
#[tauri::command]
pub fn llm_set_api_key(config: tauri::State<'_, Config>, key: String) -> Result<()> {
    let mut app_config = config.get();
    app_config.llm.api_key = key;
    config.save(app_config)
}

/// 是否已配置 LLM API key
#[tauri::command]
pub fn llm_has_api_key(config: tauri::State<'_, Config>) -> bool {
    !config.get().llm.api_key.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧版本 config.json 没有 llm 字段，必须能向后兼容解析
    #[test]
    fn test_legacy_config_without_llm_parses() {
        let legacy = r#"{
            "shortcut": "Alt+R",
            "theme": "system",
            "hideOnBlur": true,
            "language": "zh-CN",
            "showGuide": true
        }"#;
        let config: AppConfig = serde_json::from_str(legacy).unwrap();
        assert_eq!(config.shortcut, "Alt+R");
        assert_eq!(config.llm.base_url, "");
        assert_eq!(config.llm.model, "");
    }

    #[test]
    fn test_llm_config_camel_case_roundtrip() {
        let config = AppConfig::default();
        let json = serde_json::to_value(&config).unwrap();
        assert!(json.get("llm").is_some());
        assert_eq!(json["llm"]["baseUrl"], "");
        assert_eq!(json["llm"]["model"], "");
    }
}
