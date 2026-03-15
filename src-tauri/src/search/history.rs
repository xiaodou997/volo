//! 搜索历史模块
//! 记录用户选择的应用，用于搜索排序

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use crate::error::{Result, VoloError};

/// 搜索历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHistory {
    pub app_path: String,
    pub count: u32,
    pub last_used: i64,
}

/// 搜索历史管理器
pub struct SearchHistoryManager {
    conn: Mutex<Connection>,
}

impl SearchHistoryManager {
    /// 创建新的搜索历史管理器
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS search_history (
                app_path TEXT PRIMARY KEY,
                count INTEGER NOT NULL DEFAULT 1,
                last_used INTEGER NOT NULL
            )",
            [],
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 记录应用使用
    pub fn record_usage(&self, app_path: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO search_history (app_path, count, last_used) 
             VALUES (?1, 1, ?2)
             ON CONFLICT(app_path) DO UPDATE SET 
             count = count + 1, 
             last_used = ?2",
            params![app_path, now],
        )?;

        Ok(())
    }

    /// 获取应用的使用次数
    pub fn get_usage_count(&self, app_path: &str) -> u32 {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return 0,
        };

        let result = conn.query_row(
            "SELECT count FROM search_history WHERE app_path = ?1",
            params![app_path],
            |row| row.get::<_, u32>(0),
        );

        result.unwrap_or(0)
    }

    /// 获取所有历史记录
    pub fn get_all(&self) -> Result<Vec<SearchHistory>> {
        let conn = self.conn.lock()
            .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

        let mut stmt = conn.prepare(
            "SELECT app_path, count, last_used FROM search_history ORDER BY count DESC"
        )?;

        let history = stmt.query_map([], |row| {
            Ok(SearchHistory {
                app_path: row.get(0)?,
                count: row.get(1)?,
                last_used: row.get(2)?,
            })
        })?
        .filter_map(|h| h.ok())
        .collect();

        Ok(history)
    }

    /// 清空历史记录
    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

        conn.execute("DELETE FROM search_history", [])?;

        Ok(())
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn record_app_usage(
    manager: tauri::State<'_, SearchHistoryManager>,
    app_path: String,
) -> Result<()> {
    manager.record_usage(&app_path)
}

#[tauri::command]
pub fn get_search_history(
    manager: tauri::State<'_, SearchHistoryManager>,
) -> Result<Vec<SearchHistory>> {
    manager.get_all()
}

#[tauri::command]
pub fn clear_search_history(
    manager: tauri::State<'_, SearchHistoryManager>,
) -> Result<()> {
    manager.clear()
}