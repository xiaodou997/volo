//! Workflow Automation Core
//!
//! Automation 与 Workflow 分层：Workflow 描述“做什么”，Automation 描述“什么时候运行”。
//! 第一版只支持 interval trigger，刻意不引入 daily/cron，避免时区与 DST 语义在 MVP 阶段失控。
//! 核心时间计算保持纯函数；definition/runtime state 的持久化与后台 scheduler 分模块实现。

pub(crate) mod commands;
pub(crate) mod scheduler;
mod storage;
pub use storage::AutomationRecord;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Result, VoloError};

const MAX_AUTOMATION_ID_LEN: usize = 128;
const MAX_INTERVAL_MINUTES: u32 = 525_600; // 1 year

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAutomation {
    pub id: String,
    pub workflow_id: String,
    pub enabled: bool,
    pub trigger: AutomationTrigger,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationTrigger {
    Interval {
        #[serde(rename = "everyMinutes")]
        every_minutes: u32,
    },
}

impl AutomationTrigger {
    fn interval_seconds(&self) -> Result<i64> {
        match self {
            Self::Interval { every_minutes } => {
                if *every_minutes == 0 || *every_minutes > MAX_INTERVAL_MINUTES {
                    return Err(VoloError::Other(format!(
                        "automation interval 必须在 1..={} 分钟之间",
                        MAX_INTERVAL_MINUTES
                    )));
                }
                Ok(i64::from(*every_minutes) * 60)
            }
        }
    }
}

pub fn validate_automation(automation: &WorkflowAutomation) -> Result<()> {
    if automation.id.trim().is_empty() {
        return Err(VoloError::Other("automation id 不能为空".to_string()));
    }
    if automation.id.trim() != automation.id {
        return Err(VoloError::Other(
            "automation id 不能包含首尾空白".to_string(),
        ));
    }
    if automation.id.len() > MAX_AUTOMATION_ID_LEN {
        return Err(VoloError::Other(format!(
            "automation id 不能超过 {} 字节",
            MAX_AUTOMATION_ID_LEN
        )));
    }
    if automation.workflow_id.trim().is_empty() {
        return Err(VoloError::Other("automation workflowId 不能为空".to_string()));
    }

    automation.trigger.interval_seconds()?;
    Ok(())
}

/// 新建/启用 Automation 时，从当前时刻计算第一次运行时间。
pub fn initial_next_run(
    trigger: &AutomationTrigger,
    from: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let seconds = trigger.interval_seconds()?;
    from.checked_add_signed(Duration::seconds(seconds))
        .ok_or_else(|| VoloError::Other("automation next run time overflow".to_string()))
}

/// 某个计划时间已经到期后，计算严格晚于 `now` 的下一次计划时间。
///
/// 采用 skip-missed-runs 语义：如果应用睡眠/退出错过多个周期，不补跑历史周期，
/// 只沿原 cadence 推进到未来的第一个 occurrence，避免恢复时突发连续执行。
pub fn advance_next_run(
    trigger: &AutomationTrigger,
    scheduled_for: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    if scheduled_for > now {
        return Ok(scheduled_for);
    }

    let interval_seconds = trigger.interval_seconds()?;
    let elapsed_seconds = now.signed_duration_since(scheduled_for).num_seconds();
    let steps = elapsed_seconds
        .checked_div(interval_seconds)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| VoloError::Other("automation cadence overflow".to_string()))?;
    let advance_seconds = interval_seconds
        .checked_mul(steps)
        .ok_or_else(|| VoloError::Other("automation cadence overflow".to_string()))?;

    scheduled_for
        .checked_add_signed(Duration::seconds(advance_seconds))
        .ok_or_else(|| VoloError::Other("automation next run time overflow".to_string()))
}

pub fn is_due(next_run_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    next_run_at <= now
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 17, hour, minute, 0)
            .single()
            .unwrap()
    }

    fn interval(minutes: u32) -> AutomationTrigger {
        AutomationTrigger::Interval {
            every_minutes: minutes,
        }
    }

    #[test]
    fn automation_json_is_stable_and_frontend_friendly() {
        let automation = WorkflowAutomation {
            id: "every-15".to_string(),
            workflow_id: "clipboard-notify".to_string(),
            enabled: true,
            trigger: interval(15),
        };

        assert_eq!(
            serde_json::to_value(&automation).unwrap(),
            json!({
                "id": "every-15",
                "workflowId": "clipboard-notify",
                "enabled": true,
                "trigger": {
                    "type": "interval",
                    "everyMinutes": 15
                }
            })
        );
    }

    #[test]
    fn validate_rejects_invalid_ids_and_intervals() {
        let valid = WorkflowAutomation {
            id: "job-1".to_string(),
            workflow_id: "workflow-1".to_string(),
            enabled: true,
            trigger: interval(15),
        };
        assert!(validate_automation(&valid).is_ok());

        let mut invalid = valid.clone();
        invalid.id = " ".to_string();
        assert!(validate_automation(&invalid).is_err());

        let mut invalid = valid.clone();
        invalid.workflow_id = "".to_string();
        assert!(validate_automation(&invalid).is_err());

        let mut invalid = valid.clone();
        invalid.trigger = interval(0);
        assert!(validate_automation(&invalid).is_err());
    }

    #[test]
    fn initial_next_run_adds_one_interval() {
        assert_eq!(
            initial_next_run(&interval(15), at(12, 0)).unwrap(),
            at(12, 15)
        );
    }

    #[test]
    fn advance_preserves_cadence_and_skips_missed_runs() {
        // 计划 12:15 执行，但应用到 12:47 才恢复：12:30/12:45 都不补跑，下一次为 13:00。
        assert_eq!(
            advance_next_run(&interval(15), at(12, 15), at(12, 47)).unwrap(),
            at(13, 0)
        );
    }

    #[test]
    fn advance_from_exact_due_time_moves_to_next_occurrence() {
        assert_eq!(
            advance_next_run(&interval(15), at(12, 15), at(12, 15)).unwrap(),
            at(12, 30)
        );
    }

    #[test]
    fn future_schedule_is_left_unchanged() {
        assert_eq!(
            advance_next_run(&interval(15), at(12, 30), at(12, 20)).unwrap(),
            at(12, 30)
        );
        assert!(!is_due(at(12, 30), at(12, 20)));
        assert!(is_due(at(12, 30), at(12, 30)));
    }
}
