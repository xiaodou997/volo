//! 授权持久化
//! 将 Always 范围的授权读写 config_dir/permissions.json

use std::path::Path;
use crate::error::Result;
use super::engine::Grant;

/// 从 JSON 文件加载授权；文件不存在或损坏时返回空列表
pub fn load_grants(path: &Path) -> Result<Vec<Grant>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(path)?;
    let grants: Vec<Grant> = serde_json::from_str(&content).unwrap_or_default();
    Ok(grants)
}

/// 将授权写入 JSON 文件（整体覆盖）
pub fn save_grants(path: &Path, grants: &[Grant]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(grants)?;
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::engine::Scope;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("volo_perm_test_{}_{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("permissions.json")
    }

    #[test]
    fn test_grants_persistence_roundtrip() {
        let path = temp_path("roundtrip");

        // 文件不存在时加载为空
        assert!(load_grants(&path).unwrap().is_empty());

        let grants = vec![
            Grant {
                principal: "plugin-a".to_string(),
                capability: "clipboard.read".to_string(),
                scope: Scope::Always,
            },
            Grant {
                principal: "plugin-b".to_string(),
                capability: "fs.write".to_string(),
                scope: Scope::Always,
            },
        ];
        save_grants(&path, &grants).unwrap();

        let loaded = load_grants(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|g| g.principal == "plugin-a"
            && g.capability == "clipboard.read"
            && g.scope == Scope::Always));
        assert!(loaded.iter().any(|g| g.principal == "plugin-b"
            && g.capability == "fs.write"
            && g.scope == Scope::Always));

        // 覆盖写：撤销后只剩一条
        save_grants(&path, &grants[..1]).unwrap();
        let loaded = load_grants(&path).unwrap();
        assert_eq!(loaded.len(), 1);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_corrupt_file_returns_empty() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "not json {{{").unwrap();
        assert!(load_grants(&path).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
