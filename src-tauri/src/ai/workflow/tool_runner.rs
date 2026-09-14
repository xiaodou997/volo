use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use crate::ai::llm::{ChatBackend, Message};
use crate::ai::tool_executor::ToolExecutor;
use crate::error::{Result, VoloError};

use super::{WorkflowContext, WorkflowStep, WorkflowStepRunner};

const WORKFLOW_AI_SYSTEM_PROMPT: &str = "你正在执行 Volo Workflow 的一个 AI step。只完成当前 step 指令，并使用提供的 Workflow 上下文。";

/// Workflow step 适配器。
///
/// Tool step 复用统一 ToolExecutor；AI step 可选复用现有 ChatBackend。
/// Tool-only Workflow 可以继续只使用 `new`，不依赖 LLM 配置。
pub struct WorkflowToolRunner<'a> {
    executor: &'a dyn ToolExecutor,
    backend: Option<&'a dyn ChatBackend>,
}

impl<'a> WorkflowToolRunner<'a> {
    pub fn new(executor: &'a dyn ToolExecutor) -> Self {
        Self { executor, backend: None }
    }

    pub fn with_backend(executor: &'a dyn ToolExecutor, backend: &'a dyn ChatBackend) -> Self {
        Self { executor, backend: Some(backend) }
    }

    fn ai_prompt(prompt: &str, context: &WorkflowContext) -> Result<String> {
        let context = serde_json::to_string_pretty(&json!({
            "input": context.input(),
            "previousOutput": context.previous_output(),
            "outputs": context.outputs(),
        }))?;
        Ok(format!("当前 step 指令：\n{}\n\nWorkflow 上下文：\n{}", prompt, context))
    }

    async fn run_ai(&self, prompt: &str, context: &WorkflowContext) -> Result<Value> {
        let backend = self.backend.ok_or_else(|| {
            VoloError::Other("workflow AI step 需要已配置的 LLM backend".to_string())
        })?;
        let messages = vec![
            Message::system(WORKFLOW_AI_SYSTEM_PROMPT),
            Message::user(&Self::ai_prompt(prompt, context)?),
        ];
        let response = backend.chat(&messages, &[]).await?;
        if !response.tool_calls.is_empty() {
            return Err(VoloError::Other("workflow AI step 不接受 tool call 响应".to_string()));
        }
        let content = response
            .content
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| VoloError::Other("workflow AI step 返回了空内容".to_string()))?;
        Ok(Value::String(content))
    }
}

impl WorkflowStepRunner for WorkflowToolRunner<'_> {
    fn run<'a>(
        &'a self,
        step: &'a WorkflowStep,
        context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            match step {
                WorkflowStep::Tool { name, args, .. } => self.executor.execute(name, args.clone()).await,
                WorkflowStep::Ai { prompt, .. } => self.run_ai(prompt, context).await,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::llm::{ChatResponse, ToolCall};
    use crate::ai::tools::ToolSpec;
    use crate::ai::workflow::{execute_workflow, Workflow, WorkflowExecutionStatus};
    use serde_json::json;
    use std::sync::Mutex;

    struct MockToolExecutor { calls: Mutex<Vec<(String, Value)>> }

    impl MockToolExecutor {
        fn new() -> Self { Self { calls: Mutex::new(Vec::new()) } }
    }

    impl ToolExecutor for MockToolExecutor {
        fn execute<'a>(
            &'a self,
            name: &'a str,
            args: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((name.to_string(), args.clone()));
                Ok(json!({ "tool": name, "args": args }))
            })
        }
    }

    struct MockChatBackend {
        calls: Mutex<Vec<(Vec<String>, usize)>>,
        return_tool_call: bool,
    }

    impl MockChatBackend {
        fn new() -> Self { Self { calls: Mutex::new(Vec::new()), return_tool_call: false } }
        fn with_tool_call() -> Self { Self { calls: Mutex::new(Vec::new()), return_tool_call: true } }
    }

    impl ChatBackend for MockChatBackend {
        fn chat<'a>(
            &'a self,
            messages: &'a [Message],
            tools: &'a [ToolSpec],
        ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((
                    messages.iter().map(|m| m.content.clone().unwrap_or_default()).collect(),
                    tools.len(),
                ));
                Ok(ChatResponse {
                    content: Some("这是摘要".to_string()),
                    tool_calls: if self.return_tool_call {
                        vec![ToolCall { id: "call-1".into(), name: "clipboard_read".into(), arguments: json!({}) }]
                    } else {
                        vec![]
                    },
                })
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
        let execution = execute_workflow(&workflow, Value::Null, &runner).await.unwrap();
        assert_eq!(execution.status, WorkflowExecutionStatus::Completed);
        assert_eq!(executor.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ai_step_receives_context_without_tools() {
        let executor = MockToolExecutor::new();
        let backend = MockChatBackend::new();
        let runner = WorkflowToolRunner::with_backend(&executor, &backend);
        let workflow = Workflow {
            id: "mixed-demo".to_string(),
            name: "Mixed Demo".to_string(),
            steps: vec![
                WorkflowStep::Tool {
                    id: "read".to_string(),
                    name: "clipboard_read".to_string(),
                    args: json!({}),
                },
                WorkflowStep::Ai {
                    id: "summary".to_string(),
                    prompt: "总结上一步结果".to_string(),
                },
            ],
        };
        let execution = execute_workflow(&workflow, json!({ "source": "manual" }), &runner).await.unwrap();
        assert_eq!(execution.status, WorkflowExecutionStatus::Completed);
        assert_eq!(execution.output, Some(Value::String("这是摘要".to_string())));
        let calls = backend.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, 0);
        assert!(calls[0].0[1].contains("总结上一步结果"));
        assert!(calls[0].0[1].contains("\"source\": \"manual\""));
        assert!(calls[0].0[1].contains("\"read\""));
    }

    #[tokio::test]
    async fn ai_step_rejects_tool_call_response() {
        let executor = MockToolExecutor::new();
        let backend = MockChatBackend::with_tool_call();
        let runner = WorkflowToolRunner::with_backend(&executor, &backend);
        let workflow = Workflow {
            id: "ai-demo".into(),
            name: "AI Demo".into(),
            steps: vec![WorkflowStep::Ai { id: "summary".into(), prompt: "总结".into() }],
        };
        let execution = execute_workflow(&workflow, Value::Null, &runner).await.unwrap();
        assert_eq!(execution.status, WorkflowExecutionStatus::Failed);
        assert!(execution.error.as_deref().unwrap().contains("不接受 tool call 响应"));
    }
}
