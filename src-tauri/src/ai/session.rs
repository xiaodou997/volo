//! Agent 会话事件日志
//! 每会话一个 JSONL 文件（`<session_id>.jsonl`），每行 `{ ts, kind, payload }`，
//! 用于审计、调试与回放。目录：`app_data_dir/sessions/`。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::error::{Result, VoloError};

/// 旧日志保留天数
pub const SESSION_RETENTION_DAYS: u64 = 30;

/// 单个会话的事件日志（追加写 JSONL）
pub struct SessionLog {
    file: File,
    pub session_id: String,
}

impl SessionLog {
    /// 在 sessions_dir 下创建新会话日志文件，目录不存在则先创建。
    /// session_id = 时间戳 + 短 uuid，如 `20260816-121651-a1b2c3d4`
    pub fn create(sessions_dir: &Path) -> Result<Self> {
        fs::create_dir_all(sessions_dir)?;
        let session_id = format!(
            "{}-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(sessions_dir.join(format!("{}.jsonl", session_id)))?;
        Ok(Self { file, session_id })
    }

    /// 追加一行事件：`{ "ts": <RFC3339>, "kind": ..., "payload": ... }`
    pub fn log(&mut self, kind: &str, payload: &impl Serialize) -> Result<()> {
        let line = json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "kind": kind,
            "payload": payload,
        });
        writeln!(self.file, "{}", line)?;
        // 立即落盘：会话异常退出时日志不丢
        self.file.flush()?;
        Ok(())
    }
}

/// 删除 sessions_dir 下 mtime 早于 max_age_days 的 .jsonl 文件，返回删除数量。
/// 单个文件失败不影响整体清理。
pub fn cleanup_old_sessions(dir: &Path, max_age_days: u64) -> Result<usize> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(max_age_days * 24 * 3600))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else { continue };
        let Ok(modified) = metadata.modified() else { continue };
        if modified < cutoff && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// 会话日志目录：`app_data_dir/sessions/`
pub fn sessions_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| VoloError::Other(format!("app_data_dir unavailable: {}", e)))?
        .join("sessions"))
}

/// 打开会话日志目录（目录不存在则先创建）
#[tauri::command]
pub fn open_sessions_dir(app: AppHandle) -> Result<()> {
    let dir = sessions_dir(&app)?;
    fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<String>)
        .map_err(|e| VoloError::Other(format!("open sessions dir failed: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn temp_sessions_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "volo-session-test-{}-{}",
            tag,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_log_roundtrip() {
        let dir = temp_sessions_dir("roundtrip");
        let mut log = SessionLog::create(&dir).unwrap();

        log.log("user_input", &json!({"query": "你好"})).unwrap();
        log.log("done", &json!({})).unwrap();

        let path = dir.join(format!("{}.jsonl", log.session_id));
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["kind"], "user_input");
        assert_eq!(first["payload"]["query"], "你好");
        assert!(first["ts"].as_str().unwrap().contains('T'));

        let second: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["kind"], "done");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_create_makes_dir() {
        let base = temp_sessions_dir("mkdir");
        let nested = base.join("a/b/c");
        let log = SessionLog::create(&nested).unwrap();
        assert!(nested.join(format!("{}.jsonl", log.session_id)).exists());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_cleanup_removes_old_keeps_recent() {
        let dir = temp_sessions_dir("cleanup");

        // 一个旧文件（mtime 31 天前）、一个新文件、一个非 jsonl 旧文件
        let old = dir.join("old.jsonl");
        let recent = dir.join("recent.jsonl");
        let not_jsonl = dir.join("note.txt");
        fs::write(&old, "{}").unwrap();
        fs::write(&recent, "{}").unwrap();
        fs::write(&not_jsonl, "x").unwrap();

        let old_time = SystemTime::now() - Duration::from_secs(31 * 24 * 3600);
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        File::options()
            .write(true)
            .open(&not_jsonl)
            .unwrap()
            .set_modified(old_time)
            .unwrap();

        let removed = cleanup_old_sessions(&dir, SESSION_RETENTION_DAYS).unwrap();
        assert_eq!(removed, 1);
        assert!(!old.exists());
        assert!(recent.exists());
        assert!(not_jsonl.exists()); // 非 jsonl 不动

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cleanup_missing_dir_is_ok() {
        let dir = std::env::temp_dir().join(format!("volo-no-such-{}", uuid::Uuid::new_v4()));
        assert_eq!(cleanup_old_sessions(&dir, 30).unwrap(), 0);
    }
}
