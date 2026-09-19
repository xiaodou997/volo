//! 通知 API

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use crate::core::permission::{require, PermissionEngine};
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

#[derive(Debug, Deserialize)]
pub struct NotificationOptions {
    pub title: Option<String>,
    pub body: String,
    pub icon: Option<String>,
}

#[tauri::command]
pub async fn notification_show(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    options: NotificationOptions,
) -> Result<()> {
    require(&app, &engine, &plugins, plugin_id.as_deref(), "notification.show", None).await?;

    let title = options.title.unwrap_or_else(|| "Volo".to_string());

    let mut builder = app.notification().builder()
        .title(&title)
        .body(&options.body);

    if let Some(icon_path) = options.icon {
        builder = builder.icon(&icon_path);
    }

    builder.show()
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;

    Ok(())
}

// ============ 通知权限引导（#49） ============
//
// tauri-plugin-notification 在桌面端的 permission_state / request_permission
// 是硬编码返回 Granted 的 stub（见插件 desktop.rs），真正决定通知能否显示的是
// 操作系统：macOS 在首条通知送达后才把应用注册进系统设置 → 通知列表。
// 因此这里的权限流程以“发送一条可观察的引导通知”为核心，
// 让用户直接看到结果，并提供系统通知设置的深链入口。

/// 通知权限状态（跨平台统一视图，供 renderer 展示）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPermissionStatus {
    /// tauri-plugin-notification 上报的状态（桌面端恒为 granted，不代表系统真实授权）。
    pub plugin_state: String,
    /// 是否已发送过首条引导通知（macOS 首条通知后系统才会注册 Volo）。
    pub primed: bool,
    /// 系统通知设置面板的深链；不支持深链的平台为 null。
    pub settings_url: Option<String>,
}

static NOTIFICATION_STATE_LOCK: Mutex<()> = Mutex::new(());

fn notification_state_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("notification-state.json"))
}

fn load_primed(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    content
        .parse::<serde_json::Value>()
        .ok()
        .and_then(|value| value.get("primed").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

fn save_primed(path: &Path, primed: bool) -> Result<()> {
    let content = serde_json::to_string_pretty(&serde_json::json!({ "primed": primed }))?;
    std::fs::write(path, content)?;
    Ok(())
}

fn permission_state_label(state: &tauri::plugin::PermissionState) -> &'static str {
    use tauri::plugin::PermissionState as S;
    match state {
        S::Granted => "granted",
        S::Denied => "denied",
        S::Prompt | S::PromptWithRationale => "prompt",
    }
}

#[cfg(target_os = "macos")]
fn notification_settings_url(identifier: &str) -> Option<String> {
    Some(format!(
        "x-apple.systempreferences:com.apple.preference.notifications?id={identifier}"
    ))
}

#[cfg(not(target_os = "macos"))]
fn notification_settings_url(_identifier: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        Some("ms-settings:notifications".to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        None
    }
}

fn current_permission_status(app: &AppHandle) -> Result<NotificationPermissionStatus> {
    let plugin_state = app
        .notification()
        .permission_state()
        .map_err(|e| VoloError::Other(e.to_string()))?;
    let state_path = notification_state_path(app)?;
    let _guard = NOTIFICATION_STATE_LOCK
        .lock()
        .map_err(|_| VoloError::Other("notification state lock poisoned".to_string()))?;
    let primed = load_primed(&state_path);
    Ok(NotificationPermissionStatus {
        plugin_state: permission_state_label(&plugin_state).to_string(),
        primed,
        settings_url: notification_settings_url(&app.config().identifier),
    })
}

/// 查询通知权限状态。只读取，不触发系统弹窗。
#[tauri::command]
pub async fn notification_permission_status(
    app: AppHandle,
) -> Result<NotificationPermissionStatus> {
    current_permission_status(&app)
}

/// 申请/验证通知权限：调用插件权限入口（移动端为真实系统弹窗），
/// 并发送一条可观察的引导通知。桌面端插件权限 API 是 Granted stub，
/// 发送引导通知才是让 macOS 注册 Volo 的实际动作。
#[tauri::command]
pub async fn notification_request_permission(
    app: AppHandle,
) -> Result<NotificationPermissionStatus> {
    let requested = app
        .notification()
        .request_permission()
        .map_err(|e| VoloError::Other(e.to_string()))?;
    let _ = permission_state_label(&requested);

    let (title, body) = if cfg!(target_os = "macos") {
        (
            "Volo 通知权限测试",
            "能看到这条通知，说明 Volo 通知已开启；如果看不到，请打开 系统设置 → 通知 → Volo，开启「允许通知」。",
        )
    } else {
        ("Volo notification test", "This is a notification test from Volo.")
    };

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| VoloError::Other(e.to_string()))?;

    let state_path = notification_state_path(&app)?;
    let _guard = NOTIFICATION_STATE_LOCK
        .lock()
        .map_err(|_| VoloError::Other("notification state lock poisoned".to_string()))?;
    save_primed(&state_path, true)?;

    current_permission_status(&app)
}

/// 打开系统通知设置面板（macOS 直达 Volo 的通知设置项）。
#[tauri::command]
pub async fn notification_open_settings(app: AppHandle) -> Result<()> {
    let url = notification_settings_url(&app.config().identifier)
        .ok_or_else(|| VoloError::Other("当前平台不支持跳转到系统通知设置".to_string()))?;    app.opener()
        .open_url(&url, None::<String>)
        .map_err(|e| VoloError::Other(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "volo-notification-state-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn missing_state_file_means_not_primed() {
        let path = temp_state_path("missing");
        assert!(!load_primed(&path));
    }

    #[test]
    fn corrupt_state_file_means_not_primed() {
        let path = temp_state_path("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(!load_primed(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn primed_state_roundtrips() {
        let path = temp_state_path("roundtrip");
        assert!(!load_primed(&path));
        save_primed(&path, true).unwrap();
        assert!(load_primed(&path));
        save_primed(&path, false).unwrap();
        assert!(!load_primed(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn settings_url_matches_platform_contract() {
        let url = notification_settings_url("com.volo.app");
        #[cfg(target_os = "macos")]
        {
            let url = url.expect("macOS 必须提供通知设置深链");
            assert!(url.starts_with(
                "x-apple.systempreferences:com.apple.preference.notifications?id="
            ));
            assert!(url.ends_with("com.volo.app"));
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(url.as_deref(), Some("ms-settings:notifications"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(url.is_none());
        }
    }
}
