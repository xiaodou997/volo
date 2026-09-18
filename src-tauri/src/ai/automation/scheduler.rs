//! Automation 后台调度循环。
//!
//! MVP 语义：
//! - 每 15 秒检查一次 due Automations；
//! - storage 在 claim 时先推进 nextRunAt，避免同一 occurrence 重复执行；
//! - missed occurrences 不补跑；
//! - due jobs 逐个执行，保持简单可预测；
//! - Workflow 执行失败只记录日志/History，不触发即时 retry；
//! - 缺失 Workflow 不会阻断 scheduler，下一周期仍会继续尝试。

use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::AppHandle;

use crate::ai::workflow::commands::{load_saved_workflow, run_workflow_background};
use crate::error::Result;

use super::storage::{automations_dir, claim_due_automations, DueAutomation};

const SCHEDULER_TICK: Duration = Duration::from_secs(15);

/// 在 Tauri runtime 上启动单个 scheduler loop。
pub(crate) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tracing::info!(
            tick_seconds = SCHEDULER_TICK.as_secs(),
            "Automation scheduler started"
        );

        loop {
            let now = Utc::now();
            if let Err(error) = tick(&app, now).await {
                tracing::warn!("automation scheduler tick failed: {}", error);
            }
            tokio::time::sleep(SCHEDULER_TICK).await;
        }
    });
}

/// 执行一次调度 tick，返回本轮成功 claim 的 Automation 数量。
pub(crate) async fn tick(app: &AppHandle, now: DateTime<Utc>) -> Result<usize> {
    let dir = automations_dir(app)?;
    let claimed = claim_due_automations(&dir, now)?;
    let claimed_count = claimed.len();

    for claim in claimed {
        execute_claim(app, claim).await;
    }

    Ok(claimed_count)
}

async fn execute_claim(app: &AppHandle, claim: DueAutomation) {
    let automation_id = claim.automation.id.clone();
    let workflow_id = claim.automation.workflow_id.clone();
    let scheduled_for = claim.scheduled_for;

    let workflow = match load_saved_workflow(app, &workflow_id) {
        Ok(workflow) => workflow,
        Err(error) => {
            tracing::warn!(
                automation_id = %automation_id,
                workflow_id = %workflow_id,
                scheduled_for = %scheduled_for,
                "automation skipped because workflow could not be loaded: {}",
                error
            );
            return;
        }
    };

    match run_workflow_background(app.clone(), workflow, None).await {
        Ok(execution) => {
            tracing::info!(
                automation_id = %automation_id,
                workflow_id = %workflow_id,
                scheduled_for = %scheduled_for,
                status = ?execution.status,
                "automation workflow run finished"
            );
        }
        Err(error) => {
            tracing::warn!(
                automation_id = %automation_id,
                workflow_id = %workflow_id,
                scheduled_for = %scheduled_for,
                "automation workflow run failed before execution completed: {}",
                error
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::fs;
    use std::path::PathBuf;
    use std::pin::Pin;

    use chrono::{Duration as ChronoDuration, TimeZone};
    use serde_json::{json, Value};

    use crate::ai::automation::storage::{
        list_automations, save_automation,
    };
    use crate::ai::automation::{AutomationTrigger, WorkflowAutomation};
    use crate::ai::tool_executor::ToolExecutor;
    use crate::ai::workflow::commands::{history, storage as workflow_storage};
    use crate::ai::workflow::{
        execute_workflow, Workflow, WorkflowExecutionStatus, WorkflowStep, WorkflowToolRunner,
    };

    struct SmokeToolExecutor;

    impl ToolExecutor for SmokeToolExecutor {
        fn execute<'a>(
            &'a self,
            name: &'a str,
            args: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
            Box::pin(async move {
                Ok(json!({
                    "tool": name,
                    "args": args,
                    "source": "scheduler-smoke"
                }))
            })
        }
    }

    fn test_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "volo-automation-scheduler-smoke-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 18, hour, minute, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn scheduler_tick_is_shorter_than_minimum_user_interval() {
        assert!(SCHEDULER_TICK < Duration::from_secs(60));
    }

    #[tokio::test]
    async fn scheduled_workflow_smoke_claims_executes_tool_and_records_history() {
        let root = test_root();
        let automations_dir = root.join("automations");
        let workflows_dir = root.join("workflows");
        let runs_dir = root.join("workflow-runs");

        let workflow = Workflow {
            id: "scheduled-smoke".to_string(),
            name: "Scheduled Smoke".to_string(),
            steps: vec![WorkflowStep::Tool {
                id: "echo".to_string(),
                name: "smoke_echo".to_string(),
                args: json!({ "scheduled": true }),
            }],
        };
        workflow_storage::save_workflow(&workflows_dir, &workflow).unwrap();

        let automation = WorkflowAutomation {
            id: "scheduled-smoke-job".to_string(),
            workflow_id: workflow.id.clone(),
            enabled: true,
            trigger: AutomationTrigger::Interval {
                every_minutes: 15,
            },
        };
        save_automation(&automations_dir, automation, at(12, 0)).unwrap();

        let claimed = claim_due_automations(&automations_dir, at(12, 15)).unwrap();
        assert_eq!(claimed.len(), 1);
        let claim = &claimed[0];
        assert_eq!(claim.automation.workflow_id, workflow.id);
        assert_eq!(claim.scheduled_for, at(12, 15));

        let persisted = workflow_storage::load_workflow(
            &workflows_dir,
            &claim.automation.workflow_id,
        )
        .unwrap();
        let executor = SmokeToolExecutor;
        let runner = WorkflowToolRunner::new(&executor);
        let execution = execute_workflow(&persisted, Value::Null, &runner)
            .await
            .unwrap();

        assert_eq!(execution.status, WorkflowExecutionStatus::Completed);
        assert_eq!(
            execution.output,
            Some(json!({
                "tool": "smoke_echo",
                "args": { "scheduled": true },
                "source": "scheduler-smoke"
            }))
        );

        let finished_at = claim.scheduled_for + ChronoDuration::milliseconds(5);
        let record = history::build_record(
            &persisted,
            &execution,
            claim.scheduled_for,
            finished_at,
            5,
        );
        history::record_run(&runs_dir, &record).unwrap();

        let runs = history::list_runs(&runs_dir, Some(&persisted.id)).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].workflow_id, persisted.id);
        assert_eq!(runs[0].status, WorkflowExecutionStatus::Completed);
        assert_eq!(runs[0].steps.len(), 1);
        assert_eq!(runs[0].steps[0].step_id, "echo");

        let records = list_automations(&automations_dir).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].parsed_next_run().unwrap(), Some(at(12, 30)));
        assert!(claim_due_automations(&automations_dir, at(12, 15))
            .unwrap()
            .is_empty());

        fs::remove_dir_all(root).unwrap();
    }
}
