//! Volo - 桌面效率工具箱

pub mod error;
pub mod core;
pub mod api;
pub mod plugin;
pub mod search;
pub mod platform;

use core::StartupManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 初始化插件
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_positioner::init())
        // 初始化状态
        .setup(|app| {
            // 初始化日志
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();

            // 使用优化的启动流程
            let app_handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                if let Err(e) = StartupManager::optimized_startup(&app_handle).await {
                    tracing::error!("Startup failed: {}", e);
                }
            });

            Ok(())
        })
        // 注册命令
        .invoke_handler(tauri::generate_handler![
            // 窗口
            core::window::show_main_window,
            core::window::hide_main_window,
            core::window::toggle_main_window,
            core::window::set_window_height,
            // 配置
            core::config::get_config,
            core::config::save_config,
            // 快捷键
            core::shortcut::register_shortcut,
            core::shortcut::unregister_shortcut,
            // 剪贴板
            api::clipboard_read_text,
            api::clipboard_write_text,
            api::clipboard_read_image,
            api::clipboard_write_image,
            api::clipboard_read_files,
            // 数据库
            api::database::db_put,
            api::database::db_get,
            api::database::db_remove,
            api::database::db_all,
            // 文件系统
            api::fs::fs_read,
            api::fs::fs_read_binary,
            api::fs::fs_write,
            api::fs::fs_write_binary,
            api::fs::fs_exists,
            api::fs::fs_mkdir,
            api::fs::fs_remove,
            api::fs::fs_list,
            api::fs::fs_pick_file,
            api::fs::fs_pick_files,
            api::fs::fs_pick_folder,
            // 通知
            api::notification::notification_show,
            // 截图
            api::screen::screen_capture,
            api::screen::screen_capture_area,
            // Shell
            api::shell::shell_open,
            api::shell::shell_open_path,
            // 搜索
            search::app_search::search,
            search::app_cache::refresh_app_cache,
            search::app_cache::get_app_count,
            search::app_cache::get_app_icon,
            search::history::record_app_usage,
            search::history::get_search_history,
            search::history::clear_search_history,
            // 文件索引（新的优化版本）
            search::file_index::file_index_search,
            search::file_index::file_index_stats,
            search::file_index::file_index_refresh,
            // 剪贴板历史
            core::clipboard_history::clipboard_history_get_all,
            core::clipboard_history::clipboard_history_remove,
            core::clipboard_history::clipboard_history_clear,
            core::clipboard_history::clipboard_history_save,
            // 插件
            plugin::manager::list_plugins,
            plugin::manager::get_plugin,
            plugin::manager::scan_plugins,
            plugin::manager::install_plugin,
            plugin::manager::install_plugin_from_dir,
            plugin::manager::uninstall_plugin,
            plugin::runner::get_plugin_runtime,
            plugin::runner::get_plugin_html,
            plugin::runner::get_plugin_asset_path,
            plugin::runner::load_plugin,
            plugin::runner::unload_plugin,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}