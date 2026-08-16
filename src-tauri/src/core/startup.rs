//! 启动优化模块
//! 管理应用启动流程，优化启动性能

use std::time::{Instant, Duration};
use tauri::{AppHandle, Manager};
use tracing::{info, debug};

/// 启动阶段计时器
pub struct StartupTimer {
    start: Instant,
    phases: Vec<(String, Duration)>,
}

impl StartupTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            phases: Vec::new(),
        }
    }

    /// 记录阶段完成
    pub fn record_phase(&mut self, name: &str) {
        let elapsed = self.start.elapsed();
        self.phases.push((name.to_string(), elapsed));
        debug!("Startup phase [{}]: {:?}", name, elapsed);
    }

    /// 打印启动统计
    pub fn print_stats(&self) {
        info!("========== Startup Statistics ==========");
        for (i, (name, duration)) in self.phases.iter().enumerate() {
            let prev_duration = if i > 0 {
                self.phases[i - 1].1
            } else {
                Duration::from_secs(0)
            };
            let phase_time = *duration - prev_duration;
            info!("  [{}]: {:?} (total: {:?})", name, phase_time, duration);
        }
        info!("  Total: {:?}", self.start.elapsed());
        info!("========================================");
    }
}

/// 启动管理器
pub struct StartupManager;

impl StartupManager {
    /// 执行优化的启动流程
    pub async fn optimized_startup(app: &AppHandle) -> Result<(), crate::error::VoloError> {
        let mut timer = StartupTimer::new();

        // Phase 1: 初始化关键配置（同步，必须）
        Self::init_critical(app).await?;
        timer.record_phase("critical_init");

        // Phase 2: 并行初始化独立模块
        Self::init_parallel(app).await?;
        timer.record_phase("parallel_init");

        // Phase 3: 初始化 UI 相关（托盘、快捷键）
        Self::init_ui(app).await?;
        timer.record_phase("ui_init");

        // Phase 4: 延迟加载非关键模块
        tokio::spawn(Self::init_deferred(app.clone()));
        timer.record_phase("deferred_spawn");

        // 打印启动统计
        timer.print_stats();

        Ok(())
    }

    /// 初始化关键模块（同步阻塞）
    async fn init_critical(app: &AppHandle) -> Result<(), crate::error::VoloError> {
        use crate::core::Config;
        use crate::api::database::Database;

        // 配置（必须最先加载）
        let config = Config::init(app)?;
        app.manage(config);

        // 主数据库
        let db_path = app.path().app_data_dir()?.join("volo.db");
        let db = Database::new(&db_path)?;
        app.manage(db);

        // 权限引擎（加载持久化授权、打开审计库）
        let engine = crate::core::permission::PermissionEngine::init(app)?;
        app.manage(engine);

        Ok(())
    }

    /// 并行初始化独立模块
    async fn init_parallel(app: &AppHandle) -> Result<(), crate::error::VoloError> {
        use crate::search::app_cache::AppCache;
        use crate::search::history::SearchHistoryManager;
        use crate::search::file_index::FileIndex;
        use crate::plugin::manager::PluginState;

        // 并行初始化这些独立的模块
        let app_handle1 = app.clone();
        let app_handle2 = app.clone();
        let app_data_dir1 = app.path().app_data_dir()?;
        let app_data_dir2 = app_data_dir1.clone();
        let _app_data_dir3 = app_data_dir1.clone();

        let cache_task = tokio::task::spawn_blocking(move || {
            let cache = AppCache::new(&app_handle1)?;
            cache.async_load();
            Ok::<_, crate::error::VoloError>(cache)
        });

        let history_task = tokio::task::spawn_blocking(move || {
            let history_path = app_data_dir1.join("search_history.db");
            SearchHistoryManager::new(&history_path)
        });

        let file_index_task = tokio::task::spawn_blocking(move || {
            let file_index = FileIndex::new(&app_data_dir2)?;
            // 只加载统计，不启动后台索引
            let _ = file_index.get_stats();
            Ok::<_, crate::error::VoloError>(file_index)
        });

        let plugin_task = tokio::task::spawn_blocking(move || {
            PluginState::new(&app_handle2)
        });

        // 等待所有任务完成
        let cache = cache_task.await.map_err(|e| {
            crate::error::VoloError::Other(format!("Cache init failed: {}", e))
        })??;
        app.manage(cache);

        let history = history_task.await.map_err(|e| {
            crate::error::VoloError::Other(format!("History init failed: {}", e))
        })??;
        app.manage(history);

        let file_index = file_index_task.await.map_err(|e| {
            crate::error::VoloError::Other(format!("File index init failed: {}", e))
        })??;
        app.manage(file_index);

        let plugin_state = plugin_task.await.map_err(|e| {
            crate::error::VoloError::Other(format!("Plugin state init failed: {}", e))
        })??;
        app.manage(plugin_state);

        Ok(())
    }

    /// 初始化 UI 相关
    async fn init_ui(app: &AppHandle) -> Result<(), crate::error::VoloError> {
        use crate::core::{create_tray, ShortcutManager};

        // 创建托盘
        create_tray(app)?;

        // 注册快捷键
        ShortcutManager::register_default(app)?;

        Ok(())
    }

    /// 延迟加载非关键模块
    async fn init_deferred(app: AppHandle) {
        use crate::core::ClipboardHistory;
        use crate::search::file_index::FileIndex;

        // 延迟 2 秒后启动后台任务
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 启动文件索引后台更新
        if let Ok(file_index) = FileIndex::new(&app.path().app_data_dir().unwrap_or_default()) {
            file_index.start_background_index();
            info!("File index background indexing started");
        }

        // 延迟 3 秒后初始化剪贴板历史
        tokio::time::sleep(Duration::from_secs(1)).await;

        let clipboard_history = ClipboardHistory::new();
        if clipboard_history.load(&app).is_ok() {
            clipboard_history.start_monitoring(app.clone());
            info!("Clipboard history monitoring started");
        }

        info!("Deferred initialization completed");
    }
}

/// 启动性能监控
pub struct PerformanceMonitor {
    start: Instant,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// 检查是否超过阈值
    pub fn check_threshold(&self, threshold_ms: u64) -> bool {
        self.start.elapsed().as_millis() > threshold_ms as u128
    }

    /// 获取已用时间
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

// ============ Tauri Commands ============

/// 获取启动统计信息
#[tauri::command]
pub fn get_startup_stats() -> Result<StartupStats, String> {
    // 这里可以从全局状态获取启动统计
    Ok(StartupStats {
        total_time_ms: 0,
        phases: vec![],
    })
}

/// 启动统计信息
#[derive(serde::Serialize)]
pub struct StartupStats {
    pub total_time_ms: u64,
    pub phases: Vec<PhaseStat>,
}

#[derive(serde::Serialize)]
pub struct PhaseStat {
    pub name: String,
    pub duration_ms: u64,
}