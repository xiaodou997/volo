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

    #[test]
    fn scheduler_tick_is_shorter_than_minimum_user_interval() {
        assert!(SCHEDULER_TICK < Duration::from_secs(60));
    }
}
