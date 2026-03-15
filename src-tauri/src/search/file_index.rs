//! 优化的文件索引模块
//! 支持增量索引和文件系统监听

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use crate::error::{Result, VoloError};
use rusqlite::{Connection, params};
use tracing::{info, warn, debug};

/// 文件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub extension: Option<String>,
    pub size: u64,
    pub modified: i64,
}

/// 索引统计
#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    pub total_files: usize,
    pub total_dirs: usize,
    pub last_scan: Option<i64>,
    pub is_indexing: bool,
}

/// 文件索引器
pub struct FileIndex {
    db_path: PathBuf,
    index_dirs: Vec<PathBuf>,
    is_indexing: Arc<Mutex<bool>>,
}

impl FileIndex {
    pub fn new(app_data_dir: &Path) -> Result<Self> {
        let db_path = app_data_dir.join("file_index.db");
        let index = Self {
            db_path,
            index_dirs: Self::get_index_dirs(),
            is_indexing: Arc::new(Mutex::new(false)),
        };
        
        // 初始化数据库
        index.init_db()?;
        
        Ok(index)
    }

    /// 获取索引目录
    fn get_index_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Documents"));
            dirs.push(home.join("Downloads"));
            dirs.push(home.join("Desktop"));
        }
        
        dirs
    }

    /// 初始化数据库
    fn init_db(&self) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        
        // 文件表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                file_type TEXT NOT NULL,
                extension TEXT,
                size INTEGER NOT NULL,
                modified INTEGER NOT NULL,
                indexed_at INTEGER NOT NULL
            )",
            [],
        )?;
        
        // 索引统计表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS index_stats (
                id INTEGER PRIMARY KEY,
                last_scan INTEGER,
                total_files INTEGER DEFAULT 0
            )",
            [],
        )?;
        
        // 创建索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_files_name ON files(name)",
            [],
        )?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_files_type ON files(file_type)",
            [],
        )?;
        
        Ok(())
    }

    /// 增量索引
    pub fn incremental_index(&self) -> Result<IndexStats> {
        // 检查是否正在索引
        {
            let mut indexing = self.is_indexing.lock()
                .map_err(|_| VoloError::Other("Lock error".to_string()))?;
            if *indexing {
                return self.get_stats();
            }
            *indexing = true;
        }
        
        info!("Starting incremental file indexing...");
        let start_time = SystemTime::now();
        
        let mut new_files = 0;
        let mut updated_files = 0;
        let mut removed_files = 0;
        
        // 获取现有文件列表
        let existing_files = self.get_existing_files()?;
        let mut current_files: HashMap<String, i64> = HashMap::new();
        
        // 扫描目录
        for dir in &self.index_dirs {
            if dir.exists() {
                self.scan_directory_incremental(
                    dir,
                    &existing_files,
                    &mut current_files,
                    &mut new_files,
                    &mut updated_files,
                )?;
            }
        }
        
        // 删除不存在的文件
        for (path, _) in existing_files {
            if !current_files.contains_key(&path) {
                self.remove_file(&path)?;
                removed_files += 1;
            }
        }
        
        // 更新统计
        let total_files = self.count_files()?;
        let last_scan = chrono::Utc::now().timestamp();
        
        self.update_stats(last_scan, total_files)?;
        
        // 释放索引锁
        {
            let mut indexing = self.is_indexing.lock()
                .map_err(|_| VoloError::Other("Lock error".to_string()))?;
            *indexing = false;
        }
        
        let elapsed = start_time.elapsed().unwrap_or_default();
        info!(
            "Incremental indexing completed in {:?}: {} new, {} updated, {} removed, {} total",
            elapsed, new_files, updated_files, removed_files, total_files
        );
        
        self.get_stats()
    }

    /// 获取现有文件列表
    fn get_existing_files(&self) -> Result<HashMap<String, i64>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT path, modified FROM files"
        )?;
        
        let files: HashMap<String, i64> = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
        
        Ok(files)
    }

    /// 增量扫描目录
    fn scan_directory_incremental(
        &self,
        dir: &Path,
        existing: &HashMap<String, i64>,
        current: &mut HashMap<String, i64>,
        new_count: &mut usize,
        updated_count: &mut usize,
    ) -> Result<()> {
        self.scan_directory_recursive(dir, existing, current, new_count, updated_count, 3)?;
        Ok(())
    }

    /// 递归扫描目录
    fn scan_directory_recursive(
        &self,
        dir: &Path,
        existing: &HashMap<String, i64>,
        current: &mut HashMap<String, i64>,
        new_count: &mut usize,
        updated_count: &mut usize,
        depth: usize,
    ) -> Result<()> {
        if depth == 0 {
            return Ok(());
        }
        
        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                debug!("Failed to read directory {:?}: {}", dir, e);
                return Ok(());
            }
        };
        
        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            
            // 跳过隐藏文件
            if name.starts_with('.') {
                continue;
            }
            
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            
            let is_dir = metadata.is_dir();
            let modified = metadata.modified()
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            
            let path_str = path.to_string_lossy().to_string();
            current.insert(path_str.clone(), modified);
            
            if is_dir {
                // 递归扫描子目录
                self.scan_directory_recursive(&path, existing, current, new_count, updated_count, depth - 1)?;
            } else {
                // 检查是否需要更新
                if let Some(&existing_modified) = existing.get(&path_str) {
                    if existing_modified != modified {
                        // 文件已修改，更新
                        self.update_file(&path, &name, &metadata)?;
                        *updated_count += 1;
                    }
                } else {
                    // 新文件
                    self.add_file(&path, &name, &metadata)?;
                    *new_count += 1;
                }
            }
        }
        
        Ok(())
    }

    /// 添加文件到索引
    fn add_file(&self, path: &Path, name: &str, metadata: &std::fs::Metadata) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        let extension = path.extension()
            .map(|e| e.to_string_lossy().to_string().to_lowercase());
        
        conn.execute(
            "INSERT INTO files (path, name, file_type, extension, size, modified, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                path.to_string_lossy().to_string(),
                name,
                "file",
                extension,
                metadata.len() as i64,
                metadata.modified()
                    .unwrap_or(SystemTime::UNIX_EPOCH)
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                chrono::Utc::now().timestamp()
            ],
        )?;
        
        Ok(())
    }

    /// 更新文件索引
    fn update_file(&self, path: &Path, name: &str, metadata: &std::fs::Metadata) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        let extension = path.extension()
            .map(|e| e.to_string_lossy().to_string().to_lowercase());
        
        conn.execute(
            "UPDATE files SET 
                name = ?2,
                extension = ?3,
                size = ?4,
                modified = ?5,
                indexed_at = ?6
             WHERE path = ?1",
            params![
                path.to_string_lossy().to_string(),
                name,
                extension,
                metadata.len() as i64,
                metadata.modified()
                    .unwrap_or(SystemTime::UNIX_EPOCH)
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                chrono::Utc::now().timestamp()
            ],
        )?;
        
        Ok(())
    }

    /// 从索引删除文件
    fn remove_file(&self, path: &str) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// 搜索文件
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<FileInfo>> {
        let conn = Connection::open(&self.db_path)?;
        let query_lower = query.to_lowercase();
        
        // 使用 SQL 查询进行搜索
        let mut stmt = conn.prepare(
            "SELECT path, name, file_type, extension, size, modified 
             FROM files 
             WHERE name LIKE ?1 OR extension = ?2
             ORDER BY 
                CASE 
                    WHEN name = ?3 THEN 1
                    WHEN name LIKE ?4 THEN 2
                    ELSE 3
                END,
                modified DESC
             LIMIT ?5"
        )?;
        
        let files: Vec<FileInfo> = stmt.query_map(
            params![
                format!("%{}%", query_lower),
                query_lower,
                query_lower,
                format!("{}%", query_lower),
                limit
            ],
            |row| {
                Ok(FileInfo {
                    path: row.get(0)?,
                    name: row.get(1)?,
                    file_type: row.get(2)?,
                    extension: row.get(3)?,
                    size: row.get(4)?,
                    modified: row.get(5)?,
                })
            }
        )?
        .filter_map(|r| r.ok())
        .collect();
        
        Ok(files)
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> Result<IndexStats> {
        let conn = Connection::open(&self.db_path)?;
        
        let (last_scan, total_files): (Option<i64>, i64) = conn.query_row(
            "SELECT last_scan, total_files FROM index_stats WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or((None, 0));
        
        let is_indexing = *self.is_indexing.lock()
            .map_err(|_| VoloError::Other("Lock error".to_string()))?;
        
        Ok(IndexStats {
            total_files: total_files as usize,
            total_dirs: 0, // TODO: 统计目录
            last_scan,
            is_indexing,
        })
    }

    /// 更新统计
    fn update_stats(&self, last_scan: i64, total_files: usize) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        
        conn.execute(
            "INSERT OR REPLACE INTO index_stats (id, last_scan, total_files) VALUES (1, ?1, ?2)",
            params![last_scan, total_files as i64],
        )?;
        
        Ok(())
    }

    /// 统计文件数量
    fn count_files(&self) -> Result<usize> {
        let conn = Connection::open(&self.db_path)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files",
            [],
            |row| row.get(0),
        )?;
        
        Ok(count as usize)
    }

    /// 启动后台索引
    pub fn start_background_index(&self) {
        let this = self.clone();
        
        std::thread::spawn(move || {
            // 延迟启动，避免影响应用启动速度
            std::thread::sleep(Duration::from_secs(5));
            
            loop {
                if let Err(e) = this.incremental_index() {
                    warn!("Background indexing failed: {}", e);
                }
                
                // 每 60 秒检查一次
                std::thread::sleep(Duration::from_secs(60));
            }
        });
    }
}

impl Clone for FileIndex {
    fn clone(&self) -> Self {
        Self {
            db_path: self.db_path.clone(),
            index_dirs: self.index_dirs.clone(),
            is_indexing: Arc::new(Mutex::new(false)),
        }
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn file_index_search(
    index: tauri::State<'_, FileIndex>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<FileInfo>> {
    index.search(&query, limit.unwrap_or(20))
}

#[tauri::command]
pub fn file_index_stats(index: tauri::State<'_, FileIndex>) -> Result<IndexStats> {
    index.get_stats()
}

#[tauri::command]
pub fn file_index_refresh(index: tauri::State<'_, FileIndex>) -> Result<IndexStats> {
    index.incremental_index()
}