//! Automation 后台调度循环。
//!
//! MVP 语义：
//! - 每 15 秒检查一次 due Automations；
//! - storage 在 claim 时先推进 nextRunAt，避免同一 occurrence 重复执行；
//! - missed occurrences 不补跑；
//! - 同一 tick 内最多并发执行 4 个 due jobs；tick 会等待本批次完成，因此不会跨 tick 重叠；
//! - 未配置 retryPolicy 时保持原行为；配置后按固定 backoff 重试，且 retry 不跨过下一次正常 schedule；
//! - 缺失 Workflow 不会阻断 scheduler，下一周期仍会继续尝试。

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use tauri::AppHandle;

use crate::ai::workflow::commands::{
    history::WorkflowRunContext, load_saved_workflow, run_workflow_background,
};
use crate::ai::workflow::WorkflowExecutionStatus;
use crate::error::Result;

use super::storage::{
    automations_dir, claim_due_automations, schedule_retry_after_failure, DueAutomation,
};

const SCHEDULER_TICK: Duration = Duration::from_secs(15);
const MAX_CONCURRENT_AUTOMATIONS: usize = 4;

async fn run_bounded<T, F, Fut>(items: Vec<T>, limit: usize, f: F)
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = ()>,
{
    stream::iter(items)
        .for_each_concurrent(Some(limit), f)
        .await;
}

/// 在 Tauri runtime 上启动单个 scheduler loop。
pub(crate) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tracing::info!(
            tick_seconds = SCHEDULER_TICK.as_secs(),
            max_concurrent = MAX_CONCURRENT_AUTOMATIONS,
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

    // 本批次有界并发，但 tick 本身会等待全部完成。这样可以并行处理不同 Automation，
    // 同时保持“不会在下一 tick 再次启动同一 Automation”的简单无重叠语义。
    run_bounded(claimed, MAX_CONCURRENT_AUTOMATIONS, |claim| {
        execute_claim(app, &dir, claim)
    })
    .await;

    Ok(claimed_count)
}

fn schedule_retry(dir: &Path, claim: &DueAutomation) {
    match schedule_retry_after_failure(dir, claim, Utc::now()) {
        Ok(Some(record)) => {
            if let Some(retry) = record.retry_state {
                tracing::info!(
                    automation_id = %claim.automation.id,
                    retry_attempt = retry.attempt,
                    retry_at = %retry.retry_at,
                    "automation retry scheduled"
                );
            }
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(
                automation_id = %claim.automation.id,
                "failed to persist automation retry state: {}",
                error
            );
        }
    }
}

async fn execute_claim(app: &AppHandle, dir: &Path, claim: DueAutomation) {
    let automation_id = claim.automation.id.clone();
    let workflow_id = claim.automation.workflow_id.clone();
    let scheduled_for = claim.scheduled_for;
    let retry_attempt = claim.retry_attempt;

    let workflow = match load_saved_workflow(app, &workflow_id) {
        Ok(workflow) => workflow,
        Err(error) => {
            tracing::warn!(
                automation_id = %automation_id,
                workflow_id = %workflow_id,
                scheduled_for = %scheduled_for,
                retry_attempt = ?retry_attempt,
                "automation skipped because workflow could not be loaded: {}",
                error
            );
            schedule_retry(dir, &claim);
            return;
        }
    };

    let run_context =
        WorkflowRunContext::automation(automation_id.clone(), scheduled_for, retry_attempt);

    match run_workflow_background(app.clone(), workflow, None, run_context).await {
        Ok(execution) => {
            if execution.status == WorkflowExecutionStatus::Failed {
                schedule_retry(dir, &claim);
            }
            tracing::info!(
                automation_id = %automation_id,
                workflow_id = %workflow_id,
                scheduled_for = %scheduled_for,
                retry_attempt = ?retry_attempt,
                status = ?execution.status,
                "automation workflow run finished"
            );
        }
        Err(error) => {
            tracing::warn!(
                automation_id = %automation_id,
                workflow_id = %workflow_id,
                scheduled_for = %scheduled_for,
                retry_attempt = ?retry_attempt,
                "automation workflow run failed before execution completed: {}",
                error
            );
            schedule_retry(dir, &claim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::pin::Pin;

    use chrono::{Duration as ChronoDuration, TimeZone};
    use serde_json::{json, Value};

    use crate::ai::automation::storage::{list_automations, save_automation};
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
    async fn bounded_runner_executes_in_parallel_without_exceeding_limit() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        run_bounded((0..8).collect(), 3, {
            let current = Arc::clone(&current);
            let peak = Arc::clone(&peak);
            move |_| {
                let current = Arc::clone(&current);
                let peak = Arc::clone(&peak);
                async move {
                    let active = current.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(active, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    current.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })
        .await;

        let peak = peak.load(Ordering::SeqCst);
        assert!(peak > 1, "expected actual parallel progress");
        assert!(peak <= 3, "bounded runner exceeded concurrency limit");
        assert_eq!(current.load(Ordering::SeqCst), 0);
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
            trigger: AutomationTrigger::Interval { every_minutes: 15 },
            retry_policy: None,
        };
        save_automation(&automations_dir, automation, at(12, 0)).unwrap();

        let claimed = claim_due_automations(&automations_dir, at(12, 15)).unwrap();
        assert_eq!(claimed.len(), 1);
        let claim = &claimed[0];
        assert_eq!(claim.automation.workflow_id, workflow.id);
        assert_eq!(claim.scheduled_for, at(12, 15));

        let persisted =
            workflow_storage::load_workflow(&workflows_dir, &claim.automation.workflow_id).unwrap();
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
        let record = history::build_record_with_context(
            &persisted,
            &execution,
            &history::WorkflowRunContext::automation(
                claim.automation.id.clone(),
                claim.scheduled_for,
                claim.retry_attempt,
            ),
            claim.scheduled_for,
            finished_at,
            5,
        );
        history::record_run(&runs_dir, &record).unwrap();

        let runs = history::list_runs(&runs_dir, Some(&persisted.id)).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].workflow_id, persisted.id);
        assert_eq!(runs[0].source, history::WorkflowRunSource::Automation);
        assert_eq!(
            runs[0].automation_id.as_deref(),
            Some("scheduled-smoke-job")
        );
        assert_eq!(
            runs[0].scheduled_for.as_deref(),
            Some("2026-09-18T12:15:00.000Z")
        );
        assert_eq!(runs[0].retry_attempt, None);
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
