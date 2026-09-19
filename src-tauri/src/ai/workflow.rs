//! Workflow Core
//!
//! v1.11 的第一版只提供前台、确定性、顺序执行语义：
//! - step 按定义顺序逐个执行
//! - 首个失败立即终止（fail-fast）
//! - 不包含 trigger / scheduler / retry / background daemon / DAG
//! - 具体 Tool / AI 执行通过 `WorkflowStepRunner` 注入，核心不依赖 ToolRegistry 或 LLM

pub(crate) mod commands;
mod tool_runner;
pub use commands::workflow_run;
pub use tool_runner::WorkflowToolRunner;

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Result, VoloError};

/// 可持久化的 Workflow 定义。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

/// v1.11 预留两类 step：确定性 Tool 与 AI。
///
/// Workflow Core 本身不解释这两种 step，实际执行由 `WorkflowStepRunner` 决定。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowStep {
    Tool {
        id: String,
        name: String,
        #[serde(default = "default_tool_args")]
        args: Value,
    },
    Ai {
        id: String,
        prompt: String,
    },
}

impl WorkflowStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Tool { id, .. } | Self::Ai { id, .. } => id,
        }
    }
}

fn default_tool_args() -> Value {
    json!({})
}

/// 顺序执行过程中提供给 step runner 的只读上下文。
///
/// `previous_output` 支持下一阶段的“使用上一步结果”；`outputs` 则允许按 step id
/// 读取更早步骤结果。第一版只提供数据，不内置模板替换语法。
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowContext {
    input: Value,
    previous_output: Option<Value>,
    outputs: BTreeMap<String, Value>,
}

impl WorkflowContext {
    fn new(input: Value) -> Self {
        Self {
            input,
            previous_output: None,
            outputs: BTreeMap::new(),
        }
    }

    pub fn input(&self) -> &Value {
        &self.input
    }

    pub fn previous_output(&self) -> Option<&Value> {
        self.previous_output.as_ref()
    }

    pub fn output(&self, step_id: &str) -> Option<&Value> {
        self.outputs.get(step_id)
    }

    pub fn outputs(&self) -> &BTreeMap<String, Value> {
        &self.outputs
    }

    fn record(&mut self, step_id: &str, output: Value) {
        self.previous_output = Some(output.clone());
        self.outputs.insert(step_id.to_string(), output);
    }
}

/// Workflow step 的执行适配器。
///
/// 下一阶段会提供生产实现，把 Tool step 接到现有 builtin / plugin / MCP 工具链；
/// AI step 则后续再接 LLM。测试可以注入纯 mock runner。
pub trait WorkflowStepRunner: Send + Sync {
    fn run<'a>(
        &'a self,
        step: &'a WorkflowStep,
        context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowExecutionStatus {
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Completed,
    Failed,
}

/// 单个 step 的执行结果。失败 step 记录 error，成功 step 记录 output。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStepExecution {
    pub step_id: String,
    pub status: WorkflowStepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 一次前台 Workflow 执行结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowExecution {
    pub workflow_id: String,
    pub status: WorkflowExecutionStatus,
    pub steps: Vec<WorkflowStepExecution>,
    /// 仅完整成功时暴露最后一步输出；失败时为 None，避免把部分结果误认为最终结果。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 验证 Workflow 的最小结构约束。
pub fn validate_workflow(workflow: &Workflow) -> Result<()> {
    if workflow.id.trim().is_empty() {
        return Err(VoloError::Other("workflow id 不能为空".to_string()));
    }
    if workflow.name.trim().is_empty() {
        return Err(VoloError::Other("workflow name 不能为空".to_string()));
    }
    if workflow.steps.is_empty() {
        return Err(VoloError::Other("workflow 至少需要一个 step".to_string()));
    }

    let mut ids = HashSet::new();
    for step in &workflow.steps {
        let id = step.id();
        if id.trim().is_empty() {
            return Err(VoloError::Other("workflow step id 不能为空".to_string()));
        }
        if id.trim() != id {
            return Err(VoloError::Other(format!(
                "workflow step id 不能包含首尾空白: {}",
                id
            )));
        }
        if !ids.insert(id) {
            return Err(VoloError::Other(format!("workflow step id 重复: {}", id)));
        }

        match step {
            WorkflowStep::Tool { name, .. } if name.trim().is_empty() => {
                return Err(VoloError::Other(format!(
                    "workflow tool step {} 缺少 tool name",
                    id
                )));
            }
            WorkflowStep::Ai { prompt, .. } if prompt.trim().is_empty() => {
                return Err(VoloError::Other(format!(
                    "workflow ai step {} 缺少 prompt",
                    id
                )));
            }
            _ => {}
        }
    }

    Ok(())
}

/// 按定义顺序执行 Workflow；首个 step 失败后立即终止。
///
/// 结构校验失败返回 `Err`；step 的运行时失败则返回 `Ok(WorkflowExecution)`，其中
/// status=failed，并保留此前成功步骤以及失败步骤的信息，方便后续 UI 展示 timeline。
pub async fn execute_workflow(
    workflow: &Workflow,
    input: Value,
    runner: &dyn WorkflowStepRunner,
) -> Result<WorkflowExecution> {
    validate_workflow(workflow)?;

    let mut context = WorkflowContext::new(input);
    let mut steps = Vec::with_capacity(workflow.steps.len());

    for step in &workflow.steps {
        match runner.run(step, &context).await {
            Ok(output) => {
                context.record(step.id(), output.clone());
                steps.push(WorkflowStepExecution {
                    step_id: step.id().to_string(),
                    status: WorkflowStepStatus::Completed,
                    output: Some(output),
                    error: None,
                });
            }
            Err(error) => {
                let error = error.to_string();
                steps.push(WorkflowStepExecution {
                    step_id: step.id().to_string(),
                    status: WorkflowStepStatus::Failed,
                    output: None,
                    error: Some(error.clone()),
                });
                return Ok(WorkflowExecution {
                    workflow_id: workflow.id.clone(),
                    status: WorkflowExecutionStatus::Failed,
                    steps,
                    output: None,
                    error: Some(error),
                });
            }
        }
    }

    Ok(WorkflowExecution {
        workflow_id: workflow.id.clone(),
        status: WorkflowExecutionStatus::Completed,
        steps,
        output: context.previous_output().cloned(),
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq)]
    struct SeenStep {
        id: String,
        input: Value,
        previous: Option<Value>,
        completed_ids: Vec<String>,
    }

    struct RecordingRunner {
        seen: Mutex<Vec<SeenStep>>,
        fail_on: Option<String>,
    }

    impl RecordingRunner {
        fn new(fail_on: Option<&str>) -> Self {
            Self {
                seen: Mutex::new(Vec::new()),
                fail_on: fail_on.map(str::to_string),
            }
        }
    }

    impl WorkflowStepRunner for RecordingRunner {
        fn run<'a>(
            &'a self,
            step: &'a WorkflowStep,
            context: &'a WorkflowContext,
        ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
            Box::pin(async move {
                self.seen.lock().unwrap().push(SeenStep {
                    id: step.id().to_string(),
                    input: context.input().clone(),
                    previous: context.previous_output().cloned(),
                    completed_ids: context.outputs().keys().cloned().collect(),
                });

                if self.fail_on.as_deref() == Some(step.id()) {
                    return Err(VoloError::Other(format!("{} failed", step.id())));
                }
                Ok(Value::String(step.id().to_string()))
            })
        }
    }

    fn tool_step(id: &str) -> WorkflowStep {
        WorkflowStep::Tool {
            id: id.to_string(),
            name: "clipboard_read".to_string(),
            args: json!({}),
        }
    }

    fn workflow(step_ids: &[&str]) -> Workflow {
        Workflow {
            id: "wf-test".to_string(),
            name: "Test Workflow".to_string(),
            steps: step_ids.iter().map(|id| tool_step(id)).collect(),
        }
    }

    #[test]
    fn workflow_definition_round_trips_and_defaults_tool_args() {
        let parsed: Workflow = serde_json::from_value(json!({
            "id": "demo",
            "name": "Demo",
            "steps": [
                { "type": "tool", "id": "read", "name": "clipboard_read" },
                { "type": "ai", "id": "summary", "prompt": "总结上一步内容" }
            ]
        }))
        .unwrap();

        match &parsed.steps[0] {
            WorkflowStep::Tool { args, .. } => assert_eq!(args, &json!({})),
            _ => panic!("expected tool step"),
        }
        let encoded = serde_json::to_value(&parsed).unwrap();
        assert_eq!(encoded["steps"][0]["type"], "tool");
        assert_eq!(encoded["steps"][1]["type"], "ai");
    }

    #[test]
    fn validation_rejects_empty_and_duplicate_steps() {
        let empty = Workflow {
            id: "empty".to_string(),
            name: "Empty".to_string(),
            steps: vec![],
        };
        assert!(validate_workflow(&empty)
            .unwrap_err()
            .to_string()
            .contains("至少需要一个 step"));

        let duplicate = workflow(&["same", "same"]);
        assert!(validate_workflow(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("step id 重复"));
    }

    #[tokio::test]
    async fn executor_runs_steps_in_order_with_context() {
        let workflow = workflow(&["first", "second", "third"]);
        let runner = RecordingRunner::new(None);
        let input = json!({ "source": "manual" });

        let execution = execute_workflow(&workflow, input.clone(), &runner)
            .await
            .unwrap();

        assert_eq!(execution.status, WorkflowExecutionStatus::Completed);
        assert_eq!(execution.steps.len(), 3);
        assert_eq!(execution.output, Some(Value::String("third".to_string())));
        assert!(execution.error.is_none());

        let seen = runner.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].input, input);
        assert_eq!(seen[0].previous, None);
        assert!(seen[0].completed_ids.is_empty());
        assert_eq!(seen[1].previous, Some(Value::String("first".to_string())));
        assert_eq!(seen[1].completed_ids, vec!["first"]);
        assert_eq!(seen[2].previous, Some(Value::String("second".to_string())));
        assert_eq!(seen[2].completed_ids, vec!["first", "second"]);
    }

    #[tokio::test]
    async fn executor_is_fail_fast_and_preserves_partial_timeline() {
        let workflow = workflow(&["first", "broken", "never"]);
        let runner = RecordingRunner::new(Some("broken"));

        let execution = execute_workflow(&workflow, Value::Null, &runner)
            .await
            .unwrap();

        assert_eq!(execution.status, WorkflowExecutionStatus::Failed);
        assert_eq!(execution.steps.len(), 2);
        assert_eq!(execution.steps[0].status, WorkflowStepStatus::Completed);
        assert_eq!(execution.steps[1].status, WorkflowStepStatus::Failed);
        assert!(execution.steps[1]
            .error
            .as_deref()
            .unwrap()
            .contains("broken failed"));
        assert!(execution.output.is_none());
        assert!(execution
            .error
            .as_deref()
            .unwrap()
            .contains("broken failed"));

        let seen = runner.seen.lock().unwrap();
        assert_eq!(
            seen.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["first", "broken"]
        );
    }
}
