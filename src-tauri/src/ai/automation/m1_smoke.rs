use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};

use crate::ai::llm::{ChatBackend, ChatResponse, Message};
use crate::ai::plugin_headless::{
    authorize_headless_host, execute_source_with_test_host, HeadlessHost,
};
use crate::ai::tool_executor::ToolExecutor;
use crate::ai::tools::ToolSpec;
use crate::ai::workflow::commands::{history, storage as workflow_storage};
use crate::ai::workflow::{
    execute_workflow, Workflow, WorkflowExecutionStatus, WorkflowStep, WorkflowToolRunner,
};
use crate::core::permission::{
    enforce_background, store, Grant, PermissionEngine, Scope,
};
use crate::error::{Result, VoloError};

use super::storage::{claim_due_automations, list_automations, save_automation};
use super::{AutomationTrigger, WorkflowAutomation};

const PLUGIN_ID: &str = "m1-smoke-plugin";
const PLUGIN_TOOL: &str = "plugin__m1-smoke-plugin__read__smoke";
const PLUGIN_SOURCE: &str = r#"
rubick.tool.onInvoke(async function (input) {
  const items = await rubick.fs.list(input.dir);
  const text = await rubick.fs.read(input.file);
  return {
    files: items.map(function (item) { return item.name; }).sort(),
    text: text
  };
});
"#;

struct SmokeHeadlessHost {
    engine: Arc<PermissionEngine>,
    principal: String,
    permissions: Vec<String>,
}

impl SmokeHeadlessHost {
    fn new(
        engine: Arc<PermissionEngine>,
        principal: impl Into<String>,
        permissions: Vec<String>,
    ) -> Self {
        Self {
            engine,
            principal: principal.into(),
            permissions,
        }
    }

    fn string_arg<'a>(args: &'a Value, name: &str, method: &str) -> Result<&'a str> {
        args.get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                VoloError::Other(format!(
                    "M1 smoke host method '{}' requires string argument '{}'",
                    method, name
                ))
            })
    }

    fn authorize(&self, resource: &str) -> Result<()> {
        authorize_headless_host(
            &self.engine,
            &self.principal,
            PLUGIN_ID,
            &self.permissions,
            "fs.read",
            Some(resource),
        )
    }

    fn call(&self, method: &str, args: Value) -> Result<Value> {
        match method {
            "fs.read" => {
                let path = Self::string_arg(&args, "path", method)?;
                let resolved = crate::api::fs::canonicalize_existing_plugin_path(path)?;
                let resource = resolved.to_string_lossy().into_owned();
                self.authorize(&resource)?;
                Ok(Value::String(std::fs::read_to_string(resolved)?))
            }
            "fs.list" => {
                let path = Self::string_arg(&args, "path", method)?;
                let resolved = crate::api::fs::canonicalize_existing_plugin_path(path)?;
                let resource = resolved.to_string_lossy().into_owned();
                self.authorize(&resource)?;

                let mut items = Vec::new();
                for entry in std::fs::read_dir(resolved)? {
                    let entry = entry?;
                    items.push(json!({
                        "name": entry.file_name().to_string_lossy(),
                    }));
                }
                Ok(Value::Array(items))
            }
            _ => Err(VoloError::Other(format!(
                "M1 smoke host does not support method: {}",
                method
            ))),
        }
    }
}

impl HeadlessHost for SmokeHeadlessHost {
    fn call_envelope(&self, method: String, args_json: String) -> String {
        let result = serde_json::from_str::<Value>(&args_json)
            .map_err(VoloError::from)
            .and_then(|args| self.call(&method, args));

        match result {
            Ok(data) => serde_json::to_string(&json!({
                "ok": true,
                "data": data,
            }))
            .unwrap(),
            Err(error) => serde_json::to_string(&json!({
                "ok": false,
                "error": error.to_string(),
            }))
            .unwrap(),
        }
    }
}

struct SmokeToolExecutor {
    host: Arc<SmokeHeadlessHost>,
    notifications: Arc<Mutex<Vec<String>>>,
}

impl ToolExecutor for SmokeToolExecutor {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            match name {
                PLUGIN_TOOL => {
                    execute_source_with_test_host(
                        PLUGIN_SOURCE,
                        args,
                        Duration::from_secs(2),
                        self.host.clone(),
                    )
                    .await
                }
                "notification_show" => {
                    enforce_background(
                        &self.host.engine,
                        &self.host.principal,
                        "notification.show",
                        None,
                    )?;
                    let body = args
                        .get("body")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            VoloError::Other(
                                "M1 smoke notification requires body".to_string(),
                            )
                        })?;
                    self.notifications
                        .lock()
                        .map_err(|_| VoloError::Other("notification lock poisoned".to_string()))?
                        .push(body.to_string());
                    Ok(Value::String("notification-recorded".to_string()))
                }
                _ => Err(VoloError::NotFound(format!("smoke tool: {}", name))),
            }
        })
    }
}

struct SmokeChatBackend {
    seen_messages: Arc<Mutex<Vec<String>>>,
}

impl ChatBackend for SmokeChatBackend {
    fn chat<'a>(
        &'a self,
        messages: &'a [Message],
        _tools: &'a [ToolSpec],
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponse>> + Send + 'a>> {
        Box::pin(async move {
            let rendered = messages
                .iter()
                .filter_map(|message| message.content.as_deref())
                .collect::<Vec<_>>()
                .join("\n");
            self.seen_messages
                .lock()
                .map_err(|_| VoloError::Other("AI message lock poisoned".to_string()))?
                .push(rendered);

            Ok(ChatResponse {
                content: Some("m1-summary".to_string()),
                tool_calls: Vec::new(),
            })
        })
    }
}

fn test_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "volo-m1-automation-smoke-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 19, hour, minute, 0)
        .single()
        .unwrap()
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn permission_engine(
    root: &Path,
    name: &str,
    grants: &[Grant],
) -> Arc<PermissionEngine> {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let store_path = dir.join("permissions.json");
    store::save_grants(&store_path, grants).unwrap();
    Arc::new(
        PermissionEngine::new(
            store_path,
            dir.join("audit.db"),
            Duration::from_millis(50),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn m1_real_background_contract_closes_the_loop() {
    let root = test_root();
    let automations_dir = root.join("automations");
    let workflows_dir = root.join("workflows");
    let runs_dir = root.join("workflow-runs");
    let input_dir = root.join("input");
    std::fs::create_dir_all(&input_dir).unwrap();

    let input_file = input_dir.join("input.txt");
    let sibling_file = input_dir.join("sibling.txt");
    std::fs::write(&input_file, "M1 secret payload").unwrap();
    std::fs::write(&sibling_file, "visible in directory listing").unwrap();

    let canonical_dir = crate::api::fs::canonicalize_existing_plugin_path(
        input_dir.to_string_lossy().as_ref(),
    )
    .unwrap();
    let canonical_file = crate::api::fs::canonicalize_existing_plugin_path(
        input_file.to_string_lossy().as_ref(),
    )
    .unwrap();
    let dir_resource = normalized(&canonical_dir);
    let file_resource = normalized(&canonical_file);
    let principal = "workflow:m1-real-loop";

    let grants = vec![
        Grant {
            principal: principal.to_string(),
            capability: "fs.read".to_string(),
            resource: Some(dir_resource.clone()),
            scope: Scope::Always,
        },
        Grant {
            principal: principal.to_string(),
            capability: "fs.read".to_string(),
            resource: Some(file_resource.clone()),
            scope: Scope::Always,
        },
    ];
    let allowed_engine = permission_engine(&root, "allowed-permissions", &grants);
    let missing_grant_engine = permission_engine(&root, "missing-permissions", &[]);
    let manifest_permissions = vec![
        format!("fs.read:{}", dir_resource),
        format!("fs.read:{}", file_resource),
    ];
    let plugin_args = json!({
        "dir": canonical_dir.to_string_lossy(),
        "file": canonical_file.to_string_lossy(),
    });

    let undeclared_host: Arc<dyn HeadlessHost> = Arc::new(SmokeHeadlessHost::new(
        allowed_engine.clone(),
        principal,
        Vec::new(),
    ));
    let error = execute_source_with_test_host(
        PLUGIN_SOURCE,
        plugin_args.clone(),
        Duration::from_secs(2),
        undeclared_host,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("does not declare permission"));

    let missing_grant_host: Arc<dyn HeadlessHost> = Arc::new(SmokeHeadlessHost::new(
        missing_grant_engine,
        principal,
        manifest_permissions.clone(),
    ));
    let error = execute_source_with_test_host(
        PLUGIN_SOURCE,
        plugin_args.clone(),
        Duration::from_secs(2),
        missing_grant_host,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("requires an Always grant"));

    let workflow = Workflow {
        id: "m1-real-loop".to_string(),
        name: "M1 Real Background Loop".to_string(),
        steps: vec![
            WorkflowStep::Tool {
                id: "read-local".to_string(),
                name: PLUGIN_TOOL.to_string(),
                args: plugin_args,
            },
            WorkflowStep::Ai {
                id: "summarize".to_string(),
                prompt: "Summarize the local read result for the notification".to_string(),
            },
            WorkflowStep::Tool {
                id: "notify".to_string(),
                name: "notification_show".to_string(),
                args: json!({
                    "title": "Volo M1",
                    "body": "${previous}",
                }),
            },
        ],
    };
    workflow_storage::save_workflow(&workflows_dir, &workflow).unwrap();

    let automation = WorkflowAutomation {
        id: "m1-real-loop-job".to_string(),
        workflow_id: workflow.id.clone(),
        enabled: true,
        trigger: AutomationTrigger::Interval {
            every_minutes: 15,
        },
        retry_policy: None,
    };
    save_automation(&automations_dir, automation, at(12, 0)).unwrap();

    let claims = claim_due_automations(&automations_dir, at(12, 15)).unwrap();
    assert_eq!(claims.len(), 1);
    let claim = &claims[0];
    assert_eq!(claim.scheduled_for, at(12, 15));
    assert_eq!(claim.retry_attempt, None);
    assert!(claim_due_automations(&automations_dir, at(12, 15))
        .unwrap()
        .is_empty());
    assert_eq!(
        list_automations(&automations_dir).unwrap()[0]
            .parsed_next_run()
            .unwrap(),
        Some(at(12, 30))
    );

    let persisted = workflow_storage::load_workflow(
        &workflows_dir,
        &claim.automation.workflow_id,
    )
    .unwrap();

    let host = Arc::new(SmokeHeadlessHost::new(
        allowed_engine,
        principal,
        manifest_permissions,
    ));
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let executor = SmokeToolExecutor {
        host,
        notifications: notifications.clone(),
    };
    let seen_messages = Arc::new(Mutex::new(Vec::new()));
    let backend = SmokeChatBackend {
        seen_messages: seen_messages.clone(),
    };
    let runner = WorkflowToolRunner::with_backend_timeout(
        &executor,
        &backend,
        Duration::from_secs(1),
    );

    let execution = execute_workflow(&persisted, Value::Null, &runner)
        .await
        .unwrap();
    assert_eq!(execution.status, WorkflowExecutionStatus::Completed);
    assert_eq!(execution.steps.len(), 3);
    assert_eq!(execution.output, Some(Value::String("notification-recorded".to_string())));

    let ai_messages = seen_messages.lock().unwrap();
    assert_eq!(ai_messages.len(), 1);
    assert!(ai_messages[0].contains("M1 secret payload"));
    assert!(ai_messages[0].contains("input.txt"));
    drop(ai_messages);

    assert_eq!(
        notifications.lock().unwrap().as_slice(),
        ["m1-summary".to_string()]
    );

    let finished_at = claim.scheduled_for + chrono::Duration::milliseconds(12);
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
        12,
    );
    history::record_run(&runs_dir, &record).unwrap();

    let runs = history::list_runs(&runs_dir, Some(&persisted.id)).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].source, history::WorkflowRunSource::Automation);
    assert_eq!(runs[0].automation_id.as_deref(), Some("m1-real-loop-job"));
    assert_eq!(runs[0].scheduled_for.as_deref(), Some("2026-09-19T12:15:00.000Z"));
    assert_eq!(runs[0].retry_attempt, None);
    assert_eq!(runs[0].status, WorkflowExecutionStatus::Completed);
    assert_eq!(runs[0].steps.len(), 3);

    let persisted_history = serde_json::to_string(&runs[0]).unwrap();
    assert!(!persisted_history.contains("M1 secret payload"));
    assert!(!persisted_history.contains("m1-summary"));

    std::fs::remove_dir_all(root).unwrap();
}
