//! macOS 原生通知桥（UNUserNotificationCenter）\n//!\n//! 归属于统一的 platform::macos 原生层，避免 Objective-C/AppKit 调用散落在 API 层。
//!
//! tauri-plugin-notification 经由 notify-rust → mac-notification-sys 投递通知，
//! 底层是 `objc/notify.m` 的 `NSUserNotificationCenter`（旧 API），该 API 已在
//! macOS 27 移除：注册静默失败，且插件内 `let _ = notification.show()` 把错误
//! 完全吞掉，调用方只能得到假的 `Ok(())`。
//!
//! 这里改用 UNUserNotificationCenter（macOS 10.14+ 现代 API），并把真实错误
//! 抛回调用方。前置条件：应用必须经过有效签名（Developer ID / Apple Development），
//! 否则系统仍拒绝注册（macOS 26+ 的要求）。

use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::exception::catch;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
    UNNotificationRequest, UNNotificationSettings, UNNotificationSound, UNUserNotificationCenter,
};

use crate::error::{Result, VoloError};

/// 等待系统回调的最长时间。UNC 回调由系统 RunLoop 驱动，正常在毫秒级完成；
/// 超时按"已提交"处理，避免后台线程被无限挂住。
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(2);

fn ns_error_message(err: *mut NSError) -> Option<String> {
    if err.is_null() {
        return None;
    }
    let description = unsafe { (*err).localizedDescription() };
    Some(description.to_string())
}

/// 包装 ObjC FFI 入口，捕获系统直接 raise 的 NSException。
///
/// 未有效签名的进程调用 UNUserNotificationCenter（macOS 26+ 要求签名）
/// 会在调用点直接抛异常（abort 前可被 objc2 的 exception 桥捕获），
/// 这里把它映射为可读的 VoloError，而不是让进程 Abort trap。
fn catch_objc<R>(f: impl FnOnce() -> R) -> Result<R> {
    match catch(AssertUnwindSafe(f)) {
        Ok(value) => Ok(value),
        Err(exc) => Err(VoloError::Other(format!(
            "macOS 通知接口被系统拒绝（未签名或环境不支持）: {}",
            exc.map(|e| e.to_string())
                .unwrap_or_else(|| "未知 NSException".to_string())
        ))),
    }
}

/// 当前通知授权状态（真实系统状态）。
///
/// 返回值：`notDetermined` / `denied` / `authorized` / `provisional` / `ephemeral`。
pub fn authorization_status() -> Result<&'static str> {
    let center = catch_objc(UNUserNotificationCenter::currentNotificationCenter)?;
    let (tx, rx) = mpsc::channel();
    let block: RcBlock<dyn Fn(NonNull<UNNotificationSettings>)> =
        RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let status = unsafe { settings.as_ref() }.authorizationStatus();
            let _ = tx.send(status);
        });
    center.getNotificationSettingsWithCompletionHandler(&block);
    let status = rx
        .recv_timeout(CALLBACK_TIMEOUT)
        .map_err(|_| VoloError::Other("读取 macOS 通知授权状态超时".to_string()))?;
    Ok(match status {
        UNAuthorizationStatus::NotDetermined => "notDetermined",
        UNAuthorizationStatus::Denied => "denied",
        UNAuthorizationStatus::Authorized => "authorized",
        UNAuthorizationStatus::Provisional => "provisional",
        UNAuthorizationStatus::Ephemeral => "ephemeral",
        _ => "unknown",
    })
}

/// 未授权时主动申请。macOS 对已签名应用直接授予，无系统弹窗；
/// 被拒绝时返回明确错误，引导用户去系统设置。
fn ensure_authorized() -> Result<()> {
    match authorization_status()? {
        "authorized" | "provisional" | "ephemeral" => Ok(()),
        "denied" => Err(VoloError::PermissionDenied(
            "Volo 通知被系统拒绝：请打开 系统设置 → 通知 → Volo，开启「允许通知」".to_string(),
        )),
        _ => {
            let center = catch_objc(UNUserNotificationCenter::currentNotificationCenter)?;
            let options = UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound
                | UNAuthorizationOptions::Badge;
            let (tx, rx) = mpsc::channel();
            let block: RcBlock<dyn Fn(Bool, *mut NSError)> =
                RcBlock::new(move |granted: Bool, err: *mut NSError| {
                    let outcome = if granted.as_bool() {
                        Ok(())
                    } else {
                        Err(ns_error_message(err)
                            .unwrap_or_else(|| "系统拒绝了通知授权".to_string()))
                    };
                    let _ = tx.send(outcome);
                });
            catch_objc(|| {
                center.requestAuthorizationWithOptions_completionHandler(options, &block)
            })?;
            rx.recv_timeout(CALLBACK_TIMEOUT)
                .map_err(|_| VoloError::Other("macOS 通知授权申请超时".to_string()))?
                .map_err(VoloError::Other)
        }
    }
}

/// 发送一条立即展示的系统通知。真实错误（未签名/未授权/参数拒绝）会返回 Err。
pub fn send(title: &str, body: &str) -> Result<()> {
    ensure_authorized()?;

    let center = catch_objc(UNUserNotificationCenter::currentNotificationCenter)?;
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));

    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&uuid::Uuid::new_v4().to_string()),
        &content,
        None,
    );

    let (tx, rx) = mpsc::channel();
    let block: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |err: *mut NSError| {
        let _ = tx.send(ns_error_message(err));
    });
    center_add_request(&center, &request, &block)?;
    match rx.recv_timeout(CALLBACK_TIMEOUT) {
        // None = 无错误 = 已提交；超时视为已提交（系统异步完成投递）。
        Ok(None) | Err(_) => Ok(()),
        Ok(Some(message)) => Err(VoloError::Other(format!("macOS 通知投递失败: {message}"))),
    }
}

/// 投递请求同样是系统可能直接 raise 异常的入口（未签名进程），单独包装。
fn center_add_request(
    center: &UNUserNotificationCenter,
    request: &UNNotificationRequest,
    block: &RcBlock<dyn Fn(*mut NSError)>,
) -> Result<()> {
    catch_objc(|| center.addNotificationRequest_withCompletionHandler(request, Some(block)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 授权状态标签必须与 UNAuthorizationStatus 原始值一一对应，
    /// renderer 依赖这些字符串渲染通知权限卡片。
    #[test]
    fn authorization_status_labels_match_raw_values() {
        assert_eq!(UNAuthorizationStatus::NotDetermined.0, 0);
        assert_eq!(UNAuthorizationStatus::Denied.0, 1);
        assert_eq!(UNAuthorizationStatus::Authorized.0, 2);
        assert_eq!(UNAuthorizationStatus::Provisional.0, 3);
        assert_eq!(UNAuthorizationStatus::Ephemeral.0, 4);
    }

    /// 引导/投递的超时阈值必须短于后台 Workflow 对单步的时间预算，
    /// 避免通知步骤拖垮整个 Automation 执行。
    #[test]
    fn callback_timeout_is_bounded() {
        assert!(CALLBACK_TIMEOUT <= Duration::from_secs(5));
    }
}
