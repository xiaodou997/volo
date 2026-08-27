//! 插件管理器

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Manager};
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
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default)]
    pub features: Vec<Feature>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub contributes: Contributes,
}

/// 插件贡献点（Manifest v2）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
    #[serde(default)]
    pub tools: Vec<ToolManifestSpec>,
}

/// 命令（no-view）扩展定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub run: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// 命令模式："run"（默认，直接执行）或 "list"（列表模式）
    #[serde(default = "default_run_mode")]
    pub mode: String,
}

/// 命令模式默认值：run
fn default_run_mode() -> String {
    "run".to_string()
}

/// 工具（Agent 可调用）扩展定义
///
/// parameters 是工具的入参 JSON Schema（透传给 LLM），必须是 object 类型；
/// manifest 缺省时补默认空 object schema，加载时校验（见 load_plugin_from_dir）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifestSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_object_schema")]
    pub parameters: serde_json::Value,
    pub run: String,
    #[serde(default)]
    pub icon: Option<String>,
}

/// 工具入参的默认 JSON Schema：空 object
pub fn default_object_schema() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
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
    /// 热重载 watcher（仅保活，drop 即停止监听；事件经 start_hot_reload 的防抖线程处理）
    _watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

impl PluginState {
    pub fn new(app: &AppHandle) -> Result<Self> {
        let data_dir = app.path().app_data_dir()?;
        let plugins_dir = data_dir.join("plugins");
        std::fs::create_dir_all(&plugins_dir)?;

        let state = Self {
            plugins: Mutex::new(HashMap::new()),
            plugins_dir,
            _watcher: Mutex::new(None),
        };

        // 播种内置插件（已安装且版本一致的跳过，版本变化时覆盖更新）
        state.seed_builtin_plugins(app);

        // 扫描已安装的插件
        if let Err(e) = state.scan_plugins() {
            warn!("Failed to scan plugins: {}", e);
        }

        Ok(state)
    }

    /// 把内置插件复制到插件目录（版本变化或已安装副本损坏时覆盖更新）
    fn seed_builtin_plugins(&self, app: &AppHandle) {
        let Some(source) = builtin_plugins_dir(app) else {
            return;
        };

        let entries = match std::fs::read_dir(&source) {
            Ok(entries) => entries,
            Err(e) => {
                warn!("Failed to read builtin plugins dir {:?}: {}", source, e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Ok(plugin) = load_plugin_from_dir(&path) else {
                continue;
            };
            let target = self.plugins_dir.join(&plugin.id);
            if !should_reseed(&target, &plugin.version) {
                continue;
            }
            if target.exists() {
                if let Err(e) = std::fs::remove_dir_all(&target) {
                    warn!("Failed to remove outdated builtin plugin {}: {}", plugin.id, e);
                    continue;
                }
            }
            match copy_dir_all(&path, &target) {
                Ok(()) => info!("Seeded builtin plugin: {} ({})", plugin.name, plugin.id),
                Err(e) => warn!("Failed to seed builtin plugin {}: {}", plugin.id, e),
            }
        }
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
                    // `mcp` 是 MCP 工具命名空间（mcp__ 前缀）的保留 id，冲突时只告警不阻断
                    if plugin.id == "mcp" {
                        warn!(
                            "Plugin id \"mcp\" is reserved for the MCP tool namespace; \
                             its tools may shadow or be shadowed by MCP tools"
                        );
                    }
                    info!("Loaded plugin: {} ({})", plugin.name, plugin.id);
                    plugins.insert(plugin.id.clone(), plugin);
                }
            }
        }

        info!("Loaded {} plugins", plugins.len());
        Ok(())
    }

    /// 启动插件目录热重载监听（须在 manage 之后调用，回调里经 app.state 取回自身）。
    /// 文件变化后防抖 500ms 重扫插件并广播 plugins-changed，前端据此重载打开中的插件视图。
    /// 监听启动失败只告警不阻断（热重载为体验优化，不是硬依赖）
    pub fn start_hot_reload(&self, app: &AppHandle) {
        let dir = self.plugins_dir.clone();
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to create plugin watcher: {}", e);
                return;
            }
        };
        if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
            warn!("Failed to watch plugins dir {:?}: {}", dir, e);
            return;
        }

        // 防抖线程：首个事件到达后，等 500ms 静默窗口再重扫 + 广播。
        // watcher drop 时发送端断开，recv 报错退出循环，线程自然结束
        let app_handle = app.clone();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(Duration::from_millis(500)).is_ok() {}
                let state = app_handle.state::<PluginState>();
                match state.scan_plugins() {
                    Ok(()) => {
                        info!("Plugins reloaded after fs change");
                        let _ = app_handle.emit("plugins-changed", ());
                    }
                    Err(e) => warn!("Rescan plugins after fs change failed: {}", e),
                }
            }
        });

        *self._watcher.lock().unwrap() = Some(watcher);
    }

    /// 测试用构造：不启动 watcher，plugins_dir 为空路径
    #[cfg(test)]
    pub(crate) fn for_test(plugins: Vec<Plugin>) -> Self {
        Self {
            plugins: Mutex::new(plugins.into_iter().map(|p| (p.id.clone(), p)).collect()),
            plugins_dir: PathBuf::new(),
            _watcher: Mutex::new(None),
        }
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

/// 内置插件源目录：生产包读资源目录，开发模式读仓库内 plugins/
fn builtin_plugins_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(dir) = app.path().resource_dir() {
        let bundled = dir.join("plugins");
        if bundled.exists() {
            return Some(bundled);
        }
    }

    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins");
    if dev.exists() {
        Some(dev)
    } else {
        None
    }
}

/// 判断内置插件是否需要（重新）播种：
/// 目标目录不存在、已安装副本损坏、或已安装版本与内置版本不一致时返回 true
fn should_reseed(target: &Path, bundled_version: &str) -> bool {
    if !target.exists() {
        return true;
    }
    match load_plugin_from_dir(&target.to_path_buf()) {
        Ok(installed) => installed.version != bundled_version,
        Err(_) => true,
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

    // 校验命令贡献点：mode 只允许 "run" 或 "list"
    for command in &plugin.contributes.commands {
        if command.mode != "run" && command.mode != "list" {
            return Err(VoloError::Plugin(format!(
                "Command '{}' mode must be \"run\" or \"list\", got \"{}\"",
                command.id, command.mode
            )));
        }
    }

    // 校验工具贡献点：parameters 必须是 object 类型 schema，run 必须非空
    for tool in &plugin.contributes.tools {
        if tool.id.is_empty() {
            return Err(VoloError::Plugin("Tool id is required".to_string()));
        }
        if tool.name.is_empty() {
            return Err(VoloError::Plugin(format!(
                "Tool '{}' name is required",
                tool.id
            )));
        }
        if tool.run.trim().is_empty() {
            return Err(VoloError::Plugin(format!(
                "Tool '{}' run is required",
                tool.id
            )));
        }
        if tool.parameters.get("type").and_then(serde_json::Value::as_str) != Some("object") {
            return Err(VoloError::Plugin(format!(
                "Tool '{}' parameters must be an object-type JSON Schema",
                tool.id
            )));
        }
    }

    Ok(plugin)
}

/// 递归复制目录
pub(crate) fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> Result<()> {
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
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_plugin_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("volo_plugin_test_{}_{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_manifest_v2_with_commands() {
        let dir = temp_plugin_dir("manifest_v2");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "uuid-gen",
                "name": "UUID Generator",
                "version": "1.0.0",
                "manifestVersion": 2,
                "contributes": {
                    "commands": [
                        {
                            "id": "gen-uuid",
                            "name": "Generate UUID",
                            "keywords": ["uuid"],
                            "description": "Generate a UUID and copy it",
                            "run": "command.js",
                            "icon": "icon.png"
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let plugin = load_plugin_from_dir(&dir).unwrap();
        assert_eq!(plugin.id, "uuid-gen");
        // manifestVersion 字段被忽略
        assert_eq!(plugin.contributes.commands.len(), 1);
        let cmd = &plugin.contributes.commands[0];
        assert_eq!(cmd.id, "gen-uuid");
        assert_eq!(cmd.name, "Generate UUID");
        assert_eq!(cmd.keywords, vec!["uuid"]);
        assert_eq!(cmd.description.as_deref(), Some("Generate a UUID and copy it"));
        assert_eq!(cmd.run, "command.js");
        assert_eq!(cmd.icon.as_deref(), Some("icon.png"));
        // manifest 缺省 mode 时默认为 "run"
        assert_eq!(cmd.mode, "run");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_command_mode_list() {
        let dir = temp_plugin_dir("command_mode_list");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "file-explorer",
                "name": "File Explorer",
                "version": "1.0.0",
                "contributes": {
                    "commands": [
                        {
                            "id": "browse",
                            "name": "Browse Files",
                            "run": "list.js",
                            "mode": "list"
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let plugin = load_plugin_from_dir(&dir).unwrap();
        assert_eq!(plugin.contributes.commands[0].mode, "list");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_command_invalid_mode_rejected() {
        let dir = temp_plugin_dir("command_bad_mode");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "bad-plugin",
                "name": "Bad Plugin",
                "version": "1.0.0",
                "contributes": {
                    "commands": [
                        { "id": "bad", "name": "Bad", "run": "cmd.js", "mode": "view" }
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = load_plugin_from_dir(&dir);
        assert!(matches!(result, Err(VoloError::Plugin(_))));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_should_reseed() {
        // 目标目录不存在 → 需要播种
        let missing = std::env::temp_dir().join(format!("volo_reseed_missing_{}", uuid::Uuid::new_v4()));
        assert!(should_reseed(&missing, "1.0.0"));

        let dir = temp_plugin_dir("reseed");

        // 已安装副本损坏（无 plugin.json）→ 需要重播
        assert!(should_reseed(&dir, "1.0.0"));

        // 版本一致 → 跳过
        std::fs::write(
            dir.join("plugin.json"),
            r#"{ "id": "p", "name": "P", "version": "1.1.0" }"#,
        )
        .unwrap();
        assert!(!should_reseed(&dir, "1.1.0"));

        // 版本不一致（内置升级）→ 覆盖重播
        assert!(should_reseed(&dir, "1.2.0"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_v1_manifest_compatible() {
        let dir = temp_plugin_dir("manifest_v1");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "hello-world",
                "name": "Hello World",
                "version": "1.0.0",
                "main": "index.html",
                "features": [
                    { "id": "hello", "name": "Hello", "keywords": ["hi"] }
                ]
            }"#,
        )
        .unwrap();

        let plugin = load_plugin_from_dir(&dir).unwrap();
        assert_eq!(plugin.id, "hello-world");
        assert_eq!(plugin.features.len(), 1);
        // v1 插件无 contributes 字段，默认为空
        assert!(plugin.contributes.commands.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_command_missing_run_fails() {
        let dir = temp_plugin_dir("missing_run");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "bad-plugin",
                "name": "Bad Plugin",
                "version": "1.0.0",
                "contributes": {
                    "commands": [
                        { "id": "no-run", "name": "No Run" }
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = load_plugin_from_dir(&dir);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VoloError::Json(_)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_manifest_v2_with_tools() {
        let dir = temp_plugin_dir("manifest_v2_tools");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "uuid-gen",
                "name": "UUID Generator",
                "version": "1.0.0",
                "manifestVersion": 2,
                "contributes": {
                    "tools": [
                        {
                            "id": "gen_uuid",
                            "name": "生成 UUID",
                            "description": "生成指定数量的 UUID v4",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "count": { "type": "integer", "description": "数量，默认 1" }
                                }
                            },
                            "run": "tool.js",
                            "icon": "icon.png"
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let plugin = load_plugin_from_dir(&dir).unwrap();
        assert_eq!(plugin.contributes.tools.len(), 1);
        let tool = &plugin.contributes.tools[0];
        assert_eq!(tool.id, "gen_uuid");
        assert_eq!(tool.name, "生成 UUID");
        assert_eq!(tool.description.as_deref(), Some("生成指定数量的 UUID v4"));
        assert_eq!(tool.parameters["type"], "object");
        assert!(tool.parameters["properties"]["count"].is_object());
        assert_eq!(tool.run, "tool.js");
        assert_eq!(tool.icon.as_deref(), Some("icon.png"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_tool_parameters_default_filled() {
        let dir = temp_plugin_dir("tool_default_schema");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "p1",
                "name": "P1",
                "version": "1.0.0",
                "contributes": {
                    "tools": [
                        { "id": "noop", "name": "Noop", "run": "tool.js" }
                    ]
                }
            }"#,
        )
        .unwrap();

        let plugin = load_plugin_from_dir(&dir).unwrap();
        let tool = &plugin.contributes.tools[0];
        // 缺省 parameters 补默认空 object schema
        assert_eq!(
            tool.parameters,
            serde_json::json!({ "type": "object", "properties": {} })
        );
        assert!(tool.description.is_none());
        assert!(tool.icon.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_tool_invalid_parameters_rejected() {
        let dir = temp_plugin_dir("tool_bad_schema");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "bad-plugin",
                "name": "Bad Plugin",
                "version": "1.0.0",
                "contributes": {
                    "tools": [
                        {
                            "id": "bad",
                            "name": "Bad",
                            "parameters": { "type": "string" },
                            "run": "tool.js"
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = load_plugin_from_dir(&dir);
        assert!(matches!(result, Err(VoloError::Plugin(_))));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_tool_empty_run_rejected() {
        let dir = temp_plugin_dir("tool_empty_run");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "bad-plugin",
                "name": "Bad Plugin",
                "version": "1.0.0",
                "contributes": {
                    "tools": [
                        { "id": "bad", "name": "Bad", "run": "  " }
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = load_plugin_from_dir(&dir);
        assert!(matches!(result, Err(VoloError::Plugin(_))));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_tool_missing_run_fails() {
        let dir = temp_plugin_dir("tool_missing_run");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "bad-plugin",
                "name": "Bad Plugin",
                "version": "1.0.0",
                "contributes": {
                    "tools": [
                        { "id": "no-run", "name": "No Run" }
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = load_plugin_from_dir(&dir);
        assert!(matches!(result, Err(VoloError::Json(_))));

        std::fs::remove_dir_all(&dir).ok();
    }
}
