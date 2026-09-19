//! 后台 Workflow 的工具执行器。
//!
//! 与前台 Agent/Workflow 共享 ToolRegistry 与 MCP Registry，但权限语义固定为非交互：
//! - builtin Tool 走 `ToolRegistry::execute_as_background`；
//! - MCP Tool 走 `enforce_background` 后调用 MCP；
//! - plugin Tool 走嵌入式 QuickJS headless runtime；v1 支持纯 JS Tool，宿主 API 暂时明确拒绝。

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use tauri::AppHandle;

use crate::core::permission::{enforce_background, PermissionEngine};
use crate::error::Result;

use super::agent::ToolExecutor;
use super::mcp::{McpRegistry, MCP_NAME_PREFIX};
use super::plugin_headless;
use super::plugin_tools::PLUGIN_NAME_PREFIX;
use super::tools::ToolRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundToolRoute {
    Mcp,
    PluginHeadless,
    Builtin,
}

fn background_tool_route(name: &str) -> BackgroundToolRoute {
    if name.starts_with(MCP_NAME_PREFIX) {
        BackgroundToolRoute::Mcp
    } else if name.starts_with(PLUGIN_NAME_PREFIX) {
        BackgroundToolRoute::PluginHeadless
    } else {
        BackgroundToolRoute::Builtin
    }
}

/// Scheduler / 后台 Automation 使用的无交互 ToolExecutor。
///
/// `principal` 仍使用 `workflow:<workflow-id>`，因此后台任务不会获得独立于前台 Workflow
/// 的授权身份；用户在前台为该 Workflow 授予 Always 后，后台才能使用 Medium+ 能力。
pub struct BackgroundToolExecutor<'a> {
    pub app: &'a AppHandle,
    pub engine: &'a PermissionEngine,
    pub mcp: &'a McpRegistry,
    pub principal: &'a str,
}

impl ToolExecutor for BackgroundToolExecutor<'_> {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            match background_tool_route(name) {
                BackgroundToolRoute::Mcp => {
                    // capability 包含具体 MCP tool 名，Always grant 只放行这一项工具。
                    let capability = format!("mcp.call:{}", name);
                    enforce_background(self.engine, self.principal, &capability, Some(name))?;
                    self.mcp.call(name, args).await
                }
                BackgroundToolRoute::PluginHeadless => {
                    plugin_headless::execute_plugin_tool(self.app, self.principal, name, args).await
                }
                BackgroundToolRoute::Builtin => {
                    ToolRegistry::execute_as_background(
                        self.app,
                        self.engine,
                        self.principal,
                        name,
                        &args,
                    )
                    .await
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_routes_mcp_without_renderer_bridge() {
        assert_eq!(
            background_tool_route("mcp__server__tool"),
            BackgroundToolRoute::Mcp
        );
    }

    #[test]
    fn background_routes_plugin_tool_to_headless_runtime() {
        assert_eq!(
            background_tool_route("plugin__demo__tool__hash"),
            BackgroundToolRoute::PluginHeadless
        );
    }

    #[test]
    fn background_routes_regular_names_to_builtin_registry() {
        assert_eq!(
            background_tool_route("clipboard_read"),
            BackgroundToolRoute::Builtin
        );
        assert_eq!(
            background_tool_route("notification_show"),
            BackgroundToolRoute::Builtin
        );
    }
}
