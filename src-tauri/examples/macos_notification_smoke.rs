#[cfg(target_os = "macos")]
fn main() {
    use std::time::Duration;

    use serde_json::json;
    use tauri::Manager;
    use volo_lib::ai::tools::ToolRegistry;
    use volo_lib::core::permission::PermissionEngine;

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
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
            let app_handle = app.handle().clone();

            let result = tauri::async_runtime::block_on(async {
                ToolRegistry::execute_as_background(
                    &app_handle,
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
        })
        .run(tauri::generate_context!())
        .expect("macOS notification smoke failed");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("macos_notification_smoke must run on macOS");
}
