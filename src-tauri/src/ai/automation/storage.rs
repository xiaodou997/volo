use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{Result, VoloError};

use super::{
    advance_next_run, initial_next_run, is_due, retry_delay_seconds, validate_automation,
    WorkflowAutomation,
};

const MAX_PERSISTED_AUTOMATION_ID_BYTES: usize = 128;
static AUTOMATION_STORE_LOCK: Mutex<()> = Mutex::new(());

fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn lock_store() -> Result<MutexGuard<'static, ()>> {
    AUTOMATION_STORE_LOCK
        .lock()
        .map_err(|_| VoloError::Other("automation store lock poisoned".to_string()))
}

/// 后端托管的 Automation 持久化记录。
///
/// renderer 只提交 `WorkflowAutomation` definition；`next_run_at` 始终由后端计算和维护，
/// 避免保存 UI 表单时意外覆盖 scheduler runtime state。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRecord {
    #[serde(flatten)]
    pub automation: WorkflowAutomation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retry_attempt: u32,
}

impl AutomationRecord {
    fn parse_runtime_time(value: Option<&str>, field: &str) -> Result<Option<DateTime<Utc>>> {
        value
            .map(|value| {
                DateTime::parse_from_rfc3339(value)
                    .map(|value| value.with_timezone(&Utc))
                    .map_err(|error| {
                        VoloError::Other(format!(
                            "invalid automation {} '{}': {}",
                            field, value, error
                        ))
                    })
            })
            .transpose()
    }

    pub fn parsed_next_run(&self) -> Result<Option<DateTime<Utc>>> {
        Self::parse_runtime_time(self.next_run_at.as_deref(), "nextRunAt")
    }

    pub fn parsed_retry_at(&self) -> Result<Option<DateTime<Utc>>> {
        Self::parse_runtime_time(self.retry_at.as_deref(), "retryAt")
    }
}

/// Scheduler 成功 claim 的一次到期任务。
/// `scheduled_for` 记录本次原计划时刻；对应文件中的 nextRunAt 已经在返回前推进。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueAutomation {
    pub automation: WorkflowAutomation,
    pub scheduled_for: DateTime<Utc>,
    pub attempt: u32,
    pub is_retry: bool,
}

pub fn automations_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| VoloError::Other(format!("app_data_dir unavailable: {}", error)))?
        .join("automations"))
}

fn automation_file_name(automation_id: &str) -> Result<String> {
    if automation_id.as_bytes().len() > MAX_PERSISTED_AUTOMATION_ID_BYTES {
        return Err(VoloError::Other(format!(
            "automation id 过长，最多 {} 字节",
            MAX_PERSISTED_AUTOMATION_ID_BYTES
        )));
    }
    Ok(format!(
        "{}.json",
        URL_SAFE_NO_PAD.encode(automation_id.as_bytes())
    ))
}

fn automation_path(dir: &Path, automation_id: &str) -> Result<PathBuf> {
    Ok(dir.join(automation_file_name(automation_id)?))
}

#[cfg(unix)]
fn harden_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_dir(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn write_record_file(path: &Path, content: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let temp_path = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;

        if let Some(parent) = path.parent() {
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

#[cfg(not(unix))]
fn write_record_file(path: &Path, content: &[u8]) -> Result<()> {
    let temp_path = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temp_path, path)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn format_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn read_record(path: &Path) -> Result<AutomationRecord> {
    let content = fs::read_to_string(path)?;
    let record: AutomationRecord = serde_json::from_str(&content)?;
    validate_automation(&record.automation)?;
    record.parsed_next_run()?;
    record.parsed_retry_at()?;
    Ok(record)
}

fn write_record(dir: &Path, record: &AutomationRecord) -> Result<()> {
    fs::create_dir_all(dir)?;
    harden_dir(dir)?;
    let path = automation_path(dir, &record.automation.id)?;
    let content = serde_json::to_vec_pretty(record)?;
    write_record_file(&path, &content)
}

fn existing_record(dir: &Path, automation_id: &str) -> Result<Option<AutomationRecord>> {
    let path = automation_path(dir, automation_id)?;
    if !path.is_file() {
        return Ok(None);
    }
    read_record(&path).map(Some)
}

fn list_automations_unlocked(dir: &Path) -> Result<Vec<AutomationRecord>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        match read_record(&path) {
            Ok(record) => records.push(record),
            Err(error) => {
                tracing::warn!(path = %path.display(), "skip invalid automation file: {}", error);
            }
        }
    }

    records.sort_by(|a, b| a.automation.id.cmp(&b.automation.id));
    Ok(records)
}

/// 保存 renderer 提交的 definition，并由后端决定 `nextRunAt`。
pub fn save_automation(
    dir: &Path,
    automation: WorkflowAutomation,
    now: DateTime<Utc>,
) -> Result<AutomationRecord> {
    let _guard = lock_store()?;
    validate_automation(&automation)?;
    let existing = existing_record(dir, &automation.id)?;

    let preserve_cadence = automation.enabled && existing.as_ref().is_some_and(|record| {
        record.automation.enabled
            && record.automation.workflow_id == automation.workflow_id
            && record.automation.trigger == automation.trigger
            && record.next_run_at.is_some()
    });

    let next_run_at = if !automation.enabled {
        None
    } else if preserve_cadence {
        existing.as_ref().and_then(|record| record.next_run_at.clone())
    } else {
        Some(format_time(initial_next_run(&automation.trigger, now)?))
    };

    let preserve_retry = preserve_cadence && existing.as_ref().is_some_and(|record| {
        record.automation.retry == automation.retry && record.retry_at.is_some()
    });
    let (retry_at, retry_attempt) = if preserve_retry {
        existing
            .as_ref()
            .map(|record| (record.retry_at.clone(), record.retry_attempt))
            .unwrap_or((None, 0))
    } else {
        (None, 0)
    };

    let record = AutomationRecord {
        automation,
        next_run_at,
        retry_at,
        retry_attempt,
    };
    write_record(dir, &record)?;
    Ok(record)
}

pub fn list_automations(dir: &Path) -> Result<Vec<AutomationRecord>> {
    let _guard = lock_store()?;
    list_automations_unlocked(dir)
}

pub fn delete_automation(dir: &Path, automation_id: &str) -> Result<()> {
    let _guard = lock_store()?;
    let path = automation_path(dir, automation_id)?;
    if !path.is_file() {
        return Err(VoloError::NotFound(format!(
            "automation: {}",
            automation_id
        )));
    }
    fs::remove_file(path)?;
    Ok(())
}

/// Scheduler 专用：只更新后端运行状态，不允许 renderer 调用。
pub fn update_next_run(
    dir: &Path,
    automation_id: &str,
    next_run_at: Option<DateTime<Utc>>,
) -> Result<AutomationRecord> {
    let _guard = lock_store()?;
    let mut record = existing_record(dir, automation_id)?
        .ok_or_else(|| VoloError::NotFound(format!("automation: {}", automation_id)))?;
    record.next_run_at = next_run_at.map(format_time);
    write_record(dir, &record)?;
    Ok(record)
}

/// 为一次失败执行安排下一次有限重试。
///
/// `attempt` 从 1 开始，1 表示原始 cadence 执行；返回 None 表示没有 retry policy、
/// 已达到 maxRetries，或 Automation 已停用。重试采用指数退避并由核心策略封顶。
pub fn schedule_retry(
    dir: &Path,
    automation_id: &str,
    attempt: u32,
    failed_at: DateTime<Utc>,
) -> Result<Option<AutomationRecord>> {
    let _guard = lock_store()?;
    let mut record = existing_record(dir, automation_id)?
        .ok_or_else(|| VoloError::NotFound(format!("automation: {}", automation_id)))?;

    if !record.automation.enabled {
        return Ok(None);
    }
    let Some(policy) = record.automation.retry.as_ref() else {
        return Ok(None);
    };

    let retries_already_used = attempt.saturating_sub(1);
    if retries_already_used >= policy.max_retries {
        record.retry_at = None;
        record.retry_attempt = 0;
        write_record(dir, &record)?;
        return Ok(None);
    }

    let retry_number = retries_already_used + 1;
    let delay_seconds = retry_delay_seconds(policy, retry_number)?;
    let retry_at = failed_at
        .checked_add_signed(chrono::Duration::seconds(i64::from(delay_seconds)))
        .ok_or_else(|| VoloError::Other("automation retry time overflow".to_string()))?;

    record.retry_at = Some(format_time(retry_at));
    record.retry_attempt = attempt.saturating_add(1);
    write_record(dir, &record)?;
    Ok(Some(record))
}

/// 原子 claim 当前到期的 enabled Automations：
/// - 在同一把存储锁内重新读取最新 definition；
/// - 对每个 due record 先推进并持久化 nextRunAt，再返回执行 claim；
/// - enabled 但缺失 nextRunAt 的旧/异常记录只修复下一次时间，本轮不立即执行；
/// - 锁在真正执行 Workflow 前释放，避免长任务阻塞 UI 的 save/delete。
pub fn claim_due_automations(dir: &Path, now: DateTime<Utc>) -> Result<Vec<DueAutomation>> {
    let _guard = lock_store()?;
    let records = list_automations_unlocked(dir)?;
    let mut claimed = Vec::new();

    for mut record in records {
        if !record.automation.enabled {
            continue;
        }

        if let Some(retry_at) = record.parsed_retry_at()? {
            // retry pending 时暂停新的 cadence occurrence，避免定时执行与重试并发/抢跑。
            if !is_due(retry_at, now) {
                continue;
            }

            let attempt = record.retry_attempt.max(2);
            record.retry_at = None;
            record.retry_attempt = 0;

            // retry 期间如果正常 cadence 已经过期，同样按 skip-missed-runs 推进到未来，
            // 避免重试成功后立刻又补跑一个旧 occurrence。
            if let Some(next_run_at) = record.parsed_next_run()? {
                if is_due(next_run_at, now) {
                    let advanced =
                        advance_next_run(&record.automation.trigger, next_run_at, now)?;
                    record.next_run_at = Some(format_time(advanced));
                }
            }
            write_record(dir, &record)?;
            claimed.push(DueAutomation {
                automation: record.automation,
                scheduled_for: retry_at,
                attempt,
                is_retry: true,
            });
            continue;
        }

        let Some(scheduled_for) = record.parsed_next_run()? else {
            let repaired = initial_next_run(&record.automation.trigger, now)?;
            record.next_run_at = Some(format_time(repaired));
            write_record(dir, &record)?;
            continue;
        };

        if !is_due(scheduled_for, now) {
            continue;
        }

        let next_run = advance_next_run(&record.automation.trigger, scheduled_for, now)?;
        record.next_run_at = Some(format_time(next_run));
        write_record(dir, &record)?;
        claimed.push(DueAutomation {
            automation: record.automation,
            scheduled_for,
            attempt: 1,
            is_retry: false,
        });
    }

    Ok(claimed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::automation::AutomationTrigger;
    use chrono::TimeZone;

    fn test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "volo-automation-storage-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 17, hour, minute, 0)
            .single()
            .unwrap()
    }

    fn automation(id: &str, minutes: u32, enabled: bool) -> WorkflowAutomation {
        WorkflowAutomation {
            id: id.to_string(),
            workflow_id: "workflow-1".to_string(),
            enabled,
            trigger: AutomationTrigger::Interval {
                every_minutes: minutes,
            },
            retry: None,
        }
    }

    #[test]
    fn encoded_filename_never_exposes_path_segments() {
        let name = automation_file_name("../nested/job").unwrap();
        assert!(name.ends_with(".json"));
        assert!(!name.contains('/'));
        assert!(!name.contains(".."));
    }

    #[test]
    fn save_list_and_delete_round_trip() {
        let dir = test_dir();
        let saved = save_automation(&dir, automation("job-1", 15, true), at(12, 0)).unwrap();
        assert_eq!(saved.parsed_next_run().unwrap(), Some(at(12, 15)));
        assert_eq!(list_automations(&dir).unwrap(), vec![saved]);

        delete_automation(&dir, "job-1").unwrap();
        assert!(list_automations(&dir).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unchanged_enabled_definition_preserves_runtime_cadence() {
        let dir = test_dir();
        let definition = automation("job-1", 15, true);
        let first = save_automation(&dir, definition.clone(), at(12, 0)).unwrap();
        assert_eq!(first.parsed_next_run().unwrap(), Some(at(12, 15)));

        let second = save_automation(&dir, definition, at(12, 5)).unwrap();
        assert_eq!(second.parsed_next_run().unwrap(), Some(at(12, 15)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn changed_trigger_restarts_schedule_from_save_time() {
        let dir = test_dir();
        save_automation(&dir, automation("job-1", 15, true), at(12, 0)).unwrap();
        let changed = save_automation(&dir, automation("job-1", 30, true), at(12, 5)).unwrap();
        assert_eq!(changed.parsed_next_run().unwrap(), Some(at(12, 35)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disable_clears_next_run_and_reenable_restarts_from_now() {
        let dir = test_dir();
        save_automation(&dir, automation("job-1", 15, true), at(12, 0)).unwrap();
        let disabled = save_automation(&dir, automation("job-1", 15, false), at(12, 5)).unwrap();
        assert_eq!(disabled.next_run_at, None);

        let enabled = save_automation(&dir, automation("job-1", 15, true), at(12, 10)).unwrap();
        assert_eq!(enabled.parsed_next_run().unwrap(), Some(at(12, 25)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scheduler_state_update_does_not_change_definition() {
        let dir = test_dir();
        let definition = automation("job-1", 15, true);
        save_automation(&dir, definition.clone(), at(12, 0)).unwrap();
        let updated = update_next_run(&dir, "job-1", Some(at(13, 0))).unwrap();
        assert_eq!(updated.automation, definition);
        assert_eq!(updated.parsed_next_run().unwrap(), Some(at(13, 0)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claim_due_advances_before_return_and_does_not_claim_twice() {
        let dir = test_dir();
        save_automation(&dir, automation("job-1", 15, true), at(12, 0)).unwrap();

        let claimed = claim_due_automations(&dir, at(12, 15)).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].automation.id, "job-1");
        assert_eq!(claimed[0].scheduled_for, at(12, 15));
        assert_eq!(claimed[0].attempt, 1);
        assert!(!claimed[0].is_retry);
        assert_eq!(
            list_automations(&dir).unwrap()[0]
                .parsed_next_run()
                .unwrap(),
            Some(at(12, 30))
        );
        assert!(claim_due_automations(&dir, at(12, 15)).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claim_due_skips_missed_occurrences() {
        let dir = test_dir();
        save_automation(&dir, automation("job-1", 15, true), at(12, 0)).unwrap();

        let claimed = claim_due_automations(&dir, at(12, 47)).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].scheduled_for, at(12, 15));
        assert_eq!(
            list_automations(&dir).unwrap()[0]
                .parsed_next_run()
                .unwrap(),
            Some(at(13, 0))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn retry_pending_blocks_cadence_and_retry_claim_skips_missed_occurrence() {
        use crate::ai::automation::AutomationRetryPolicy;

        let dir = test_dir();
        let mut definition = automation("job-1", 1, true);
        definition.retry = Some(AutomationRetryPolicy {
            max_retries: 2,
            initial_delay_seconds: 120,
        });
        save_automation(&dir, definition, at(12, 0)).unwrap();

        let first = claim_due_automations(&dir, at(12, 1)).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].attempt, 1);

        let retry = schedule_retry(&dir, "job-1", first[0].attempt, at(12, 1))
            .unwrap()
            .unwrap();
        assert_eq!(retry.parsed_retry_at().unwrap(), Some(at(12, 3)));
        assert_eq!(retry.retry_attempt, 2);

        // 12:02 正常 cadence 已到，但 retry 尚未到：不允许新 occurrence 抢跑。
        assert!(claim_due_automations(&dir, at(12, 2)).unwrap().is_empty());

        let retried = claim_due_automations(&dir, at(12, 3)).unwrap();
        assert_eq!(retried.len(), 1);
        assert!(retried[0].is_retry);
        assert_eq!(retried[0].attempt, 2);
        assert_eq!(retried[0].scheduled_for, at(12, 3));

        let state = list_automations(&dir).unwrap().remove(0);
        assert_eq!(state.retry_at, None);
        assert_eq!(state.retry_attempt, 0);
        assert_eq!(state.parsed_next_run().unwrap(), Some(at(12, 4)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn retry_stops_after_configured_limit() {
        use crate::ai::automation::AutomationRetryPolicy;

        let dir = test_dir();
        let mut definition = automation("job-1", 15, true);
        definition.retry = Some(AutomationRetryPolicy {
            max_retries: 1,
            initial_delay_seconds: 30,
        });
        save_automation(&dir, definition, at(12, 0)).unwrap();

        assert!(schedule_retry(&dir, "job-1", 1, at(12, 15))
            .unwrap()
            .is_some());
        assert!(schedule_retry(&dir, "job-1", 2, at(12, 16))
            .unwrap()
            .is_none());

        let state = list_automations(&dir).unwrap().remove(0);
        assert_eq!(state.retry_at, None);
        assert_eq!(state.retry_attempt, 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disabled_automation_is_never_claimed() {
        let dir = test_dir();
        save_automation(&dir, automation("job-1", 15, false), at(12, 0)).unwrap();
        assert!(claim_due_automations(&dir, at(13, 0)).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_does_not_break_listing() {
        let dir = test_dir();
        let valid = save_automation(&dir, automation("valid", 15, true), at(12, 0)).unwrap();
        fs::write(dir.join("broken.json"), "{not-json").unwrap();
        assert_eq!(list_automations(&dir).unwrap(), vec![valid]);
        fs::remove_dir_all(dir).unwrap();
    }
}
