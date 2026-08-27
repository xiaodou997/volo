//! 搜索历史模块
//! 记录用户选择的结果项（应用/插件功能/命令），用于搜索排序
//!
//! item_key 约定（共用一张表，命名空间天然不冲突）：
//! - 应用：app path（/ 开头，历史数据沿用此 key）
//! - 插件功能/命令：`{plugin_id}#{feature_or_command_id}`

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use crate::error::{Result, VoloError};

/// 搜索历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHistory {
    pub item_key: String,
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

        // 列名 app_path 是历史遗留，实际存任意 item_key；不改表结构，零迁移
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

    /// 记录结果项使用
    pub fn record_usage(&self, item_key: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|_| VoloError::Other("Database lock error".to_string()))?;

        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO search_history (app_path, count, last_used) 
             VALUES (?1, 1, ?2)
             ON CONFLICT(app_path) DO UPDATE SET 
             count = count + 1, 
             last_used = ?2",
            params![item_key, now],
        )?;

        Ok(())
    }

    /// 获取结果项的使用次数
    pub fn get_usage_count(&self, item_key: &str) -> u32 {
        self.get_stats(item_key).map(|(count, _)| count).unwrap_or(0)
    }

    /// 获取 (使用次数, 最近使用时间)；无记录返回 None
    fn get_stats(&self, item_key: &str) -> Option<(u32, i64)> {
        let conn = self.conn.lock().ok()?;

        conn.query_row(
            "SELECT count, last_used FROM search_history WHERE app_path = ?1",
            params![item_key],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?)),
        )
        .ok()
    }

    /// frecency 分数：频率加成（封顶 20）+ 时间衰减加成
    /// （24h 内用过 +10，7 天内 +5），让"最近常用"排得更靠前
    pub fn get_frecency(&self, item_key: &str) -> f64 {
        let Some((count, last_used)) = self.get_stats(item_key) else {
            return 0.0;
        };

        let age_secs = chrono::Utc::now().timestamp() - last_used;
        let recency_bonus = if age_secs < 24 * 3600 {
            10.0
        } else if age_secs < 7 * 24 * 3600 {
            5.0
        } else {
            0.0
        };

        (count as f64).min(20.0) + recency_bonus
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
                item_key: row.get(0)?,
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

/// 记录一次结果项选择（应用传 path，插件功能/命令传 `{plugin_id}#{id}`）
#[tauri::command]
pub fn record_item_usage(
    manager: tauri::State<'_, SearchHistoryManager>,
    key: String,
) -> Result<()> {
    manager.record_usage(&key)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DB_SEQ: AtomicU64 = AtomicU64::new(0);

    /// 每个测试用独立的临时库文件（测试并行运行，不能共享）
    fn temp_manager() -> SearchHistoryManager {
        let seq = DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "volo-history-test-{}-{}.db",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_file(&path);
        SearchHistoryManager::new(&path).unwrap()
    }

    #[test]
    fn test_record_and_count_any_key_kind() {
        let m = temp_manager();
        // 应用 key（path）与插件 key（plugin#id）共存互不干扰
        m.record_usage("/Applications/WeChat.app").unwrap();
        m.record_usage("/Applications/WeChat.app").unwrap();
        m.record_usage("uuid-gen#gen-uuid").unwrap();

        assert_eq!(m.get_usage_count("/Applications/WeChat.app"), 2);
        assert_eq!(m.get_usage_count("uuid-gen#gen-uuid"), 1);
        assert_eq!(m.get_usage_count("不存在"), 0);
    }

    #[test]
    fn test_frecency_never_used_is_zero() {
        let m = temp_manager();
        assert_eq!(m.get_frecency("没用过"), 0.0);
    }

    #[test]
    fn test_frecency_combines_count_and_recency() {
        let m = temp_manager();
        // 刚用过（24h 内）：count 1 + 近期加成 10
        m.record_usage("a#b").unwrap();
        let score = m.get_frecency("a#b");
        assert_eq!(score, 11.0);

        // 频率加成封顶 20：25 次使用 + 近期加成 = 30
        for _ in 0..24 {
            m.record_usage("a#b").unwrap();
        }
        assert_eq!(m.get_frecency("a#b"), 30.0);
    }

    #[test]
    fn test_clear() {
        let m = temp_manager();
        m.record_usage("x").unwrap();
        m.clear().unwrap();
        assert_eq!(m.get_usage_count("x"), 0);
    }
}