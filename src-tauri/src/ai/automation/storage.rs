use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{Result, VoloError};

use super::{initial_next_run, validate_automation, WorkflowAutomation};

const MAX_PERSISTED_AUTOMATION_ID_BYTES: usize = 128;

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
}

impl AutomationRecord {
    pub fn parsed_next_run(&self) -> Result<Option<DateTime<Utc>>> {
        self.next_run_at
            .as_deref()
            .map(|value| {
                DateTime::parse_from_rfc3339(value)
                    .map(|value| value.with_timezone(&Utc))
                    .map_err(|error| {
                        VoloError::Other(format!(
                            "invalid automation nextRunAt '{}': {}",
                            value, error
                        ))
                    })
            })
            .transpose()
    }
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

/// 保存 renderer 提交的 definition，并由后端决定 `nextRunAt`。
pub fn save_automation(
    dir: &Path,
    automation: WorkflowAutomation,
    now: DateTime<Utc>,
) -> Result<AutomationRecord> {
    validate_automation(&automation)?;
    let existing = existing_record(dir, &automation.id)?;

    let next_run_at = if !automation.enabled {
        None
    } else {
        let preserve = existing.as_ref().is_some_and(|record| {
            record.automation.enabled
                && record.automation.workflow_id == automation.workflow_id
                && record.automation.trigger == automation.trigger
                && record.next_run_at.is_some()
        });

        if preserve {
            existing.and_then(|record| record.next_run_at)
        } else {
            Some(format_time(initial_next_run(&automation.trigger, now)?))
        }
    };

    let record = AutomationRecord {
        automation,
        next_run_at,
    };
    write_record(dir, &record)?;
    Ok(record)
}

pub fn list_automations(dir: &Path) -> Result<Vec<AutomationRecord>> {
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

pub fn delete_automation(dir: &Path, automation_id: &str) -> Result<()> {
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
    let mut record = existing_record(dir, automation_id)?
        .ok_or_else(|| VoloError::NotFound(format!("automation: {}", automation_id)))?;
    record.next_run_at = next_run_at.map(format_time);
    write_record(dir, &record)?;
    Ok(record)
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
    fn corrupt_file_does_not_break_listing() {
        let dir = test_dir();
        let valid = save_automation(&dir, automation("valid", 15, true), at(12, 0)).unwrap();
        fs::write(dir.join("broken.json"), "{not-json").unwrap();
        assert_eq!(list_automations(&dir).unwrap(), vec![valid]);
        fs::remove_dir_all(dir).unwrap();
    }
}
