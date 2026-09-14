use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::ai::mcp::McpRegistry;
use crate::ai::plugin_tools::{AgentToolExecutor, PluginToolState};
use crate::core::config::Config;
use crate::core::permission::PermissionEngine;
use crate::error::Result;
use crate::plugin::manager::PluginState;

use super::{execute_workflow, validate_workflow, Workflow, WorkflowExecution, WorkflowToolRunner};

/// Workflow 在 PermissionEngine 中使用独立 principal，避免复用 Agent 的持久授权。
pub(crate) fn workflow_principal(workflow_id: &str) -> String {
    format!("workflow:{}", workflow_id)
}

/// 前台手动执行一个 Workflow。
///
/// v1.11 当前只支持 Tool step；AI step 会由 `WorkflowToolRunner` 明确返回失败。
/// 不包含持久化、后台执行、trigger、scheduler 或 retry。
#[tauri::command]
pub async fn workflow_run(
    app: AppHandle,
    workflow: Workflow,
    input: Option<Value>,
) -> Result<WorkflowExecution> {
    // 先验证，避免无效定义触发 MCP 连接或权限流程。
    validate_workflow(&workflow)?;

    let principal = workflow_principal(&workflow.id);
    let mcp_servers = app.state::<Config>().get().mcp_servers;
    let mcp = app.state::<McpRegistry>();

    // 与 Agent 相同：MCP 连接按配置幂等建立，单 server 失败由 Registry warn + skip。
    mcp.connect_all(&mcp_servers).await;

    let engine = app.state::<PermissionEngine>();
    let plugins = app.state::<PluginState>();
    let tool_state = app.state::<PluginToolState>();

    let executor = AgentToolExecutor {
        app: &app,
        engine: &engine,
        plugins: &plugins,
        tool_state: &tool_state,
        mcp: &mcp,
        principal: &principal,
    };
    let runner = WorkflowToolRunner::new(&executor);

    execute_workflow(&workflow, input.unwrap_or(Value::Null), &runner).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_principal_is_stable_and_isolated_from_agent() {
        assert_eq!(workflow_principal("daily-note"), "workflow:daily-note");
        assert_ne!(
            workflow_principal("daily-note"),
            crate::ai::tools::AGENT_PRINCIPAL
        );
        assert_ne!(workflow_principal("a"), workflow_principal("b"));
    }
}
