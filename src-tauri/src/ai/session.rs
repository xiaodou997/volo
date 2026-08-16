//! Agent 会话事件日志
//! 每会话一个 JSONL 文件（`<session_id>.jsonl`），每行 `{ ts, kind, payload }`，
//! 用于审计、调试与回放。目录：`app_data_dir/sessions/`。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::{json, Value};
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

/// 会话列表上限
const MAX_LISTED_SESSIONS: usize = 50;

/// 会话列表项（camelCase：id/startedAt/preview）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: String,
    pub started_at: String,
    pub preview: String,
}

/// 回放事件（camelCase，kind ∈ user|message|tool_call|tool_result|error）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayEvent {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

impl ReplayEvent {
    fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            content: None,
            name: None,
            args: None,
            result: None,
        }
    }
}

/// 列出会话日志（前端历史列表用）
#[tauri::command]
pub fn agent_list_sessions(app: AppHandle) -> Result<Vec<SessionMeta>> {
    list_sessions(&sessions_dir(&app)?)
}

/// 读取单个会话日志并映射为回放事件流
#[tauri::command]
pub fn agent_read_session(app: AppHandle, session_id: String) -> Result<Vec<ReplayEvent>> {
    read_session(&sessions_dir(&app)?, &session_id)
}

/// 扫 dir 下 .jsonl，按文件名倒序（时间戳前缀即时间序），上限 MAX_LISTED_SESSIONS 条
fn list_sessions(dir: &Path) -> Result<Vec<SessionMeta>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".jsonl") {
            names.push(name);
        }
    }
    names.sort_unstable_by(|a, b| b.cmp(a));
    names.truncate(MAX_LISTED_SESSIONS);

    let mut metas = Vec::with_capacity(names.len());
    for name in names {
        let id = name.trim_end_matches(".jsonl").to_string();
        metas.push(SessionMeta {
            started_at: format_started_at(&id),
            preview: first_user_query(&dir.join(&name)),
            id,
        });
    }
    Ok(metas)
}

/// 文件名时间戳前缀 `yyyymmdd-hhmmss` → `yyyy-mm-dd hh:mm:ss`，解析失败返回原前缀
fn format_started_at(id: &str) -> String {
    let prefix = id.get(..15).unwrap_or(id);
    match chrono::NaiveDateTime::parse_from_str(prefix, "%Y%m%d-%H%M%S") {
        Ok(ts) => ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        Err(_) => prefix.to_string(),
    }
}

/// 文件里第一条 user_input 行的 payload.query，截断 50 字符
fn first_user_query(path: &Path) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return String::new();
    };
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["kind"] == "user_input" {
            if let Some(query) = v["payload"]["query"].as_str() {
                const MAX: usize = 50;
                if query.chars().count() <= MAX {
                    return query.to_string();
                }
                return format!("{}…", query.chars().take(MAX).collect::<String>());
            }
        }
    }
    String::new()
}

/// 逐行解析 JSONL 映射回放事件；坏行跳过不报错
fn read_session(dir: &Path, session_id: &str) -> Result<Vec<ReplayEvent>> {
    // 防目录穿越：session_id 只允许 [a-zA-Z0-9-]
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(VoloError::Other(format!("非法的 session_id: {}", session_id)));
    }
    let content = fs::read_to_string(dir.join(format!("{}.jsonl", session_id)))?;

    let mut events = Vec::new();
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let payload = &v["payload"];
        match v["kind"].as_str().unwrap_or("") {
            "user_input" => {
                let mut e = ReplayEvent::new("user");
                e.content = payload["query"].as_str().map(String::from);
                events.push(e);
            }
            "model_response" => {
                // 纯 tool_call 轮（content 空）不产回放气泡
                if let Some(content) = payload["content"].as_str() {
                    if !content.is_empty() {
                        let mut e = ReplayEvent::new("message");
                        e.content = Some(content.to_string());
                        events.push(e);
                    }
                }
            }
            "tool_call" => {
                let mut e = ReplayEvent::new("tool_call");
                e.name = payload["name"].as_str().map(String::from);
                e.args = payload.get("args").cloned();
                events.push(e);
            }
            "tool_result" => {
                let mut e = ReplayEvent::new("tool_result");
                e.name = payload["name"].as_str().map(String::from);
                e.result = payload["result"].as_str().map(String::from);
                events.push(e);
            }
            "error" => {
                let mut e = ReplayEvent::new("error");
                e.content = payload["message"].as_str().map(String::from);
                events.push(e);
            }
            // done 等其余 kind 不回放
            _ => {}
        }
    }
    Ok(events)
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

    /// 往 dir 写一个 fixture 会话日志
    fn write_fixture(dir: &Path, name: &str, lines: &[Value]) {
        let content = lines
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(dir.join(format!("{}.jsonl", name)), content).unwrap();
    }

    fn user_input(query: &str) -> Value {
        json!({"ts": "2026-08-16T10:00:00Z", "kind": "user_input", "payload": {"query": query}})
    }

    #[test]
    fn test_list_sessions_desc_preview_and_started_at() {
        let dir = temp_sessions_dir("list");
        write_fixture(&dir, "20260810-100000-aaaaaaaa", &[user_input("第一条会话")]);
        // 长 query 验证 50 字符截断；另有一个非 jsonl 文件应被忽略
        write_fixture(
            &dir,
            "20260811-113000-bbbbbbbb",
            &[user_input(&"很".repeat(60))],
        );
        fs::write(dir.join("note.txt"), "x").unwrap();

        let metas = list_sessions(&dir).unwrap();
        assert_eq!(metas.len(), 2);
        // 文件名倒序（新→旧）
        assert_eq!(metas[0].id, "20260811-113000-bbbbbbbb");
        assert_eq!(metas[0].started_at, "2026-08-11 11:30:00");
        assert_eq!(metas[0].preview.chars().count(), 51); // 50 字符 + 省略号
        assert!(metas[0].preview.ends_with('…'));
        assert_eq!(metas[1].id, "20260810-100000-aaaaaaaa");
        assert_eq!(metas[1].preview, "第一条会话");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_list_sessions_empty_dir() {
        let dir = std::env::temp_dir().join(format!("volo-no-such-{}", uuid::Uuid::new_v4()));
        assert!(list_sessions(&dir).unwrap().is_empty());
    }

    #[test]
    fn test_read_session_maps_events_and_skips_bad_lines() {
        let dir = temp_sessions_dir("read");
        let id = "20260816-121651-a1b2c3d4";
        let lines = vec![
            user_input("剪贴板里有什么"),
            json!({"kind": "model_response", "payload": {"content": null, "tool_calls": ["clipboard_read"]}}),
            json!({"kind": "tool_call", "payload": {"name": "clipboard_read", "args": {}}}),
            json!({"kind": "tool_result", "payload": {"name": "clipboard_read", "result": "你好"}}),
            json!({"kind": "model_response", "payload": {"content": "剪贴板里是：你好", "tool_calls": []}}),
            json!({"kind": "error", "payload": {"message": "出错了"}}),
            json!({"kind": "done", "payload": {"reason": "completed"}}),
        ];
        write_fixture(&dir, id, &lines);
        // 追加一行坏行，应被跳过
        let path = dir.join(format!("{}.jsonl", id));
        let mut content = fs::read_to_string(&path).unwrap();
        content.push_str("\n{not valid json");
        fs::write(&path, content).unwrap();

        let events = read_session(&dir, id).unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        // content 为 null 的 model_response 与 done 都不产回放事件
        assert_eq!(kinds, vec!["user", "tool_call", "tool_result", "message", "error"]);
        assert_eq!(events[0].content.as_deref(), Some("剪贴板里有什么"));
        assert_eq!(events[1].name.as_deref(), Some("clipboard_read"));
        assert_eq!(events[1].args, Some(json!({})));
        assert_eq!(events[2].result.as_deref(), Some("你好"));
        assert_eq!(events[3].content.as_deref(), Some("剪贴板里是：你好"));
        assert_eq!(events[4].content.as_deref(), Some("出错了"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_session_rejects_traversal() {
        let dir = temp_sessions_dir("traversal");
        for bad in ["../etc/passwd", "a/b", "..", "a.b", ""] {
            assert!(read_session(&dir, bad).is_err(), "应拒绝: {}", bad);
        }
        // 合法 id 但文件不存在 → io 错误
        assert!(read_session(&dir, "20260816-121651-notexist").is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
