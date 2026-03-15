//! 权限验证模块
//! 检查插件是否有权限调用特定的 API

use std::collections::HashSet;
use crate::error::{Result, VoloError};

/// 权限管理器
pub struct PermissionManager {
    /// 插件 ID
    plugin_id: String,
    /// 声明的权限
    permissions: HashSet<String>,
}

impl PermissionManager {
    /// 创建新的权限管理器
    pub fn new(plugin_id: String, permissions: Vec<String>) -> Self {
        Self {
            plugin_id,
            permissions: permissions.into_iter().collect(),
        }
    }

    /// 检查是否有权限
    pub fn check(&self, permission: &str) -> Result<()> {
        // 无需权限的 API
        let no_permission_required = [
            "window.hide",
            "window.show",
            "window.setSize",
            "system.platform",
            "system.darkMode",
            "system.version",
        ];

        if no_permission_required.contains(&permission) {
            return Ok(());
        }

        // 检查精确匹配
        if self.permissions.contains(permission) {
            return Ok(());
        }

        // 检查通配符匹配
        // 例如 "fs.read" 匹配 "fs.read:/Users/*/**"
        let base_permission = permission.split(':').next().unwrap_or(permission);
        for p in &self.permissions {
            if p.starts_with(&format!("{}:", base_permission)) || p == base_permission {
                return Ok(());
            }
        }

        // 检查模块级权限
        // 例如 "fs.read" 匹配 "fs.*"
        let module = permission.split('.').next().unwrap_or(permission);
        if self.permissions.contains(&format!("{}.*", module)) {
            return Ok(());
        }

        Err(VoloError::PermissionDenied(format!(
            "Plugin '{}' does not have permission '{}'",
            self.plugin_id, permission
        )))
    }

    /// 检查是否有任一权限
    pub fn check_any(&self, permissions: &[&str]) -> Result<()> {
        for p in permissions {
            if self.check(p).is_ok() {
                return Ok(());
            }
        }

        Err(VoloError::PermissionDenied(format!(
            "Plugin '{}' does not have any of the required permissions: {:?}",
            self.plugin_id, permissions
        )))
    }
}

/// 权限级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// 低风险，默认允许
    Low,
    /// 中风险，需要声明
    Medium,
    /// 高风险，需要用户确认
    High,
}

/// 获取权限级别
pub fn get_permission_level(permission: &str) -> PermissionLevel {
    let low_permissions = [
        "clipboard.write",
        "notification",
        "db.read",
        "db.write",
    ];

    let high_permissions = [
        "shell.execute",
        "fs.write",
        "screen.capture",
    ];

    if low_permissions.iter().any(|p| permission.starts_with(p)) {
        return PermissionLevel::Low;
    }

    if high_permissions.iter().any(|p| permission.starts_with(p)) {
        return PermissionLevel::High;
    }

    PermissionLevel::Medium
}

/// 权限描述
pub fn get_permission_description(permission: &str) -> &'static str {
    match permission {
        "clipboard.read" => "读取剪贴板内容",
        "clipboard.write" => "写入剪贴板内容",
        "notification" => "发送系统通知",
        "db.read" => "读取插件数据库",
        "db.write" => "写入插件数据库",
        "fs.read" => "读取文件",
        "fs.write" => "写入文件",
        "shell.open" => "打开链接或文件",
        "shell.execute" => "执行系统命令",
        "screen.capture" => "截取屏幕",
        "http" => "发送网络请求",
        _ => "未知权限",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_check() {
        let manager = PermissionManager::new(
            "test-plugin".to_string(),
            vec!["clipboard.read".to_string(), "fs.read".to_string()],
        );

        assert!(manager.check("clipboard.read").is_ok());
        assert!(manager.check("fs.read").is_ok());
        assert!(manager.check("clipboard.write").is_err());
        assert!(manager.check("shell.execute").is_err());
    }

    #[test]
    fn test_wildcard_permission() {
        let manager = PermissionManager::new(
            "test-plugin".to_string(),
            vec!["fs.*".to_string()],
        );

        assert!(manager.check("fs.read").is_ok());
        assert!(manager.check("fs.write").is_ok());
        assert!(manager.check("clipboard.read").is_err());
    }

    #[test]
    fn test_no_permission_required() {
        let manager = PermissionManager::new(
            "test-plugin".to_string(),
            vec![],
        );

        assert!(manager.check("window.hide").is_ok());
        assert!(manager.check("system.platform").is_ok());
        assert!(manager.check("clipboard.read").is_err());
    }
}