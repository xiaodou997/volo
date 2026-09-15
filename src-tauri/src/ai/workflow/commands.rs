use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::ai::llm::OpenAiBackend;
use crate::ai::mcp::McpRegistry;
use crate::ai::plugin_tools::{AgentToolExecutor, PluginToolState};
use crate::core::config::Config;
use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

use super::{
    execute_workflow, validate_workflow, Workflow, WorkflowExecution, WorkflowStep,
    WorkflowToolRunner,
};

#[path = "storage.rs"]
mod storage;

/// Workflow 在 PermissionEngine 中使用独立 principal，避免复用 Agent 的持久授权。
pub(crate) fn workflow_principal(workflow_id: &str) -> String {
    format!("workflow:{}", workflow_id)
}

/// 列出已持久化的 Workflow 定义。
#[tauri::command]
pub fn workflow_list(app: AppHandle) -> Result<Vec<Workflow>> {
    storage::list_workflows(&storage::workflows_dir(&app)?)
}

/// 保存或覆盖一个 Workflow 定义。
#[tauri::command]
pub fn workflow_save(app: AppHandle, workflow: Workflow) -> Result<()> {
    storage::save_workflow(&storage::workflows_dir(&app)?, &workflow)
}

/// 删除一个已持久化的 Workflow 定义。
#[tauri::command]
pub fn workflow_delete(app: AppHandle, workflow_id: String) -> Result<()> {
    storage::delete_workflow(&storage::workflows_dir(&app)?, &workflow_id)
}

/// 前台手动执行一个 Workflow。
///
/// v1.11 支持 Tool 与单轮 AI step；AI step 不开启内部 tool loop。
/// 不包含后台执行、trigger、scheduler 或 retry。
#[tauri::command]
pub async fn workflow_run(
    app: AppHandle,
    workflow: Workflow,
    input: Option<Value>,
) -> Result<WorkflowExecution> {
    // 先验证，避免无效定义触发 MCP 连接或权限流程。
    validate_workflow(&workflow)?;

    let has_ai_step = workflow
        .steps
        .iter()
        .any(|step| matches!(step, WorkflowStep::Ai { .. }));
    let app_config = app.state::<Config>().get();
    let mcp_servers = app_config.mcp_servers;
    let llm = app_config.llm;
    let llm_backend = if has_ai_step {
        if llm.model.trim().is_empty() {
            return Err(VoloError::Other(
                "Workflow 包含 AI step，请先配置 LLM 模型".to_string(),
            ));
        }
        if llm.api_key.trim().is_empty() {
            return Err(VoloError::Other(
                "Workflow 包含 AI step，请先配置 LLM API Key".to_string(),
            ));
        }
        Some(OpenAiBackend::new(llm.base_url, llm.model, llm.api_key))
    } else {
        None
    };

    let principal = workflow_principal(&workflow.id);
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
    let runner = match llm_backend.as_ref() {
        Some(backend) => WorkflowToolRunner::with_backend(&executor, backend),
        None => WorkflowToolRunner::new(&executor),
    };

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
