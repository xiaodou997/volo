//! Workflow Automation Core
//!
//! Automation 与 Workflow 分层：Workflow 描述“做什么”，Automation 描述“什么时候运行”。
//! 当前支持 interval 与 daily trigger。daily 使用运行 Volo 的系统本地时区，nextRunAt 仍持久化为 UTC。
//! 核心时间计算保持纯函数；definition/runtime state 的持久化与后台 scheduler 分模块实现。

pub(crate) mod commands;
pub(crate) mod scheduler;
mod storage;
pub use storage::AutomationRecord;

use chrono::{
    DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc,
};
use serde::{Deserialize, Serialize};

use crate::error::{Result, VoloError};

const MAX_AUTOMATION_ID_LEN: usize = 128;
const MAX_INTERVAL_MINUTES: u32 = 525_600; // 1 year
const DAILY_DST_SEARCH_MINUTES: i64 = 180;

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
    Daily {
        hour: u32,
        minute: u32,
    },
}

impl AutomationTrigger {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Interval { every_minutes } => {
                if *every_minutes == 0 || *every_minutes > MAX_INTERVAL_MINUTES {
                    return Err(VoloError::Other(format!(
                        "automation interval 必须在 1..={} 分钟之间",
                        MAX_INTERVAL_MINUTES
                    )));
                }
            }
            Self::Daily { hour, minute } => {
                if *hour > 23 || *minute > 59 {
                    return Err(VoloError::Other(
                        "automation daily 时间必须是有效的 HH:mm".to_string(),
                    ));
                }
            }
        }
        Ok(())
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

    automation.trigger.validate()
}

fn interval_seconds(every_minutes: u32) -> Result<i64> {
    if every_minutes == 0 || every_minutes > MAX_INTERVAL_MINUTES {
        return Err(VoloError::Other(format!(
            "automation interval 必须在 1..={} 分钟之间",
            MAX_INTERVAL_MINUTES
        )));
    }
    Ok(i64::from(every_minutes) * 60)
}

/// 将某个本地日期上的 daily 时间解析为 UTC occurrence。
///
/// DST 策略：
/// - 不存在的本地时间（春季跳时）向后寻找第一个有效分钟，最多 3 小时；
/// - 重复的本地时间（秋季回拨）取严格晚于 after 的最早 occurrence；
/// - 调度推进会通过 earliest_date 保证同一个本地日期最多执行一次。
fn resolve_daily_occurrence<Tz: TimeZone>(
    timezone: &Tz,
    date: NaiveDate,
    hour: u32,
    minute: u32,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    let base = date
        .and_hms_opt(hour, minute, 0)
        .ok_or_else(|| VoloError::Other("automation daily 时间无效".to_string()))?;

    for offset in 0..=DAILY_DST_SEARCH_MINUTES {
        let probe: NaiveDateTime = base
            .checked_add_signed(Duration::minutes(offset))
            .ok_or_else(|| VoloError::Other("automation daily time overflow".to_string()))?;

        match timezone.from_local_datetime(&probe) {
            LocalResult::Single(value) => {
                let candidate = value.with_timezone(&Utc);
                return Ok((candidate > after).then_some(candidate));
            }
            LocalResult::Ambiguous(first, second) => {
                let mut candidates = [first.with_timezone(&Utc), second.with_timezone(&Utc)];
                candidates.sort();
                return Ok(candidates.into_iter().find(|candidate| *candidate > after));
            }
            LocalResult::None => {}
        }
    }

    Err(VoloError::Other(
        "automation daily 时间在本地时区中无法解析".to_string(),
    ))
}

fn next_daily_run_in_timezone<Tz: TimeZone>(
    timezone: &Tz,
    hour: u32,
    minute: u32,
    after: DateTime<Utc>,
    earliest_date: Option<NaiveDate>,
) -> Result<DateTime<Utc>> {
    if hour > 23 || minute > 59 {
        return Err(VoloError::Other(
            "automation daily 时间必须是有效的 HH:mm".to_string(),
        ));
    }

    let after_local_date = after.with_timezone(timezone).date_naive();
    let mut date = earliest_date
        .filter(|date| *date > after_local_date)
        .unwrap_or(after_local_date);

    // 正常情况下最多检查今天/明天；保留 8 天上限用于异常时区边界，避免无限循环。
    for _ in 0..8 {
        if let Some(candidate) = resolve_daily_occurrence(timezone, date, hour, minute, after)? {
            return Ok(candidate);
        }
        date = date
            .succ_opt()
            .ok_or_else(|| VoloError::Other("automation daily date overflow".to_string()))?;
    }

    Err(VoloError::Other(
        "automation daily 无法计算下一次运行时间".to_string(),
    ))
}

/// 新建/启用 Automation 时，从当前时刻计算第一次运行时间。
pub fn initial_next_run(
    trigger: &AutomationTrigger,
    from: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    trigger.validate()?;
    match trigger {
        AutomationTrigger::Interval { every_minutes } => {
            let seconds = interval_seconds(*every_minutes)?;
            from.checked_add_signed(Duration::seconds(seconds))
                .ok_or_else(|| VoloError::Other("automation next run time overflow".to_string()))
        }
        AutomationTrigger::Daily { hour, minute } => {
            next_daily_run_in_timezone(&Local, *hour, *minute, from, None)
        }
    }
}

/// 某个计划时间已经到期后，计算严格晚于 `now` 的下一次计划时间。
///
/// 采用 skip-missed-runs 语义：如果应用睡眠/退出错过多个周期，不补跑历史周期。
/// interval 沿原 cadence 推进；daily 选择当前本地日期之后最近的一次目标时间，并保证
/// 同一个本地日期最多执行一次。
pub fn advance_next_run(
    trigger: &AutomationTrigger,
    scheduled_for: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    trigger.validate()?;
    if scheduled_for > now {
        return Ok(scheduled_for);
    }

    match trigger {
        AutomationTrigger::Interval { every_minutes } => {
            let interval_seconds = interval_seconds(*every_minutes)?;
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
        AutomationTrigger::Daily { hour, minute } => {
            let scheduled_date = scheduled_for.with_timezone(&Local).date_naive();
            let earliest_date = scheduled_date
                .succ_opt()
                .ok_or_else(|| VoloError::Other("automation daily date overflow".to_string()))?;
            next_daily_run_in_timezone(&Local, *hour, *minute, now, Some(earliest_date))
        }
    }
}

pub fn is_due(next_run_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    next_run_at <= now
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};
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

    fn daily(hour: u32, minute: u32) -> AutomationTrigger {
        AutomationTrigger::Daily { hour, minute }
    }

    #[test]
    fn automation_json_is_stable_and_frontend_friendly() {
        let interval_automation = WorkflowAutomation {
            id: "every-15".to_string(),
            workflow_id: "clipboard-notify".to_string(),
            enabled: true,
            trigger: interval(15),
        };

        assert_eq!(
            serde_json::to_value(&interval_automation).unwrap(),
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

        let daily_automation = WorkflowAutomation {
            id: "daily-note".to_string(),
            workflow_id: "clipboard-notify".to_string(),
            enabled: true,
            trigger: daily(9, 30),
        };
        assert_eq!(
            serde_json::to_value(&daily_automation).unwrap(),
            json!({
                "id": "daily-note",
                "workflowId": "clipboard-notify",
                "enabled": true,
                "trigger": {
                    "type": "daily",
                    "hour": 9,
                    "minute": 30
                }
            })
        );
    }

    #[test]
    fn validate_rejects_invalid_ids_intervals_and_daily_times() {
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

        let mut invalid = valid;
        invalid.trigger = daily(24, 0);
        assert!(validate_automation(&invalid).is_err());
        invalid.trigger = daily(23, 60);
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
    fn daily_schedule_uses_local_calendar_time_and_skips_to_next_day() {
        let timezone = FixedOffset::east_opt(8 * 60 * 60).unwrap();

        // UTC 00:00 == 本地 08:00，当天 09:30 => UTC 01:30。
        let first = next_daily_run_in_timezone(&timezone, 9, 30, at(0, 0), None).unwrap();
        assert_eq!(first, at(1, 30));

        // 当地当天 10:00 已经过 09:30，下一次应为次日 09:30。
        let next = next_daily_run_in_timezone(&timezone, 9, 30, at(2, 0), None).unwrap();
        assert_eq!(
            next,
            Utc.with_ymd_and_hms(2026, 9, 18, 1, 30, 0)
                .single()
                .unwrap()
        );
    }

    #[test]
    fn daily_advance_never_runs_twice_on_the_same_local_date() {
        let timezone = FixedOffset::east_opt(8 * 60 * 60).unwrap();
        let scheduled = at(1, 30); // 本地 09:30。
        let scheduled_date = scheduled.with_timezone(&timezone).date_naive();
        let earliest_date = scheduled_date.succ_opt().unwrap();

        let next = next_daily_run_in_timezone(
            &timezone,
            9,
            30,
            scheduled,
            Some(earliest_date),
        )
        .unwrap();

        assert_eq!(
            next,
            Utc.with_ymd_and_hms(2026, 9, 18, 1, 30, 0)
                .single()
                .unwrap()
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
