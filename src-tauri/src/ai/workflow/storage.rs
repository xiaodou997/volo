use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use tauri::{AppHandle, Manager};

use crate::error::{Result, VoloError};

use super::{validate_workflow, Workflow};

const MAX_PERSISTED_WORKFLOW_ID_BYTES: usize = 128;

/// Workflow 定义目录：`app_data_dir/workflows/`。
pub fn workflows_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| VoloError::Other(format!("app_data_dir unavailable: {}", error)))?
        .join("workflows"))
}

/// 用户提供的 workflow id 不直接作为文件名，避免目录穿越和平台非法字符。
fn workflow_file_name(workflow_id: &str) -> Result<String> {
    if workflow_id.as_bytes().len() > MAX_PERSISTED_WORKFLOW_ID_BYTES {
        return Err(VoloError::Other(format!(
            "workflow id 过长，最多 {} 字节",
            MAX_PERSISTED_WORKFLOW_ID_BYTES
        )));
    }
    Ok(format!(
        "{}.json",
        URL_SAFE_NO_PAD.encode(workflow_id.as_bytes())
    ))
}

fn workflow_path(dir: &Path, workflow_id: &str) -> Result<PathBuf> {
    Ok(dir.join(workflow_file_name(workflow_id)?))
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
fn write_workflow_file(path: &Path, content: &[u8]) -> Result<()> {
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

        // 尽力同步目录中的 rename 元数据；不支持目录 fsync 的文件系统不阻断保存。
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
fn write_workflow_file(path: &Path, content: &[u8]) -> Result<()> {
    let temp_path = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;

        // Windows 的 std::fs::rename 不能覆盖现有文件。先删除旧文件再替换，
        // 仍然避免直接 truncate 目标文件造成半写 JSON。
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

pub fn save_workflow(dir: &Path, workflow: &Workflow) -> Result<()> {
    validate_workflow(workflow)?;
    fs::create_dir_all(dir)?;
    harden_dir(dir)?;

    let path = workflow_path(dir, &workflow.id)?;
    let content = serde_json::to_vec_pretty(workflow)?;
    write_workflow_file(&path, &content)
}

/// 列出所有可读取的 Workflow。
///
/// 单个损坏/旧格式文件不会让整个列表不可用；记录 warning 后跳过，方便用户仍能管理其余定义。
pub fn list_workflows(dir: &Path) -> Result<Vec<Workflow>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut workflows = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        let workflow = match fs::read_to_string(&path)
            .map_err(VoloError::from)
            .and_then(|content| serde_json::from_str::<Workflow>(&content).map_err(VoloError::from))
            .and_then(|workflow| {
                validate_workflow(&workflow)?;
                Ok(workflow)
            }) {
            Ok(workflow) => workflow,
            Err(error) => {
                tracing::warn!(path = %path.display(), "skip invalid workflow file: {}", error);
                continue;
            }
        };
        workflows.push(workflow);
    }

    workflows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(workflows)
}

pub fn delete_workflow(dir: &Path, workflow_id: &str) -> Result<()> {
    let path = workflow_path(dir, workflow_id)?;
    if !path.is_file() {
        return Err(VoloError::NotFound(format!("workflow: {}", workflow_id)));
    }
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "volo-workflow-storage-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn workflow(id: &str, name: &str) -> Workflow {
        Workflow {
            id: id.to_string(),
            name: name.to_string(),
            steps: vec![super::super::WorkflowStep::Tool {
                id: "read".to_string(),
                name: "clipboard_read".to_string(),
                args: json!({}),
            }],
        }
    }

    #[test]
    fn encoded_filename_never_exposes_path_segments() {
        let name = workflow_file_name("../nested/workflow").unwrap();
        assert!(name.ends_with(".json"));
        assert!(!name.contains('/'));
        assert!(!name.contains(".."));
    }

    #[test]
    fn save_list_overwrite_and_delete_round_trip() {
        let dir = test_dir();
        let first = workflow("daily-note", "Daily Note");
        save_workflow(&dir, &first).unwrap();

        let listed = list_workflows(&dir).unwrap();
        assert_eq!(listed, vec![first.clone()]);

        let mut updated = first.clone();
        updated.name = "Daily Note Updated".to_string();
        save_workflow(&dir, &updated).unwrap();
        assert_eq!(list_workflows(&dir).unwrap(), vec![updated]);

        delete_workflow(&dir, "daily-note").unwrap();
        assert!(list_workflows(&dir).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_workflow_is_not_persisted() {
        let dir = test_dir();
        let invalid = Workflow {
            id: "invalid".to_string(),
            name: "Invalid".to_string(),
            steps: vec![],
        };
        assert!(save_workflow(&dir, &invalid).is_err());
        assert!(list_workflows(&dir).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_does_not_break_listing() {
        let dir = test_dir();
        let valid = workflow("valid", "Valid");
        save_workflow(&dir, &valid).unwrap();
        fs::write(dir.join("broken.json"), "{not-json").unwrap();

        assert_eq!(list_workflows(&dir).unwrap(), vec![valid]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persisted_id_has_bounded_filename_length() {
        let id = "x".repeat(MAX_PERSISTED_WORKFLOW_ID_BYTES + 1);
        assert!(workflow_file_name(&id).unwrap_err().to_string().contains("过长"));
    }
}
