//! 后台/定时任务的非交互权限裁决。
//!
//! Scheduler 不能依赖审批弹窗：应用可能隐藏、无人值守，等待审批会造成后台任务挂起。
//! 因此后台语义刻意比前台 `PermissionEngine::enforce` 更严格：
//! - Low 风险继续按现有策略默认允许；
//! - Medium / High / Critical 只接受 `Always` 授权；
//! - `Session` / `Once` 不可用于后台任务，保证重启前后语义一致；
//! - 缺少持久授权时立即拒绝，不 emit `permission-request`，也不等待超时。

use crate::core::capability::{capability_meta, RiskLevel};
use crate::error::{Result, VoloError};

use super::engine::{PermissionEngine, Scope};

fn normalize_resource(resource: Option<&str>) -> Option<String> {
    resource.map(|value| value.replace('\\', "/"))
}

/// 为后台执行做无交互权限检查。
///
/// 调用方仍需负责插件 manifest 声明检查；本函数只处理运行时授权范围。
pub fn enforce_background(
    engine: &PermissionEngine,
    principal: &str,
    capability: &str,
    resource: Option<&str>,
) -> Result<()> {
    let meta = capability_meta(capability);

    if meta.risk == RiskLevel::Low {
        engine.audit(principal, capability, resource, "allow", None);
        return Ok(());
    }

    let normalized_resource = normalize_resource(resource);
    let always_granted = engine.list_grants()?.into_iter().any(|grant| {
        grant.plugin_id == principal
            && grant.capability == capability
            && grant.scope == Scope::Always
            && normalize_resource(grant.resource.as_deref()) == normalized_resource
    });

    if always_granted {
        engine.audit(
            principal,
            capability,
            resource,
            "allow",
            Some(Scope::Always),
        );
        return Ok(());
    }

    engine.audit(principal, capability, resource, "deny", None);
    Err(VoloError::PermissionDenied(format!(
        "Background permission '{}' for '{}' requires an Always grant for resource {:?}",
        capability, principal, resource
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::permission::{store, Grant};
    use std::path::PathBuf;
    use std::time::Duration;

    fn test_paths(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "volo-background-permission-test-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (dir.join("permissions.json"), dir.join("audit.db"), dir)
    }

    fn engine_with_grants(name: &str, grants: &[Grant]) -> (PermissionEngine, PathBuf) {
        let (store_path, audit_path, dir) = test_paths(name);
        store::save_grants(&store_path, grants).unwrap();
        let engine =
            PermissionEngine::new(store_path, audit_path, Duration::from_millis(20)).unwrap();
        (engine, dir)
    }

    #[test]
    fn low_risk_background_permission_does_not_require_a_grant() {
        let (engine, dir) = engine_with_grants("low", &[]);
        assert!(enforce_background(&engine, "workflow:notify", "notification.show", None,).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn medium_risk_background_permission_requires_always() {
        let (engine, dir) = engine_with_grants("medium-deny", &[]);
        let error =
            enforce_background(&engine, "workflow:clip", "clipboard.read", None).unwrap_err();
        assert!(error.to_string().contains("requires an Always grant"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn session_grant_is_not_sufficient_for_background_execution() {
        let grants = vec![Grant {
            principal: "workflow:clip".to_string(),
            capability: "clipboard.read".to_string(),
            resource: None,
            scope: Scope::Session,
        }];
        let (engine, dir) = engine_with_grants("session-deny", &grants);
        assert!(enforce_background(&engine, "workflow:clip", "clipboard.read", None,).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn always_grant_allows_matching_background_resource() {
        let grants = vec![Grant {
            principal: "workflow:read-file".to_string(),
            capability: "fs.read".to_string(),
            resource: Some("/tmp/input.txt".to_string()),
            scope: Scope::Always,
        }];
        let (engine, dir) = engine_with_grants("always-resource", &grants);

        assert!(enforce_background(
            &engine,
            "workflow:read-file",
            "fs.read",
            Some("/tmp/input.txt"),
        )
        .is_ok());
        assert!(enforce_background(
            &engine,
            "workflow:read-file",
            "fs.read",
            Some("/tmp/other.txt"),
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn always_grant_is_principal_scoped() {
        let grants = vec![Grant {
            principal: "workflow:a".to_string(),
            capability: "clipboard.read".to_string(),
            resource: None,
            scope: Scope::Always,
        }];
        let (engine, dir) = engine_with_grants("principal", &grants);

        assert!(enforce_background(&engine, "workflow:a", "clipboard.read", None,).is_ok());
        assert!(enforce_background(&engine, "workflow:b", "clipboard.read", None,).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
