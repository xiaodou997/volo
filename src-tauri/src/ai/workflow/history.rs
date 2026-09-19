use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{Result, VoloError};

use super::super::{Workflow, WorkflowExecution, WorkflowExecutionStatus, WorkflowStepStatus};

const MAX_LISTED_WORKFLOW_RUNS: usize = 100;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunSource {
    #[default]
    Manual,
    Automation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunContext {
    pub source: WorkflowRunSource,
    pub automation_id: Option<String>,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub retry_attempt: Option<u32>,
}

impl WorkflowRunContext {
    pub fn manual() -> Self {
        Self {
            source: WorkflowRunSource::Manual,
            automation_id: None,
            scheduled_for: None,
            retry_attempt: None,
        }
    }

    pub fn automation(
        automation_id: impl Into<String>,
        scheduled_for: DateTime<Utc>,
        retry_attempt: Option<u32>,
    ) -> Self {
        Self {
            source: WorkflowRunSource::Automation,
            automation_id: Some(automation_id.into()),
            scheduled_for: Some(scheduled_for),
            retry_attempt,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunStepRecord {
    pub step_id: String,
    pub status: WorkflowStepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Workflow 执行审计记录。
///
/// 有意不持久化 input / step output / final output，避免剪贴板、文件内容或其他敏感数据
/// 因为“执行历史”被长期落盘。当前运行的完整输出仍由 WorkflowExecution 返回给 UI。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunRecord {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    #[serde(default)]
    pub source: WorkflowRunSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_attempt: Option<u32>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub status: WorkflowExecutionStatus,
    pub steps: Vec<WorkflowRunStepRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn workflow_runs_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| VoloError::Other(format!("app_data_dir unavailable: {}", error)))?
        .join("workflow-runs"))
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

fn run_id(started_at: DateTime<Utc>) -> String {
    format!(
        "{}-{}",
        started_at.timestamp_millis(),
        &uuid::Uuid::new_v4().to_string()[..8]
    )
}

#[cfg(unix)]
fn write_record(path: &Path, content: &[u8]) -> Result<()> {
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
fn write_record(path: &Path, content: &[u8]) -> Result<()> {
    let temp_path = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

pub fn build_record(
    workflow: &Workflow,
    execution: &WorkflowExecution,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    duration_ms: u64,
) -> WorkflowRunRecord {
    build_record_with_context(
        workflow,
        execution,
        &WorkflowRunContext::manual(),
        started_at,
        finished_at,
        duration_ms,
    )
}

pub fn build_record_with_context(
    workflow: &Workflow,
    execution: &WorkflowExecution,
    context: &WorkflowRunContext,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    duration_ms: u64,
) -> WorkflowRunRecord {
    WorkflowRunRecord {
        id: run_id(started_at),
        workflow_id: workflow.id.clone(),
        workflow_name: workflow.name.clone(),
        source: context.source,
        automation_id: context.automation_id.clone(),
        scheduled_for: context
            .scheduled_for
            .as_ref()
            .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true)),
        retry_attempt: context.retry_attempt,
        started_at: started_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        finished_at: finished_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        duration_ms,
        status: execution.status,
        steps: execution
            .steps
            .iter()
            .map(|step| WorkflowRunStepRecord {
                step_id: step.step_id.clone(),
                status: step.status,
                error: step.error.clone(),
            })
            .collect(),
        error: execution.error.clone(),
    }
}

pub fn record_run(dir: &Path, record: &WorkflowRunRecord) -> Result<()> {
    fs::create_dir_all(dir)?;
    harden_dir(dir)?;
    let content = serde_json::to_vec_pretty(record)?;
    write_record(&dir.join(format!("{}.json", record.id)), &content)
}

/// 返回最近的执行记录；可按 workflow id 过滤。
/// 单个损坏记录只 warning + skip，不影响其余历史读取。
pub fn list_runs(dir: &Path, workflow_id: Option<&str>) -> Result<Vec<WorkflowRunRecord>> {
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

        let record = match fs::read_to_string(&path)
            .map_err(VoloError::from)
            .and_then(|content| {
                serde_json::from_str::<WorkflowRunRecord>(&content).map_err(VoloError::from)
            }) {
            Ok(record) => record,
            Err(error) => {
                tracing::warn!(path = %path.display(), "skip invalid workflow run record: {}", error);
                continue;
            }
        };

        if workflow_id.is_some_and(|id| record.workflow_id != id) {
            continue;
        }
        records.push(record);
    }

    records.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    records.truncate(MAX_LISTED_WORKFLOW_RUNS);
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::workflow::{WorkflowExecution, WorkflowStep, WorkflowStepExecution};
    use serde_json::json;

    fn test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "volo-workflow-run-history-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn workflow(id: &str, name: &str) -> Workflow {
        Workflow {
            id: id.to_string(),
            name: name.to_string(),
            steps: vec![WorkflowStep::Tool {
                id: "read".to_string(),
                name: "clipboard_read".to_string(),
                args: json!({}),
            }],
        }
    }

    fn execution(status: WorkflowExecutionStatus) -> WorkflowExecution {
        let failed = status == WorkflowExecutionStatus::Failed;
        WorkflowExecution {
            workflow_id: "daily".to_string(),
            status,
            steps: vec![WorkflowStepExecution {
                step_id: "read".to_string(),
                status: if failed {
                    WorkflowStepStatus::Failed
                } else {
                    WorkflowStepStatus::Completed
                },
                output: Some(json!({ "secret": "must-not-be-persisted" })),
                error: failed.then(|| "permission denied".to_string()),
            }],
            output: Some(json!("also-not-persisted")),
            error: failed.then(|| "permission denied".to_string()),
        }
    }

    #[test]
    fn audit_record_excludes_execution_outputs() {
        let workflow = workflow("daily", "Daily");
        let started = Utc::now();
        let record = build_record(
            &workflow,
            &execution(WorkflowExecutionStatus::Completed),
            started,
            started,
            12,
        );
        let encoded = serde_json::to_value(&record).unwrap();

        assert!(encoded.get("output").is_none());
        assert!(encoded["steps"][0].get("output").is_none());
        assert_eq!(encoded["durationMs"], 12);
        assert_eq!(record.source, WorkflowRunSource::Manual);
        assert!(record.automation_id.is_none());
        assert!(record.scheduled_for.is_none());
        assert!(record.retry_attempt.is_none());
    }

    #[test]
    fn record_and_list_can_filter_by_workflow() {
        let dir = test_dir();
        let started = Utc::now();

        let first = build_record(
            &workflow("a", "A"),
            &execution(WorkflowExecutionStatus::Completed),
            started,
            started,
            1,
        );
        let second = build_record(
            &workflow("b", "B"),
            &execution(WorkflowExecutionStatus::Failed),
            started + chrono::Duration::milliseconds(1),
            started + chrono::Duration::milliseconds(2),
            2,
        );
        record_run(&dir, &first).unwrap();
        record_run(&dir, &second).unwrap();

        let all = list_runs(&dir, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].workflow_id, "b");

        let filtered = list_runs(&dir, Some("a")).unwrap();
        assert_eq!(filtered, vec![first]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_steps_keep_errors_for_audit() {
        let workflow = workflow("daily", "Daily");
        let started = Utc::now();
        let record = build_record(
            &workflow,
            &execution(WorkflowExecutionStatus::Failed),
            started,
            started,
            4,
        );

        assert_eq!(record.status, WorkflowExecutionStatus::Failed);
        assert_eq!(record.steps[0].status, WorkflowStepStatus::Failed);
        assert_eq!(record.steps[0].error.as_deref(), Some("permission denied"));
        assert_eq!(record.error.as_deref(), Some("permission denied"));
    }

    #[test]
    fn automation_provenance_is_persisted_without_execution_payloads() {
        let workflow = workflow("daily", "Daily");
        let started = Utc::now();
        let scheduled_for = started - chrono::Duration::seconds(3);
        let record = build_record_with_context(
            &workflow,
            &execution(WorkflowExecutionStatus::Completed),
            &WorkflowRunContext::automation("daily-job", scheduled_for, Some(2)),
            started,
            started,
            12,
        );
        let encoded = serde_json::to_value(&record).unwrap();

        assert_eq!(encoded["source"], "automation");
        assert_eq!(encoded["automationId"], "daily-job");
        assert_eq!(encoded["retryAttempt"], 2);
        assert_eq!(
            encoded["scheduledFor"],
            scheduled_for.to_rfc3339_opts(SecondsFormat::Millis, true)
        );
        assert!(encoded.get("output").is_none());
        assert!(encoded["steps"][0].get("output").is_none());
    }

    #[test]
    fn legacy_history_without_source_defaults_to_manual() {
        let value = serde_json::json!({
            "id": "legacy-run",
            "workflowId": "legacy",
            "workflowName": "Legacy",
            "startedAt": "2026-09-18T00:00:00.000Z",
            "finishedAt": "2026-09-18T00:00:01.000Z",
            "durationMs": 1000,
            "status": "completed",
            "steps": []
        });
        let record: WorkflowRunRecord = serde_json::from_value(value).unwrap();
        assert_eq!(record.source, WorkflowRunSource::Manual);
        assert!(record.automation_id.is_none());
        assert!(record.scheduled_for.is_none());
        assert!(record.retry_attempt.is_none());
    }

    #[test]
    fn corrupt_history_file_does_not_break_listing() {
        let dir = test_dir();
        let started = Utc::now();
        let record = build_record(
            &workflow("valid", "Valid"),
            &execution(WorkflowExecutionStatus::Completed),
            started,
            started,
            1,
        );
        record_run(&dir, &record).unwrap();
        fs::write(dir.join("broken.json"), "{bad").unwrap();

        assert_eq!(list_runs(&dir, None).unwrap(), vec![record]);
        fs::remove_dir_all(dir).unwrap();
    }
}
