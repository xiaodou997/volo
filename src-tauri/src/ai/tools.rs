//! Tool Registry
//! Agent 可调用的内置工具：每个工具映射一个 capability，执行前过权限引擎

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};

/// Agent 调工具时在权限引擎中的身份
pub const AGENT_PRINCIPAL: &str = "agent:builtin";

/// fs_read 返回内容的最大长度（字节），超出截断并标注
const FS_READ_MAX_BYTES: usize = 4096;

/// 工具描述（parameters 为 JSON Schema）
///
/// name/description 用 String：内置工具是静态字符串，
/// 插件工具在运行时由 manifest 动态生成（见 ai::plugin_tools）
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 内置工具注册表
pub struct ToolRegistry;

impl ToolRegistry {
    /// 全部内置工具的规格
    pub fn specs() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "clipboard_read".to_string(),
                description: "读取系统剪贴板的文本内容".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                }),
            },
            ToolSpec {
                name: "fs_read".to_string(),
                description: "读取本地文本文件的内容（超长会截断）".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "文件的绝对路径",
                        },
                    },
                    "required": ["path"],
                }),
            },
            ToolSpec {
                name: "notification_show".to_string(),
                description: "发送一条系统通知".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "body": {
                            "type": "string",
                            "description": "通知正文",
                        },
                        "title": {
                            "type": "string",
                            "description": "通知标题（可选）",
                        },
                    },
                    "required": ["body"],
                }),
            },
            ToolSpec {
                name: "fs_write".to_string(),
                description: "将文本内容写入本地文件（覆盖已有内容）".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "文件的绝对路径（支持 ~ 开头的主目录简写）",
                        },
                        "content": {
                            "type": "string",
                            "description": "要写入的文本内容",
                        },
                    },
                    "required": ["path", "content"],
                }),
            },
            ToolSpec {
                name: "shell_open".to_string(),
                description: "用系统默认程序打开 URL 或本地文件/文件夹".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "target": {
                            "type": "string",
                            "description": "http(s) URL 或本地文件/文件夹路径",
                        },
                    },
                    "required": ["target"],
                }),
            },
            ToolSpec {
                name: "clipboard_write".to_string(),
                description: "把文本写入系统剪贴板".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "要写入剪贴板的文本",
                        },
                    },
                    "required": ["text"],
                }),
            },
        ]
    }

    /// 工具名对应的 capability；未知名返回 None
    pub fn capability_of(name: &str) -> Option<&'static str> {
        match name {
            "clipboard_read" => Some("clipboard.read"),
            "fs_read" => Some("fs.read"),
            "notification_show" => Some("notification.show"),
            "fs_write" => Some("fs.write"),
            "shell_open" => Some("shell.open"),
            "clipboard_write" => Some("clipboard.write"),
            _ => None,
        }
    }

    /// 执行工具：先经权限引擎裁决（Medium 风险会弹审批），再执行底层逻辑
    ///
    /// `enforce` 本身不做声明检查（声明检查在插件面 guard `require()` 中），
    /// agent 作为内置 principal 直接进入授权查询/运行时审批流程。
    pub async fn execute(
        app: &AppHandle,
        engine: &PermissionEngine,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        let capability = Self::capability_of(name)
            .ok_or_else(|| VoloError::NotFound(format!("tool: {}", name)))?;

        // 路径类参数先展开 `~` 再进权限管道，审批弹窗与审计日志展示真实路径
        let resource_owned = match name {
            "fs_read" | "fs_write" => args
                .get("path")
                .and_then(Value::as_str)
                .map(Self::expand_tilde),
            "shell_open" => args
                .get("target")
                .and_then(Value::as_str)
                .map(Self::expand_tilde),
            _ => None,
        };
        let resource = resource_owned.as_deref();

        engine
            .enforce(app, AGENT_PRINCIPAL, capability, resource)
            .await?;

        match name {
            "clipboard_read" => {
                let text = app
                    .clipboard()
                    .read_text()
                    .map_err(|e| VoloError::Other(format!("Clipboard read failed: {}", e)))?;
                Ok(Value::String(text))
            }
            "fs_read" => {
                let path = args
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| VoloError::Other("fs_read 缺少必填参数 path".to_string()))?;
                let content = std::fs::read_to_string(Self::expand_tilde(path))?;
                Ok(Value::String(Self::truncate(&content)))
            }
            "notification_show" => {
                let body = args
                    .get("body")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        VoloError::Other("notification_show 缺少必填参数 body".to_string())
                    })?;
                let title = args
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Volo");
                app.notification()
                    .builder()
                    .title(title)
                    .body(body)
                    .show()
                    .map_err(|e| VoloError::Other(format!("Notification failed: {}", e)))?;
                Ok(Value::String("通知已发送".to_string()))
            }
            "fs_write" => {
                let path = args
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| VoloError::Other("fs_write 缺少必填参数 path".to_string()))?;
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        VoloError::Other("fs_write 缺少必填参数 content".to_string())
                    })?;
                std::fs::write(Self::expand_tilde(path), content)?;
                Ok(Value::String(format!("写入 {} 字节", content.len())))
            }
            "shell_open" => {
                let target = args
                    .get("target")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        VoloError::Other("shell_open 缺少必填参数 target".to_string())
                    })?;
                // http(s) 用 open_url，其余按本地路径用 open_path（先展开 `~`）
                if target.starts_with("http://") || target.starts_with("https://") {
                    app.opener()
                        .open_url(target, None::<String>)
                        .map_err(|e| VoloError::Other(format!("打开 URL 失败: {}", e)))?;
                } else {
                    let target = Self::expand_tilde(target);
                    app.opener()
                        .open_path(&target, None::<String>)
                        .map_err(|e| VoloError::Other(format!("打开路径失败: {}", e)))?;
                }
                Ok(Value::String(format!("已打开 {}", target)))
            }
            "clipboard_write" => {
                let text = args
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        VoloError::Other("clipboard_write 缺少必填参数 text".to_string())
                    })?;
                app.clipboard()
                    .write_text(text)
                    .map_err(|e| VoloError::Other(format!("Clipboard write failed: {}", e)))?;
                Ok(Value::String("已写入剪贴板".to_string()))
            }
            _ => unreachable!("capability_of 已过滤未知工具"),
        }
    }

    /// 展开路径开头的 `~` 为用户主目录（LLM 常传字面量 `~/...`，std::fs 不会展开）
    fn expand_tilde(path: &str) -> String {
        let home = if path == "~" {
            Some("")
        } else {
            path.strip_prefix("~/")
        };
        match home {
            Some(rest) => match dirs::home_dir() {
                Some(dir) if rest.is_empty() => dir.to_string_lossy().into_owned(),
                Some(dir) => dir.join(rest).to_string_lossy().into_owned(),
                None => path.to_string(),
            },
            None => path.to_string(),
        }
    }

    /// 超过 FS_READ_MAX_BYTES 时在字符边界截断并附加标注
    fn truncate(content: &str) -> String {
        if content.len() <= FS_READ_MAX_BYTES {
            return content.to_string();
        }
        let mut end = FS_READ_MAX_BYTES;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n...（内容已截断，原文共 {} 字节）",
            &content[..end],
            content.len()
        )
    }
}

/// 以 ToolRegistry 为底层的 ToolExecutor（生产环境用，见 ai::agent）
pub struct RegistryExecutor<'a> {
    pub app: &'a AppHandle,
    pub engine: &'a PermissionEngine,
}

impl crate::ai::agent::ToolExecutor for RegistryExecutor<'_> {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move { ToolRegistry::execute(self.app, self.engine, name, &args).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            ToolRegistry::expand_tilde("~/Desktop/a.txt"),
            home.join("Desktop/a.txt").to_string_lossy()
        );
        assert_eq!(
            ToolRegistry::expand_tilde("~"),
            home.to_string_lossy()
        );
        // 非 ~ 路径原样返回
        assert_eq!(ToolRegistry::expand_tilde("/tmp/a.txt"), "/tmp/a.txt");
        // 中间的 ~ 不展开
        assert_eq!(ToolRegistry::expand_tilde("/tmp/~x"), "/tmp/~x");
    }

    #[test]
    fn test_specs_are_well_formed() {
        let specs = ToolRegistry::specs();
        assert_eq!(specs.len(), 6);

        for spec in &specs {
            assert!(!spec.name.is_empty());
            assert!(!spec.description.is_empty());
            assert_eq!(spec.parameters["type"], "object");
            assert!(spec.parameters.get("properties").is_some());
            // 每个工具都映射到已注册的 capability
            assert!(ToolRegistry::capability_of(&spec.name).is_some());
        }

        assert_eq!(
            ToolRegistry::capability_of("fs_read"),
            Some("fs.read")
        );
        assert_eq!(
            ToolRegistry::capability_of("fs_write"),
            Some("fs.write")
        );
        assert_eq!(
            ToolRegistry::capability_of("shell_open"),
            Some("shell.open")
        );
        assert_eq!(
            ToolRegistry::capability_of("clipboard_write"),
            Some("clipboard.write")
        );
        assert!(ToolRegistry::capability_of("not_a_tool").is_none());
    }

    #[test]
    fn test_fs_read_required_params() {
        let specs = ToolRegistry::specs();
        let fs = specs.iter().find(|s| s.name == "fs_read").unwrap();
        assert_eq!(fs.parameters["required"], json!(["path"]));

        let notif = specs
            .iter()
            .find(|s| s.name == "notification_show")
            .unwrap();
        assert_eq!(notif.parameters["required"], json!(["body"]));
    }

    #[test]
    fn test_new_tools_required_params() {
        let specs = ToolRegistry::specs();
        let fs_write = specs.iter().find(|s| s.name == "fs_write").unwrap();
        assert_eq!(fs_write.parameters["required"], json!(["path", "content"]));

        let shell_open = specs.iter().find(|s| s.name == "shell_open").unwrap();
        assert_eq!(shell_open.parameters["required"], json!(["target"]));

        let clipboard_write = specs
            .iter()
            .find(|s| s.name == "clipboard_write")
            .unwrap();
        assert_eq!(clipboard_write.parameters["required"], json!(["text"]));
    }

    #[test]
    fn test_truncate() {
        let short = "hello";
        assert_eq!(ToolRegistry::truncate(short), "hello");

        // 超长在字符边界截断（含多字节字符）
        let long = "汉".repeat(3000); // 9000 字节
        let out = ToolRegistry::truncate(&long);
        assert!(out.contains("内容已截断"));
        assert!(out.len() < long.len());
    }
}
