//! 配置管理模块

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri::Manager;

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
    /// 是否在 Dock 栏显示应用图标（仅 macOS 生效，其余平台仅持久化）
    #[serde(default = "default_true")]
    pub show_dock_icon: bool,
    /// LLM 配置。api_key 仅后端持有，返回 renderer 前会清空。
    #[serde(default)]
    pub llm: LlmConfig,
    /// MCP server 配置。env 可能包含 token，返回 renderer 前会清空。
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// MCP server 配置：url 非空 = 远程 Streamable HTTP server；否则 = stdio 本地子进程
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// 启动命令（如 npx、python、某个二进制路径）；url 非空时忽略
    pub command: String,
    /// 命令参数
    #[serde(default)]
    pub args: Vec<String>,
    /// 附加环境变量；可能包含 API token 等 secret，不应返回 renderer。
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// 远程 server 的 URL（Streamable HTTP transport，单 endpoint POST JSON-RPC）；
    /// 为空则按 stdio 本地子进程处理
    #[serde(default)]
    pub url: String,
    /// 是否启用（缺省启用）
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// LLM 配置；base_url/model/api_key 为空字符串表示未配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    /// OpenAI 兼容服务地址（空则使用官方默认）
    pub base_url: String,
    /// 模型名
    pub model: String,
    /// API key。当前仍用于后端持久化兼容；任何 IPC getter 都必须先 redact。
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
            show_dock_icon: true,
            llm: LlmConfig::default(),
            mcp_servers: HashMap::new(),
        }
    }
}

impl AppConfig {
    /// 生成允许返回给 renderer 的配置副本。
    /// Secret 只允许通过 write-only 命令写入，绝不能通过通用 config getter 回传。
    fn redacted_for_renderer(&self) -> Self {
        let mut redacted = self.clone();
        redacted.llm.api_key.clear();
        for server in redacted.mcp_servers.values_mut() {
            server.env.clear();
        }
        redacted
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

    /// 获取后端完整配置（含 secret）。只允许 Rust 内部使用。
    pub fn get(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    /// 获取 renderer 可见配置（secret 已清空）。
    pub fn get_redacted(&self) -> AppConfig {
        self.get().redacted_for_renderer()
    }

    /// 保存后端完整配置。
    pub fn save(&self, config: AppConfig) -> Result<()> {
        let content = serde_json::to_string_pretty(&config)?;
        std::fs::write(&self.config_path, content)?;
        *self.config.lock().unwrap() = config;
        Ok(())
    }

    /// 保存来自 renderer 的普通配置更新。
    ///
    /// renderer 永远拿不到 LLM API key / MCP env，因此也不能通过 save_config 覆盖、清空或伪造这些字段。
    /// 对现有 MCP server，secret env 始终从后端旧值合并回来；新 server 的 env 强制为空。
    fn save_from_renderer(&self, mut config: AppConfig) -> Result<()> {
        let existing = self.get();
        config.llm.api_key = existing.llm.api_key;

        for (name, server) in config.mcp_servers.iter_mut() {
            server.env = existing
                .mcp_servers
                .get(name)
                .map(|old| old.env.clone())
                .unwrap_or_default();
        }

        self.save(config)
    }
}

// ============ Tauri Commands ============

/// 获取通用配置。所有 secret 在跨 IPC 前已清空。
#[tauri::command]
pub fn get_config(config: tauri::State<'_, Config>) -> AppConfig {
    config.get_redacted()
}

/// 保存 renderer 可编辑配置；secret 字段由后端保留，renderer 无法覆盖。
#[tauri::command]
pub fn save_config(config: tauri::State<'_, Config>, new_config: AppConfig) -> Result<()> {
    config.save_from_renderer(new_config)
}

/// 获取 LLM 非敏感配置。api_key 字段始终为空；是否已配置由 llm_has_api_key 查询。
#[tauri::command]
pub fn llm_get_config(config: tauri::State<'_, Config>) -> LlmConfig {
    config.get_redacted().llm
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

/// 保存 LLM API key。该命令是 write-only secret 入口，key 不会通过任何 getter 返回 renderer。
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

/// 应用 Dock 图标显隐（仅 macOS 有效，其余平台为空操作）
pub fn apply_dock_icon_visibility(app: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app.set_activation_policy(policy) {
            tracing::warn!("set_activation_policy failed: {}", e);
        }
        // Accessory → Regular 切换后 macOS 会把 Dock 图标还原成进程默认的 exec 图标，
        // 显式重设应用图标修复（对打包应用无影响，图标同源）
        if visible {
            set_macos_app_icon();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, visible);
    }
}

/// macOS：用内嵌的 icon.png 显式设置 NSApplication 图标
// objc 0.2 的宏内部使用了过时的 `cfg(feature = "cargo-clippy")`，在宏展开处无法消除，故允许
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn set_macos_app_icon() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    static PNG: &[u8] = include_bytes!("../../icons/icon.png");
    unsafe {
        let data: *mut Object = msg_send![class!(NSData),
            dataWithBytes: PNG.as_ptr() as *const std::ffi::c_void
            length: PNG.len() as u64
        ];
        if data.is_null() {
            return;
        }
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let nsapp: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![nsapp, setApplicationIconImage: image];
    }
}

/// 设置 Dock 图标显隐：持久化配置并立即生效（macOS）
#[tauri::command]
pub fn set_dock_icon_visible(
    app: AppHandle,
    config: tauri::State<'_, Config>,
    visible: bool,
) -> Result<()> {
    let mut app_config = config.get();
    app_config.show_dock_icon = visible;
    config.save(app_config)?;
    apply_dock_icon_visibility(&app, visible);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "volo_config_test_{}_{}",
            name,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

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

    #[test]
    fn test_renderer_config_redacts_secrets() {
        let mut config = AppConfig::default();
        config.llm.api_key = "sk-super-secret".to_string();
        config.mcp_servers.insert(
            "filesystem".to_string(),
            McpServerConfig {
                command: "node".to_string(),
                args: vec![],
                env: HashMap::from([
                    ("TOKEN".to_string(), "top-secret".to_string()),
                    ("PATH_HINT".to_string(), "not-secret-but-private".to_string()),
                ]),
                url: String::new(),
                enabled: true,
            },
        );

        let redacted = config.redacted_for_renderer();
        assert!(redacted.llm.api_key.is_empty());
        assert!(redacted.mcp_servers["filesystem"].env.is_empty());

        // 后端原值不能因生成 redacted 副本而被改动。
        assert_eq!(config.llm.api_key, "sk-super-secret");
        assert_eq!(config.mcp_servers["filesystem"].env["TOKEN"], "top-secret");
    }

    #[test]
    fn test_renderer_save_preserves_backend_secrets() {
        let path = temp_config_path("preserve_secrets");
        let mut existing = AppConfig::default();
        existing.llm.api_key = "sk-existing".to_string();
        existing.mcp_servers.insert(
            "server".to_string(),
            McpServerConfig {
                command: "node".to_string(),
                args: vec!["old.js".to_string()],
                env: HashMap::from([("TOKEN".to_string(), "secret".to_string())]),
                url: String::new(),
                enabled: true,
            },
        );

        let state = Config {
            config: Mutex::new(existing),
            config_path: path.clone(),
        };

        let mut from_renderer = state.get_redacted();
        from_renderer.llm.model = "new-model".to_string();
        from_renderer.llm.api_key = "malicious-overwrite".to_string();
        from_renderer.mcp_servers.get_mut("server").unwrap().args = vec!["new.js".to_string()];
        from_renderer.mcp_servers.get_mut("server").unwrap().env =
            HashMap::from([("TOKEN".to_string(), "malicious-overwrite".to_string())]);

        state.save_from_renderer(from_renderer).unwrap();
        let saved = state.get();
        assert_eq!(saved.llm.model, "new-model");
        assert_eq!(saved.llm.api_key, "sk-existing");
        assert_eq!(saved.mcp_servers["server"].args, vec!["new.js"]);
        assert_eq!(saved.mcp_servers["server"].env["TOKEN"], "secret");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// 旧版本 config.json 没有 mcpServers 字段，必须能向后兼容解析
    #[test]
    fn test_legacy_config_without_mcp_servers_parses() {
        let legacy = r#"{
            "shortcut": "Alt+R",
            "theme": "system",
            "hideOnBlur": true,
            "language": "zh-CN",
            "showGuide": true
        }"#;
        let config: AppConfig = serde_json::from_str(legacy).unwrap();
        assert!(config.mcp_servers.is_empty());
        // 旧配置没有 showDockIcon 字段，默认显示
        assert!(config.show_dock_icon);
    }

    #[test]
    fn test_mcp_servers_camel_case() {
        let mut config = AppConfig::default();
        config.mcp_servers.insert(
            "echo".to_string(),
            McpServerConfig {
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "echo-server".to_string()],
                env: HashMap::from([("KEY".to_string(), "VALUE".to_string())]),
                url: String::new(),
                enabled: true,
            },
        );
        let json = serde_json::to_value(&config).unwrap();
        let server = &json["mcpServers"]["echo"];
        assert_eq!(server["command"], "npx");
        assert_eq!(server["args"], serde_json::json!(["-y", "echo-server"]));
        assert_eq!(server["env"]["KEY"], "VALUE");
        assert_eq!(server["enabled"], true);

        // args/env/enabled 缺省时的解析
        let minimal: McpServerConfig =
            serde_json::from_str(r#"{"command": "echo-server"}"#).unwrap();
        assert!(minimal.args.is_empty());
        assert!(minimal.env.is_empty());
        assert!(minimal.enabled);
    }
}
