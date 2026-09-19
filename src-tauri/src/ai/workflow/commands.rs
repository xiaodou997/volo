use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::ai::background_tools::BackgroundToolExecutor;
use crate::ai::llm::OpenAiBackend;
use crate::ai::mcp::McpRegistry;
use crate::ai::plugin_tools::{AgentToolExecutor, PluginToolState};
use crate::ai::tool_executor::ToolExecutor;
use crate::core::config::Config;
use crate::core::permission::PermissionEngine;
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

use super::{
    execute_workflow, validate_workflow, Workflow, WorkflowExecution, WorkflowStep,
    WorkflowToolRunner,
};

#[path = "history.rs"]
pub(crate) mod history;
#[path = "storage.rs"]
pub(crate) mod storage;

const BACKGROUND_AI_STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// Workflow 在 PermissionEngine 中使用独立 principal，避免复用 Agent 的持久授权。
pub(crate) fn workflow_principal(workflow_id: &str) -> String {
    format!("workflow:{}", workflow_id)
}

fn llm_backend_for(app: &AppHandle, workflow: &Workflow) -> Result<Option<OpenAiBackend>> {
    let has_ai_step = workflow
        .steps
        .iter()
        .any(|step| matches!(step, WorkflowStep::Ai { .. }));
    if !has_ai_step {
        return Ok(None);
    }

    let llm = app.state::<Config>().get().llm;
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
    Ok(Some(OpenAiBackend::new(
        llm.base_url,
        llm.model,
        llm.api_key,
    )))
}

async fn execute_and_record(
    app: &AppHandle,
    workflow: &Workflow,
    input: Value,
    executor: &dyn ToolExecutor,
    llm_backend: Option<&OpenAiBackend>,
    run_context: history::WorkflowRunContext,
    ai_timeout: Option<Duration>,
) -> Result<WorkflowExecution> {
    let started_at = Utc::now();
    let started = Instant::now();
    let runner = match (llm_backend, ai_timeout) {
        (Some(backend), Some(timeout)) => {
            WorkflowToolRunner::with_backend_timeout(executor, backend, timeout)
        }
        (Some(backend), None) => WorkflowToolRunner::with_backend(executor, backend),
        (None, _) => WorkflowToolRunner::new(executor),
    };

    let execution = execute_workflow(workflow, input, &runner).await?;
    let finished_at = Utc::now();
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let record = history::build_record_with_context(
        workflow,
        &execution,
        &run_context,
        started_at,
        finished_at,
        duration_ms,
    );

    // Audit 属于旁路能力：不能因为磁盘/目录问题把已经完成的 Workflow 改判为失败。
    let audit_result =
        history::workflow_runs_dir(app).and_then(|dir| history::record_run(&dir, &record));
    if let Err(error) = audit_result {
        tracing::warn!(
            workflow_id = %workflow.id,
            run_id = %record.id,
            "persist workflow run audit failed: {}",
            error
        );
    }

    Ok(execution)
}

/// Scheduler 按 id 读取最新保存的 Workflow definition。
pub(crate) fn load_saved_workflow(app: &AppHandle, workflow_id: &str) -> Result<Workflow> {
    storage::load_workflow(&storage::workflows_dir(app)?, workflow_id)
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

/// 列出最近的 Workflow 执行审计记录；workflow_id 为空时返回所有 Workflow。
#[tauri::command]
pub fn workflow_list_runs(
    app: AppHandle,
    workflow_id: Option<String>,
) -> Result<Vec<history::WorkflowRunRecord>> {
    history::list_runs(&history::workflow_runs_dir(&app)?, workflow_id.as_deref())
}

/// 前台手动执行一个 Workflow。
#[tauri::command]
pub async fn workflow_run(
    app: AppHandle,
    workflow: Workflow,
    input: Option<Value>,
) -> Result<WorkflowExecution> {
    validate_workflow(&workflow)?;
    let llm_backend = llm_backend_for(&app, &workflow)?;
    let app_config = app.state::<Config>().get();
    let principal = workflow_principal(&workflow.id);
    let mcp = app.state::<McpRegistry>();

    // 与 Agent 相同：MCP 连接按配置幂等建立，单 server 失败由 Registry warn + skip。
    mcp.connect_all(&app_config.mcp_servers).await;

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

    execute_and_record(
        &app,
        &workflow,
        input.unwrap_or(Value::Null),
        &executor,
        llm_backend.as_ref(),
        history::WorkflowRunContext::manual(),
        None,
    )
    .await
}

/// Scheduler / Automation 的后台执行入口。
///
/// 与手动 Workflow 共用 LLM、MCP、step runner 与 History；仅 ToolExecutor 切换为
/// non-interactive background 模式，因此不会 emit 权限审批或 renderer plugin-tool-call。
pub(crate) async fn run_workflow_background(
    app: AppHandle,
    workflow: Workflow,
    input: Option<Value>,
    run_context: history::WorkflowRunContext,
) -> Result<WorkflowExecution> {
    validate_workflow(&workflow)?;
    let llm_backend = llm_backend_for(&app, &workflow)?;
    let app_config = app.state::<Config>().get();
    let principal = workflow_principal(&workflow.id);
    let mcp = app.state::<McpRegistry>();
    mcp.connect_all(&app_config.mcp_servers).await;

    let engine = app.state::<PermissionEngine>();
    let executor = BackgroundToolExecutor {
        app: &app,
        engine: &engine,
        mcp: &mcp,
        principal: &principal,
    };

    execute_and_record(
        &app,
        &workflow,
        input.unwrap_or(Value::Null),
        &executor,
        llm_backend.as_ref(),
        run_context,
        Some(BACKGROUND_AI_STEP_TIMEOUT),
    )
    .await
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
