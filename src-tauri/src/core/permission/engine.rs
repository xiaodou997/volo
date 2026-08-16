//! Permission Engine
//! 权限裁决引擎：声明检查 → 授权查询 → 运行时审批

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::core::capability::{capability_meta, RiskLevel};
use crate::error::{Result, VoloError};

use super::{audit::AuditLog, store};

/// 默认审批超时时间
pub const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);

/// 裁决结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

/// 授权范围
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// 仅本次调用
    Once,
    /// 本次会话（内存，重启失效）
    Session,
    /// 始终允许（持久化到 permissions.json）
    Always,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Once => "once",
            Scope::Session => "session",
            Scope::Always => "always",
        }
    }
}

/// 一条授权记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    pub principal: String,
    pub capability: String,
    pub scope: Scope,
}

/// 审批请求事件 payload（emit `permission-request`）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub request_id: String,
    pub plugin_id: String,
    pub capability: String,
    pub description: String,
    pub risk: RiskLevel,
    pub resource: Option<String>,
}

/// 授权列表项（permission_list_grants 返回值）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantInfo {
    pub plugin_id: String,
    pub capability: String,
    pub scope: Scope,
    pub risk: RiskLevel,
    pub description: &'static str,
}

/// 审批响应（仅模块内部构造）
pub struct PendingResponse {
    allow: bool,
    scope: Scope,
}

/// 核心决策逻辑（纯函数，不依赖 Tauri，便于测试）
///
/// - 未声明 → Deny（任何风险等级）
/// - 已有授权 → Allow
/// - 已声明且低风险 → Allow（低风险默认允许，不弹审批）
/// - 已声明且 Medium/High/Critical 无授权 → Ask
pub fn decide(declared: bool, risk: RiskLevel, grant: Option<Scope>) -> Decision {
    if !declared {
        return Decision::Deny;
    }
    if grant.is_some() {
        return Decision::Allow;
    }
    match risk {
        RiskLevel::Low => Decision::Allow,
        _ => Decision::Ask,
    }
}

/// 权限引擎（Tauri managed state）
pub struct PermissionEngine {
    /// (principal, capability) -> scope；Session 授权只活在内存，Always 同步持久化
    grants: Mutex<HashMap<(String, String), Scope>>,
    /// request_id -> 等待审批的 channel
    pending: Mutex<HashMap<String, oneshot::Sender<PendingResponse>>>,
    /// Always 授权持久化路径（config_dir/permissions.json）
    store_path: PathBuf,
    /// 审计日志（app_data_dir/audit.db）
    audit: Mutex<AuditLog>,
    /// 审批超时（可配置，便于测试）
    approval_timeout: Duration,
}

impl PermissionEngine {
    /// 从 AppHandle 初始化（加载持久化授权、打开审计库）
    pub fn init(app: &AppHandle) -> Result<Self> {
        let config_dir = app.path().app_config_dir()?;
        let data_dir = app.path().app_data_dir()?;
        Self::new(
            config_dir.join("permissions.json"),
            data_dir.join("audit.db"),
            DEFAULT_APPROVAL_TIMEOUT,
        )
    }

    /// 以显式路径构造（测试可用临时目录与短超时）
    pub fn new(store_path: PathBuf, audit_path: PathBuf, approval_timeout: Duration) -> Result<Self> {
        let persisted = store::load_grants(&store_path)?;
        let grants: HashMap<(String, String), Scope> = persisted
            .into_iter()
            .map(|g| ((g.principal, g.capability), g.scope))
            .collect();

        let audit = AuditLog::open(&audit_path)?;

        info!("PermissionEngine initialized with {} persisted grants", grants.len());

        Ok(Self {
            grants: Mutex::new(grants),
            pending: Mutex::new(HashMap::new()),
            store_path,
            audit: Mutex::new(audit),
            approval_timeout,
        })
    }

    /// 检查插件声明的权限是否覆盖 capability
    ///
    /// 匹配规则（迁移自 plugin/permission.rs）：
    /// - 精确匹配："clipboard.read" 声明覆盖 "clipboard.read"
    /// - 冒号 scope：声明 "fs.read:/Users/**" 或 "fs.read" 覆盖 "fs.read:/a/b"
    /// - 模块级通配："fs.*" 覆盖 "fs.read"、"fs.write"
    pub fn declared(permissions: &[String], capability: &str) -> bool {
        let base = capability.split(':').next().unwrap_or(capability);

        for p in permissions {
            // 精确匹配
            if p == capability {
                return true;
            }

            // 声明的 base 与 capability 的 base 相同（声明可带 scope）
            let p_base = p.split(':').next().unwrap_or(p);
            if p_base == base {
                return true;
            }

            // 模块级通配："fs.*"
            let module = base.split('.').next().unwrap_or(base);
            if p == &format!("{}.*", module) {
                return true;
            }
        }

        false
    }

    /// 执行权限裁决（声明检查由 guard 完成，这里处理授权与审批）
    pub async fn enforce(
        &self,
        app: &AppHandle,
        principal: &str,
        capability: &str,
        resource: Option<&str>,
    ) -> Result<()> {
        let meta = capability_meta(capability);
        let key = (principal.to_string(), capability.to_string());

        let existing = self
            .grants
            .lock()
            .map_err(|_| VoloError::Other("Permission lock error".to_string()))?
            .get(&key)
            .copied();

        match decide(true, meta.risk, existing) {
            Decision::Allow => {
                // Once 授权用完即删
                if existing == Some(Scope::Once) {
                    if let Ok(mut grants) = self.grants.lock() {
                        grants.remove(&key);
                    }
                }
                if meta.risk >= RiskLevel::Medium {
                    self.audit(principal, capability, resource, "allow", existing);
                }
                Ok(())
            }
            Decision::Ask => self.ask(app, principal, capability, resource).await,
            Decision::Deny => {
                self.audit(principal, capability, resource, "deny", None);
                Err(VoloError::PermissionDenied(format!(
                    "Permission '{}' denied for '{}'",
                    capability, principal
                )))
            }
        }
    }

    /// 审批流程：发事件 → 等待响应（带超时）
    async fn ask(
        &self,
        app: &AppHandle,
        principal: &str,
        capability: &str,
        resource: Option<&str>,
    ) -> Result<()> {
        let (request_id, rx) = self.begin_request()?;

        let meta = capability_meta(capability);
        let payload = PermissionRequest {
            request_id: request_id.clone(),
            plugin_id: principal.to_string(),
            capability: capability.to_string(),
            description: meta.description.to_string(),
            risk: meta.risk,
            resource: resource.map(|r| r.to_string()),
        };

        // 通知主窗口弹审批框；发不出去（如无窗口）则直接视为拒绝
        if let Err(e) = app.emit("permission-request", &payload) {
            self.cancel_request(&request_id);
            warn!("Failed to emit permission-request: {}", e);
            return Err(VoloError::PermissionDenied(format!(
                "Permission '{}' denied for '{}': approval UI unavailable",
                capability, principal
            )));
        }

        self.wait_for_response(&request_id, principal, capability, resource, rx)
            .await
    }

    /// 生成 request_id 并挂起等待通道（与发事件解耦，便于测试）
    pub fn begin_request(&self) -> Result<(String, oneshot::Receiver<PendingResponse>)> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| VoloError::Other("Permission lock error".to_string()))?
            .insert(request_id.clone(), tx);
        Ok((request_id, rx))
    }

    /// 等待审批响应：超时视为 Deny；允许时按 scope 记录授权
    pub async fn wait_for_response(
        &self,
        request_id: &str,
        principal: &str,
        capability: &str,
        resource: Option<&str>,
        rx: oneshot::Receiver<PendingResponse>,
    ) -> Result<()> {
        let outcome = tokio::time::timeout(self.approval_timeout, rx).await;
        self.cancel_request(request_id);

        match outcome {
            Ok(Ok(PendingResponse { allow: true, scope })) => {
                // Once 不落授权表（仅放行本次调用）；Session/Always 记录
                if scope != Scope::Once {
                    self.record_grant(principal, capability, scope)?;
                }
                self.audit(principal, capability, resource, "allow", Some(scope));
                Ok(())
            }
            Ok(Ok(PendingResponse { allow: false, .. })) => {
                self.audit(principal, capability, resource, "deny", None);
                Err(VoloError::PermissionDenied(format!(
                    "Permission '{}' denied by user for '{}'",
                    capability, principal
                )))
            }
            _ => {
                // 超时或 channel 断开
                self.audit(principal, capability, resource, "deny", None);
                Err(VoloError::PermissionDenied(format!(
                    "Permission '{}' approval timed out for '{}'",
                    capability, principal
                )))
            }
        }
    }

    /// 用户审批响应（permission_respond 命令调用）
    pub fn respond(&self, request_id: &str, allow: bool, scope: Scope) -> Result<()> {
        let tx = self
            .pending
            .lock()
            .map_err(|_| VoloError::Other("Permission lock error".to_string()))?
            .remove(request_id)
            .ok_or_else(|| VoloError::NotFound(format!("permission request: {}", request_id)))?;

        tx.send(PendingResponse { allow, scope })
            .map_err(|_| VoloError::Other("Permission request already closed".to_string()))
    }

    /// 列出当前所有授权（含 Session 与 Always）
    pub fn list_grants(&self) -> Result<Vec<GrantInfo>> {
        let grants = self
            .grants
            .lock()
            .map_err(|_| VoloError::Other("Permission lock error".to_string()))?;

        Ok(grants
            .iter()
            .map(|((principal, capability), scope)| {
                let meta = capability_meta(capability);
                GrantInfo {
                    plugin_id: principal.clone(),
                    capability: capability.clone(),
                    scope: *scope,
                    risk: meta.risk,
                    description: meta.description,
                }
            })
            .collect())
    }

    /// 撤销授权
    pub fn revoke(&self, principal: &str, capability: &str) -> Result<()> {
        let removed = self
            .grants
            .lock()
            .map_err(|_| VoloError::Other("Permission lock error".to_string()))?
            .remove(&(principal.to_string(), capability.to_string()));

        if removed == Some(Scope::Always) {
            self.persist_always_grants()?;
        }

        info!("Revoked permission '{}' for '{}'", capability, principal);
        Ok(())
    }

    /// 记录授权；Always 同步持久化
    fn record_grant(&self, principal: &str, capability: &str, scope: Scope) -> Result<()> {
        self.grants
            .lock()
            .map_err(|_| VoloError::Other("Permission lock error".to_string()))?
            .insert((principal.to_string(), capability.to_string()), scope);

        if scope == Scope::Always {
            self.persist_always_grants()?;
        }
        Ok(())
    }

    /// 将当前 Always 授权整体写回 permissions.json
    fn persist_always_grants(&self) -> Result<()> {
        let grants = self
            .grants
            .lock()
            .map_err(|_| VoloError::Other("Permission lock error".to_string()))?;

        let always: Vec<Grant> = grants
            .iter()
            .filter(|(_, scope)| **scope == Scope::Always)
            .map(|((principal, capability), scope)| Grant {
                principal: principal.clone(),
                capability: capability.clone(),
                scope: *scope,
            })
            .collect();

        store::save_grants(&self.store_path, &always)
    }

    fn cancel_request(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(request_id);
        }
    }

    /// 写审计日志（Medium 及以上由调用方控制；失败只告警不阻断）
    pub fn audit(
        &self,
        principal: &str,
        capability: &str,
        resource: Option<&str>,
        decision: &str,
        scope: Option<Scope>,
    ) {
        let result = self.audit.lock().ok().map(|log| {
            log.record(
                principal,
                capability,
                resource,
                decision,
                scope.map(|s| s.as_str()),
            )
        });
        if let Some(Err(e)) = result {
            warn!("Failed to write audit log: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_engine(name: &str, timeout: Duration) -> (PermissionEngine, PathBuf) {
        let dir = std::env::temp_dir().join(format!("volo_engine_test_{}_{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = PermissionEngine::new(
            dir.join("permissions.json"),
            dir.join("audit.db"),
            timeout,
        )
        .unwrap();
        (engine, dir)
    }

    fn perms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // ---- 迁移自 plugin/permission.rs 的匹配测试 ----

    #[test]
    fn test_declared_exact_match() {
        let permissions = perms(&["clipboard.read", "fs.read"]);
        assert!(PermissionEngine::declared(&permissions, "clipboard.read"));
        assert!(PermissionEngine::declared(&permissions, "fs.read"));
        assert!(!PermissionEngine::declared(&permissions, "clipboard.write"));
        assert!(!PermissionEngine::declared(&permissions, "shell.execute"));
    }

    #[test]
    fn test_declared_wildcard() {
        let permissions = perms(&["fs.*"]);
        assert!(PermissionEngine::declared(&permissions, "fs.read"));
        assert!(PermissionEngine::declared(&permissions, "fs.write"));
        assert!(!PermissionEngine::declared(&permissions, "clipboard.read"));
    }

    #[test]
    fn test_declared_empty_denies_all() {
        let permissions = perms(&[]);
        assert!(!PermissionEngine::declared(&permissions, "clipboard.read"));
        assert!(!PermissionEngine::declared(&permissions, "db.read"));
    }

    #[test]
    fn test_declared_scope_match() {
        // 声明带 scope："fs.read:/Users/**" 覆盖 "fs.read" 及 "fs.read:/a/b"
        let permissions = perms(&["fs.read:/Users/**"]);
        assert!(PermissionEngine::declared(&permissions, "fs.read"));
        assert!(PermissionEngine::declared(&permissions, "fs.read:/a/b"));
        // capability 带 scope 时，裸声明 "fs.read" 也覆盖
        assert!(PermissionEngine::declared(&perms(&["fs.read"]), "fs.read:/a/b"));
        assert!(!PermissionEngine::declared(&permissions, "fs.write:/a/b"));
    }

    // ---- 决策矩阵 ----

    #[test]
    fn test_decide_matrix() {
        // 未声明：任何风险等级都 Deny
        for risk in [RiskLevel::Low, RiskLevel::Medium, RiskLevel::High, RiskLevel::Critical] {
            assert_eq!(decide(false, risk, None), Decision::Deny);
            assert_eq!(decide(false, risk, Some(Scope::Always)), Decision::Deny);
        }

        // 已声明、无授权：Low → Allow，其余 → Ask
        assert_eq!(decide(true, RiskLevel::Low, None), Decision::Allow);
        assert_eq!(decide(true, RiskLevel::Medium, None), Decision::Ask);
        assert_eq!(decide(true, RiskLevel::High, None), Decision::Ask);
        assert_eq!(decide(true, RiskLevel::Critical, None), Decision::Ask);

        // 已声明、有授权：任何等级 Allow
        for scope in [Scope::Once, Scope::Session, Scope::Always] {
            assert_eq!(decide(true, RiskLevel::Medium, Some(scope)), Decision::Allow);
            assert_eq!(decide(true, RiskLevel::High, Some(scope)), Decision::Allow);
        }
    }

    // ---- respond 唤醒 pending ----

    #[tokio::test]
    async fn test_respond_wakes_pending_and_records_grant() {
        let (engine, dir) = temp_engine("respond", Duration::from_secs(5));
        let engine = std::sync::Arc::new(engine);

        let (request_id, rx) = engine.begin_request().unwrap();

        // 模拟前端审批：另一个线程延迟调用 respond，唤醒挂起的等待
        let engine2 = engine.clone();
        let rid = request_id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            engine2.respond(&rid, true, Scope::Session).unwrap();
        });

        let result = engine
            .wait_for_response(&request_id, "plugin-a", "clipboard.read", None, rx)
            .await;
        assert!(result.is_ok());

        // Session 授权已记录在内存
        let grants = engine.list_grants().unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].plugin_id, "plugin-a");
        assert_eq!(grants[0].capability, "clipboard.read");
        assert_eq!(grants[0].scope, Scope::Session);

        // Session 授权不持久化
        assert!(!dir.join("permissions.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_respond_once_not_recorded() {
        let (engine, dir) = temp_engine("once", Duration::from_secs(5));

        let (request_id, rx) = engine.begin_request().unwrap();
        engine.respond(&request_id, true, Scope::Once).unwrap();

        let result = engine
            .wait_for_response(&request_id, "plugin-a", "fs.read", None, rx)
            .await;
        assert!(result.is_ok());
        // Once 不记录授权
        assert!(engine.list_grants().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_respond_always_persisted() {
        let (engine, dir) = temp_engine("always", Duration::from_secs(5));

        let (request_id, rx) = engine.begin_request().unwrap();
        engine.respond(&request_id, true, Scope::Always).unwrap();

        engine
            .wait_for_response(&request_id, "plugin-a", "fs.write", Some("/tmp/x"), rx)
            .await
            .unwrap();

        // 已持久化，新引擎实例能加载
        let engine2 = PermissionEngine::new(
            dir.join("permissions.json"),
            dir.join("audit.db"),
            Duration::from_secs(5),
        )
        .unwrap();
        let grants = engine2.list_grants().unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].scope, Scope::Always);

        // 撤销后持久化文件同步更新
        engine2.revoke("plugin-a", "fs.write").unwrap();
        let loaded = store::load_grants(&dir.join("permissions.json")).unwrap();
        assert!(loaded.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_respond_deny() {
        let (engine, dir) = temp_engine("deny", Duration::from_secs(5));

        let (request_id, rx) = engine.begin_request().unwrap();
        engine.respond(&request_id, false, Scope::Once).unwrap();

        let result = engine
            .wait_for_response(&request_id, "plugin-a", "fs.read", None, rx)
            .await;
        assert!(matches!(result, Err(VoloError::PermissionDenied(_))));
        assert!(engine.list_grants().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- 超时路径 ----

    #[tokio::test]
    async fn test_approval_timeout_is_deny() {
        let (engine, dir) = temp_engine("timeout", Duration::from_millis(50));

        let (request_id, rx) = engine.begin_request().unwrap();
        let result = engine
            .wait_for_response(&request_id, "plugin-a", "screen.capture", None, rx)
            .await;
        assert!(matches!(result, Err(VoloError::PermissionDenied(_))));

        // 超时后 pending 已清理，respond 找不到请求
        assert!(engine.respond(&request_id, true, Scope::Once).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
