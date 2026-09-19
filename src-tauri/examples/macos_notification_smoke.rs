//! macOS 原生通知链路 smoke（生产调用链）。
//!
//! 前置条件：当前二进制必须经过有效签名（Developer ID / Apple Development）。
//! macOS 26+ 拒绝未签名进程使用 UNUserNotificationCenter——未签名时本例程打印
//! SKIP 并以 0 退出，而不是让 Abort trap 污染 CI；签名 runner 上则走完整断言：
//! ToolRegistry → notification_show → tauri_plugin_notification → .show()。
//!
//! 注意：UNC 的 completion handler 由主线程 RunLoop 驱动，因此断言逻辑必须放到
//! 独立线程里执行，让 `.run()` 的主 RunLoop 保持运转。

#[cfg(target_os = "macos")]
fn main() {
    use std::process::Command;
    use std::time::Duration;

    use tauri::Manager;

    // codesign -dv 把结果写到 stderr（stdout 恒为空）。
    let output = Command::new("codesign")
        .args(["-dv", &std::env::current_exe().unwrap().to_string_lossy()])
        .output()
        .expect("codesign -dv failed");
    let info = String::from_utf8_lossy(&output.stderr);
    if !info.contains("Authority=") {
        println!(
            "SKIP: unsigned binary cannot use UNUserNotificationCenter on this macOS; \
             codesign info: {info}"
        );
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // 等主 RunLoop 转起来，UNC 回调才有投递线程。
                std::thread::sleep(Duration::from_millis(500));
                if let Err(err) = run_smoke(&app_handle) {
                    eprintln!("macOS notification smoke failed: {err}");
                    app_handle.exit(1);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("macOS notification smoke failed");
}

#[cfg(target_os = "macos")]
fn run_smoke(app_handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use serde_json::json;
    use volo_lib::ai::tools::ToolRegistry;
    use volo_lib::core::permission::PermissionEngine;

    let root = std::env::temp_dir().join(format!(
        "volo-macos-notification-smoke-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root)?;

    let engine = PermissionEngine::new(
        root.join("permissions.json"),
        root.join("audit.db"),
        Duration::from_secs(1),
    )?;

    let result = tauri::async_runtime::block_on(async {
        ToolRegistry::execute_as_background(
            app_handle,
            &engine,
            "workflow:macos-notification-smoke",
            "notification_show",
            &json!({
                "title": "Volo CI",
                "body": "macOS notification smoke",
            }),
        )
        .await
    })?;

    if result != json!("通知已发送") {
        return Err(format!("unexpected notification result: {result}").into());
    }

    println!("macOS notification smoke passed: {result}");
    app_handle.exit(0);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("macos_notification_smoke must run on macOS");
}
