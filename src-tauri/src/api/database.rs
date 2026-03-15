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
                rev TEXT,
                data TEXT NOT NULL,
                updated_at INTEGER
            )",
            [],
        )?;
        
        Ok(Self { 
            conn: Mutex::new(conn) 
        })
    }
}

#[tauri::command]
pub fn db_put(db: State<'_, Database>, id: String, data: serde_json::Value) -> Result<Doc> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;
    
    let rev = uuid::Uuid::new_v4().to_string();
    let data_str = serde_json::to_string(&data)?;
    let updated_at = chrono::Utc::now().timestamp();
    
    conn.execute(
        "INSERT OR REPLACE INTO docs (id, rev, data, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, &rev, data_str, updated_at],
    )?;
    
    Ok(Doc {
        _id: id,
        _rev: Some(rev),
        data,
        updated_at: Some(updated_at),
    })
}

#[tauri::command]
pub fn db_get(db: State<'_, Database>, id: String) -> Result<Option<Doc>> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;
    
    let mut stmt = conn.prepare(
        "SELECT id, rev, data, updated_at FROM docs WHERE id = ?1"
    )?;
    
    let result = stmt.query_row(params![id], |row| {
        Ok(Doc {
            _id: row.get(0)?,
            _rev: row.get(1)?,
            data: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or(serde_json::Value::Null),
            updated_at: row.get(3)?,
        })
    }).optional()?;
    
    Ok(result)
}

#[tauri::command]
pub fn db_remove(db: State<'_, Database>, id: String) -> Result<()> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;
    
    conn.execute("DELETE FROM docs WHERE id = ?1", params![id])?;
    
    Ok(())
}

#[tauri::command]
pub fn db_all(db: State<'_, Database>) -> Result<Vec<Doc>> {
    let conn = db.conn.lock()
        .map_err(|_| VoloError::Other("Database lock error".to_string()))?;
    
    let mut stmt = conn.prepare(
        "SELECT id, rev, data, updated_at FROM docs ORDER BY updated_at DESC"
    )?;
    
    let docs = stmt.query_map([], |row| {
        Ok(Doc {
            _id: row.get(0)?,
            _rev: row.get(1)?,
            data: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or(serde_json::Value::Null),
            updated_at: row.get(3)?,
        })
    })?
    .filter_map(|d| d.ok())
    .collect();
    
    Ok(docs)
}