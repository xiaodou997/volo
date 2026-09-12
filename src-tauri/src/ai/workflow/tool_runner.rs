use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::error::{Result, VoloError};

use super::{WorkflowContext, WorkflowStep, WorkflowStepRunner};
use crate::ai::tool_executor::ToolExecutor;

/// Workflow Tool step 适配器。
///
/// 只负责把 `WorkflowStep::Tool` 转发给现有统一 ToolExecutor；AI step 在下一阶段实现。
pub struct WorkflowToolRunner<'a> {
    executor: &'a dyn ToolExecutor,
}

impl<'a> WorkflowToolRunner<'a> {
    pub fn new(executor: &'a dyn ToolExecutor) -> Self {
        Self { executor }
    }
}

impl WorkflowStepRunner for WorkflowToolRunner<'_> {
    fn run<'a>(
        &'a self,
        step: &'a WorkflowStep,
        _context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            match step {
                WorkflowStep::Tool { name, args, .. } => {
                    self.executor.execute(name, args.clone()).await
                }
                WorkflowStep::Ai { .. } => Err(VoloError::Other(
                    "workflow AI step 暂未支持".to_string(),
                )),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::workflow::{execute_workflow, Workflow, WorkflowExecutionStatus};
    use serde_json::json;
    use std::sync::Mutex;

    struct MockToolExecutor {
        calls: Mutex<Vec<(String, Value)>>,
    }

    impl MockToolExecutor {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl ToolExecutor for MockToolExecutor {
        fn execute<'a>(
            &'a self,
            name: &'a str,
            args: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
            Box::pin(async move {
                self.calls
                    .lock()
                    .unwrap()
                    .push((name.to_string(), args.clone()));
                Ok(json!({ "tool": name, "args": args }))
            })
        }
    }

    #[tokio::test]
    async fn tool_step_delegates_name_and_args() {
        let executor = MockToolExecutor::new();
        let runner = WorkflowToolRunner::new(&executor);
        let workflow = Workflow {
            id: "tool-demo".to_string(),
            name: "Tool Demo".to_string(),
            steps: vec![WorkflowStep::Tool {
                id: "read".to_string(),
                name: "fs_read".to_string(),
                args: json!({ "path": "/tmp/demo.txt" }),
            }],
        };

        let execution = execute_workflow(&workflow, Value::Null, &runner)
            .await
            .unwrap();

        assert_eq!(execution.status, WorkflowExecutionStatus::Completed);
        assert_eq!(
            execution.output,
            Some(json!({
                "tool": "fs_read",
                "args": { "path": "/tmp/demo.txt" }
            }))
        );
        assert_eq!(
            executor.calls.lock().unwrap().as_slice(),
            &[("fs_read".to_string(), json!({ "path": "/tmp/demo.txt" }))]
        );
    }

    #[tokio::test]
    async fn ai_step_fails_explicitly_without_invoking_tool_executor() {
        let executor = MockToolExecutor::new();
        let runner = WorkflowToolRunner::new(&executor);
        let workflow = Workflow {
            id: "ai-demo".to_string(),
            name: "AI Demo".to_string(),
            steps: vec![WorkflowStep::Ai {
                id: "summarize".to_string(),
                prompt: "总结内容".to_string(),
            }],
        };

        let execution = execute_workflow(&workflow, Value::Null, &runner)
            .await
            .unwrap();

        assert_eq!(execution.status, WorkflowExecutionStatus::Failed);
        assert!(execution
            .error
            .as_deref()
            .unwrap()
            .contains("AI step 暂未支持"));
        assert!(executor.calls.lock().unwrap().is_empty());
    }
}
