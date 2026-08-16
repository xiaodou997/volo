//! Capability Registry
//! 插件面 API 的能力注册表：每个 capability 的风险等级与中文描述

use serde::Serialize;

/// 风险等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// 低风险：已声明即默认允许，无需审批
    Low,
    /// 中风险：需要用户审批
    Medium,
    /// 高风险：需要用户审批，UI 高亮警告
    High,
    /// 严重风险：预留（computer.*），需要用户审批
    Critical,
}

/// Capability 元数据
#[derive(Debug, Clone, Copy)]
pub struct CapabilityMeta {
    pub id: &'static str,
    pub risk: RiskLevel,
    pub description: &'static str,
}

/// 查询 capability 的元数据；未知 capability 默认 Medium
pub fn capability_meta(capability: &str) -> CapabilityMeta {
    // 忽略 scope 后缀（如 "fs.read:/path"）
    let base = capability.split(':').next().unwrap_or(capability);

    match base {
        "notification.show" => CapabilityMeta {
            id: "notification.show",
            risk: RiskLevel::Low,
            description: "发送系统通知",
        },
        "clipboard.write" => CapabilityMeta {
            id: "clipboard.write",
            risk: RiskLevel::Low,
            description: "写入剪贴板内容",
        },
        "db.read" => CapabilityMeta {
            id: "db.read",
            risk: RiskLevel::Low,
            description: "读取插件数据库",
        },
        "db.write" => CapabilityMeta {
            id: "db.write",
            risk: RiskLevel::Low,
            description: "写入插件数据库",
        },
        "clipboard.read" => CapabilityMeta {
            id: "clipboard.read",
            risk: RiskLevel::Medium,
            description: "读取剪贴板内容",
        },
        "screen.capture" => CapabilityMeta {
            id: "screen.capture",
            risk: RiskLevel::Medium,
            description: "截取屏幕",
        },
        "fs.read" => CapabilityMeta {
            id: "fs.read",
            risk: RiskLevel::Medium,
            description: "读取文件",
        },
        "shell.open" => CapabilityMeta {
            id: "shell.open",
            risk: RiskLevel::Medium,
            description: "打开链接或文件",
        },
        "fs.write" => CapabilityMeta {
            id: "fs.write",
            risk: RiskLevel::High,
            description: "写入或删除文件",
        },
        "shell.execute" => CapabilityMeta {
            id: "shell.execute",
            risk: RiskLevel::High,
            description: "执行系统命令",
        },
        c if c.starts_with("fs.pick") => CapabilityMeta {
            id: "fs.pick",
            risk: RiskLevel::Low,
            description: "选择文件或文件夹",
        },
        // 预留：计算机控制类能力
        c if c.starts_with("computer.") => CapabilityMeta {
            id: "computer.control",
            risk: RiskLevel::Critical,
            description: "控制计算机（预留能力）",
        },
        _ => CapabilityMeta {
            id: "unknown",
            risk: RiskLevel::Medium,
            description: "未知权限",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_levels() {
        assert_eq!(capability_meta("notification.show").risk, RiskLevel::Low);
        assert_eq!(capability_meta("clipboard.write").risk, RiskLevel::Low);
        assert_eq!(capability_meta("db.read").risk, RiskLevel::Low);
        assert_eq!(capability_meta("db.write").risk, RiskLevel::Low);
        assert_eq!(capability_meta("fs.pick").risk, RiskLevel::Low);
        assert_eq!(capability_meta("fs.pick.file").risk, RiskLevel::Low);

        assert_eq!(capability_meta("clipboard.read").risk, RiskLevel::Medium);
        assert_eq!(capability_meta("screen.capture").risk, RiskLevel::Medium);
        assert_eq!(capability_meta("fs.read").risk, RiskLevel::Medium);
        assert_eq!(capability_meta("shell.open").risk, RiskLevel::Medium);

        assert_eq!(capability_meta("fs.write").risk, RiskLevel::High);
        assert_eq!(capability_meta("shell.execute").risk, RiskLevel::High);

        assert_eq!(capability_meta("computer.click").risk, RiskLevel::Critical);

        // 未知 capability 默认 Medium
        assert_eq!(capability_meta("something.else").risk, RiskLevel::Medium);
    }
}
