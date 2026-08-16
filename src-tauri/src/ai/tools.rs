//! Tool Registry
//! Agent 可调用的内置工具：每个工具映射一个 capability，执行前过权限引擎

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};

/// Agent 调工具时在权限引擎中的身份
pub const AGENT_PRINCIPAL: &str = "agent:builtin";

/// fs_read 返回内容的最大长度（字节），超出截断并标注
const FS_READ_MAX_BYTES: usize = 4096;

/// 工具描述（parameters 为 JSON Schema）
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

/// 内置工具注册表
pub struct ToolRegistry;

impl ToolRegistry {
    /// 全部内置工具的规格
    pub fn specs() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "clipboard_read",
                description: "读取系统剪贴板的文本内容",
                parameters: json!({
                    "type": "object",
                    "properties": {},
                }),
            },
            ToolSpec {
                name: "fs_read",
                description: "读取本地文本文件的内容（超长会截断）",
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
                name: "notification_show",
                description: "发送一条系统通知",
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
        ]
    }

    /// 工具名对应的 capability；未知名返回 None
    pub fn capability_of(name: &str) -> Option<&'static str> {
        match name {
            "clipboard_read" => Some("clipboard.read"),
            "fs_read" => Some("fs.read"),
            "notification_show" => Some("notification.show"),
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

        let resource = match name {
            "fs_read" => args.get("path").and_then(Value::as_str),
            _ => None,
        };

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
                let content = std::fs::read_to_string(path)?;
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
            _ => unreachable!("capability_of 已过滤未知工具"),
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
    fn test_specs_are_well_formed() {
        let specs = ToolRegistry::specs();
        assert_eq!(specs.len(), 3);

        for spec in &specs {
            assert!(!spec.name.is_empty());
            assert!(!spec.description.is_empty());
            assert_eq!(spec.parameters["type"], "object");
            assert!(spec.parameters.get("properties").is_some());
            // 每个工具都映射到已注册的 capability
            assert!(ToolRegistry::capability_of(spec.name).is_some());
        }

        assert_eq!(
            ToolRegistry::capability_of("fs_read"),
            Some("fs.read")
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
