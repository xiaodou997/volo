//! 数据库 API

use rusqlite::{Connection, params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use tauri::State;
use crate::error::{Result, VoloError};

/// 文档结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Doc {
    pub _id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _rev: Option<String>,
    pub data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

/// 数据库
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS docs (
                id TEXT PRIMARY KEY,
                plugin_id TEXT NOT NULL,
                rev TEXT,
                data TEXT NOT NULL,
                updated_at INTEGER
            )",
            [],
        )?;

        // 创建索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_plugin_id ON docs(plugin_id)",
            [],
        )?;

        Ok(Self {
            conn: Mutex::new(conn)
        })
    }
}

/// 生成带插件前缀的 ID
fn make_id(plugin_id: &str, id: &str) -> String {
    format!("{}:{}", plugin_id, id)
}

/// 存储文档
#[tauri::command]
pub fn db_put(
    db: State<'_, Database>,
    plugin_id: String,
    id: String,
    data: serde_json::Value,
) -> Result<Doc> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

    let full_id = make_id(&plugin_id, &id);
    let rev = uuid::Uuid::new_v4().to_string();
    let data_str = serde_json::to_string(&data)?;
    let updated_at = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT OR REPLACE INTO docs (id, plugin_id, rev, data, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![full_id, plugin_id, &rev, data_str, updated_at],
    )?;

    Ok(Doc {
        _id: id,  // 返回原始 ID，不带前缀
        _rev: Some(rev),
        data,
        updated_at: Some(updated_at),
    })
}

/// 获取文档
#[tauri::command]
pub fn db_get(
    db: State<'_, Database>,
    plugin_id: String,
    id: String,
) -> Result<Option<Doc>> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

    let full_id = make_id(&plugin_id, &id);

    let mut stmt = conn.prepare(
        "SELECT id, rev, data, updated_at FROM docs WHERE id = ?1"
    )?;

    let result = stmt.query_row(params![full_id], |row| {
        // 从完整 ID 中提取原始 ID
        let full_id: String = row.get(0)?;
        let original_id = full_id.split(':').nth(1).unwrap_or(&full_id).to_string();

        Ok(Doc {
            _id: original_id,
            _rev: row.get(1)?,
            data: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or(serde_json::Value::Null),
            updated_at: row.get(3)?,
        })
    }).optional()?;

    Ok(result)
}

/// 删除文档
#[tauri::command]
pub fn db_remove(
    db: State<'_, Database>,
    plugin_id: String,
    id: String,
) -> Result<()> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

    let full_id = make_id(&plugin_id, &id);
    conn.execute("DELETE FROM docs WHERE id = ?1", params![full_id])?;

    Ok(())
}

/// 获取插件所有文档
#[tauri::command]
pub fn db_all(
    db: State<'_, Database>,
    plugin_id: String,
) -> Result<Vec<Doc>> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, rev, data, updated_at FROM docs WHERE plugin_id = ?1 ORDER BY updated_at DESC"
    )?;

    let docs = stmt.query_map(params![plugin_id], |row| {
        // 从完整 ID 中提取原始 ID
        let full_id: String = row.get(0)?;
        let original_id = full_id.split(':').nth(1).unwrap_or(&full_id).to_string();

        Ok(Doc {
            _id: original_id,
            _rev: row.get(1)?,
            data: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or(serde_json::Value::Null),
            updated_at: row.get(3)?,
        })
    })?
    .filter_map(|d| d.ok())
    .collect();

    Ok(docs)
}