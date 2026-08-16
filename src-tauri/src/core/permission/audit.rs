//! 审计日志
//! Medium 及以上风险等级的权限决策写入 app_data_dir/audit.db

use rusqlite::{params, Connection};
use std::path::Path;
use crate::error::Result;

pub struct AuditLog {
    conn: Connection,
}

impl AuditLog {
    /// 打开（或创建）审计数据库
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                principal TEXT NOT NULL,
                capability TEXT NOT NULL,
                resource TEXT,
                decision TEXT NOT NULL,
                scope TEXT
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    /// 记录一条决策
    pub fn record(
        &self,
        principal: &str,
        capability: &str,
        resource: Option<&str>,
        decision: &str,
        scope: Option<&str>,
    ) -> Result<()> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO audit_log (timestamp, principal, capability, resource, decision, scope)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![timestamp, principal, capability, resource, decision, scope],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_write_and_read() {
        let dir = std::env::temp_dir().join(format!("volo_audit_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.db");

        let log = AuditLog::open(&path).unwrap();
        log.record("plugin-a", "fs.write", Some("/tmp/x"), "allow", Some("session")).unwrap();
        log.record("plugin-b", "clipboard.read", None, "deny", None).unwrap();

        let count: i64 = log
            .conn
            .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);

        let decision: String = log
            .conn
            .query_row(
                "SELECT decision FROM audit_log WHERE principal = 'plugin-b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(decision, "deny");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
