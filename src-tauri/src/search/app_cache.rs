//! 应用缓存模块
//! 提供应用列表的缓存机制，避免每次搜索都扫描文件系统

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tauri::AppHandle;
use tauri::Manager;
use tracing::{info, warn};
use crate::error::{Result, VoloError};

/// 应用信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
    #[serde(rename = "type")]
    pub app_type: String,
    /// 搜索用的拼音
    #[serde(default)]
    pub pinyin: Option<String>,
    /// 搜索用的首字母
    #[serde(default)]
    pub initials: Option<String>,
}

/// 内部缓存数据
struct CacheData {
    apps: Vec<AppInfo>,
    name_index: HashMap<String, usize>,
    loaded: bool,
}

/// 应用缓存状态
pub struct AppCache {
    /// 内存缓存
    data: Arc<RwLock<CacheData>>,
    /// 数据库路径
    db_path: PathBuf,
}

impl AppCache {
    /// 创建新的应用缓存
    pub fn new(app: &AppHandle) -> Result<Self> {
        let db_path = app.path().app_data_dir()?.join("apps.db");
        let db = Connection::open(&db_path)?;

        // 创建表
        db.execute(
            "CREATE TABLE IF NOT EXISTS apps (
                path TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                icon TEXT,
                app_type TEXT NOT NULL,
                pinyin TEXT,
                initials TEXT,
                updated_at INTEGER
            )",
            [],
        )?;

        // 创建索引
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_apps_name ON apps(name)",
            [],
        )?;

        drop(db); // 关闭连接，后续通过路径重新打开

        Ok(Self {
            data: Arc::new(RwLock::new(CacheData {
                apps: Vec::new(),
                name_index: HashMap::new(),
                loaded: false,
            })),
            db_path,
        })
    }

    /// 从数据库加载缓存
    pub fn load_from_db(&self) -> Result<()> {
        let db = Connection::open(&self.db_path)?;
        
        let mut stmt = db.prepare(
            "SELECT path, name, icon, app_type, pinyin, initials FROM apps"
        )?;

        let apps = stmt.query_map([], |row| {
            Ok(AppInfo {
                path: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                app_type: row.get(3)?,
                pinyin: row.get(4)?,
                initials: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

        // 构建索引
        let mut name_index = HashMap::new();
        for (i, app) in apps.iter().enumerate() {
            name_index.insert(app.name.to_lowercase(), i);
        }

        let mut data = self.data.write().map_err(|_| VoloError::Other("Lock error".to_string()))?;
        data.apps = apps;
        data.name_index = name_index;
        data.loaded = true;

        info!("Loaded {} apps from cache", data.apps.len());
        Ok(())
    }

    /// 扫描并更新缓存
    pub fn scan_and_update(&self) -> Result<()> {
        info!("Scanning applications...");

        let new_apps = scan_apps()?;
        let now = chrono::Utc::now().timestamp();

        // 更新数据库
        let mut db = Connection::open(&self.db_path)?;
        let tx = db.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO apps (path, name, icon, app_type, pinyin, initials, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
            )?;

            for app in &new_apps {
                stmt.execute(params![
                    &app.path,
                    &app.name,
                    &app.icon,
                    &app.app_type,
                    &app.pinyin,
                    &app.initials,
                    now
                ])?;
            }
        }
        tx.commit()?;

        // 更新内存缓存
        let mut name_index = HashMap::new();
        for (i, app) in new_apps.iter().enumerate() {
            name_index.insert(app.name.to_lowercase(), i);
        }

        let mut data = self.data.write().map_err(|_| VoloError::Other("Lock error".to_string()))?;
        data.apps = new_apps;
        data.name_index = name_index;
        data.loaded = true;

        info!("Updated {} apps in cache", data.apps.len());
        Ok(())
    }

    /// 异步加载（先从数据库加载，后台扫描更新）
    pub fn async_load(&self) {
        // 先从数据库加载
        if let Err(e) = self.load_from_db() {
            warn!("Failed to load from db: {}", e);
        }

        // 后台扫描更新
        let data = Arc::clone(&self.data);
        let db_path = self.db_path.clone();

        std::thread::spawn(move || {
            // 简单的延迟，避免启动时立即扫描
            std::thread::sleep(std::time::Duration::from_secs(2));

            if let Ok(new_apps) = scan_apps() {
                let now = chrono::Utc::now().timestamp();

                // 更新数据库
                if let Ok(mut db) = Connection::open(&db_path) {
                    if let Ok(tx) = db.transaction() {
                        {
                            let mut stmt = tx.prepare_cached(
                                "INSERT OR REPLACE INTO apps (path, name, icon, app_type, pinyin, initials, updated_at)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                            ).ok();

                            if let Some(ref mut s) = stmt {
                                for app in &new_apps {
                                    let _ = s.execute(params![
                                        &app.path,
                                        &app.name,
                                        &app.icon,
                                        &app.app_type,
                                        &app.pinyin,
                                        &app.initials,
                                        now
                                    ]);
                                }
                            }
                        }
                        let _ = tx.commit();
                    }
                }

                // 更新内存
                if let Ok(mut data_lock) = data.write() {
                    data_lock.apps = new_apps;
                    data_lock.loaded = true;
                }

                info!("Background scan completed");
            }
        });
    }

    /// 获取所有应用
    pub fn get_apps(&self) -> Vec<AppInfo> {
        self.data.read()
            .map(|d| d.apps.clone())
            .unwrap_or_default()
    }

    /// 检查是否已加载
    pub fn is_loaded(&self) -> bool {
        self.data.read().map(|d| d.loaded).unwrap_or(false)
    }

    /// 获取应用数量
    pub fn count(&self) -> usize {
        self.data.read().map(|d| d.apps.len()).unwrap_or(0)
    }
}

// ============ 平台特定扫描实现 ============

/// 扫描系统应用
#[cfg(target_os = "macos")]
fn scan_apps() -> Result<Vec<AppInfo>> {
    let mut apps = Vec::new();
    let app_dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];

    // 用户应用目录
    if let Some(user_apps) = dirs::home_dir().map(|h| h.join("Applications")) {
        if user_apps.exists() {
            apps.extend(scan_app_dir(&user_apps)?);
        }
    }

    // 系统应用目录
    for dir in app_dirs {
        if dir.exists() {
            apps.extend(scan_app_dir(&dir)?);
        }
    }

    // 去重（按路径）
    apps.sort_by(|a, b| a.path.cmp(&b.path));
    apps.dedup_by(|a, b| a.path == b.path);

    Ok(apps)
}

#[cfg(target_os = "macos")]
fn scan_app_dir(dir: &PathBuf) -> Result<Vec<AppInfo>> {
    let mut apps = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().map_or(false, |ext| ext == "app") {
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                // 生成拼音（简化版，后续可引入拼音库）
                let pinyin = generate_pinyin(&name);
                let initials = generate_initials(&name);

                // 图标延迟加载，先不提取
                apps.push(AppInfo {
                    name,
                    path: path.to_string_lossy().to_string(),
                    icon: None,
                    app_type: "app".to_string(),
                    pinyin: Some(pinyin),
                    initials: Some(initials),
                });
            }
        }
    }

    Ok(apps)
}

#[cfg(target_os = "windows")]
fn scan_apps() -> Result<Vec<AppInfo>> {
    // TODO: 实现 Windows 应用扫描
    // 1. 扫描注册表: HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall
    // 2. 扫描开始菜单: C:\ProgramData\Microsoft\Windows\Start Menu\Programs
    Ok(Vec::new())
}

#[cfg(target_os = "linux")]
fn scan_apps() -> Result<Vec<AppInfo>> {
    // TODO: 实现 Linux 应用扫描
    // 扫描 .desktop 文件: /usr/share/applications, ~/.local/share/applications
    Ok(Vec::new())
}

// ============ 拼音生成（简化版） ============

/// 生成拼音（简化版，仅处理常见中文）
/// TODO: 引入 pinyin crate 实现完整拼音转换
fn generate_pinyin(name: &str) -> String {
    // 简化实现：保留原始名称，后续可引入拼音库
    name.to_lowercase()
}

/// 生成首字母（简化版）
fn generate_initials(name: &str) -> String {
    let mut initials = String::new();
    for c in name.chars() {
        if c.is_ascii_alphabetic() {
            initials.push(c.to_ascii_lowercase());
        }
        // TODO: 处理中文字符的首字母
    }
    initials
}

// ============ Tauri Commands ============

/// 刷新应用缓存
#[tauri::command]
pub fn refresh_app_cache(cache: tauri::State<'_, AppCache>) -> Result<()> {
    cache.scan_and_update()
}

/// 获取应用数量
#[tauri::command]
pub fn get_app_count(cache: tauri::State<'_, AppCache>) -> usize {
    cache.count()
}

/// 获取应用图标
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn get_app_icon(path: String) -> Result<Option<String>> {
    crate::platform::macos::get_app_icon(&path)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn get_app_icon(_path: String) -> Result<Option<String>> {
    Ok(None)
}