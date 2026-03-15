//! 文件搜索模块

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use crate::error::{Result, VoloError};

/// 文件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub extension: Option<String>,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

/// 文件索引
#[derive(Debug, Clone)]
struct FileEntry {
    path: PathBuf,
    name: String,
    file_type: String,
    extension: Option<String>,
    name_lower: String,
}

/// 文件搜索器内部状态
struct FileSearcherInner {
    entries: Vec<FileEntry>,
    indexed: bool,
}

/// 文件搜索器
#[derive(Clone)]
pub struct FileSearcher {
    inner: Arc<Mutex<FileSearcherInner>>,
}

impl FileSearcher {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FileSearcherInner {
                entries: Vec::new(),
                indexed: false,
            })),
        }
    }

    /// 获取索引目录
    fn get_index_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // 用户主目录
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Documents"));
            dirs.push(home.join("Downloads"));
            dirs.push(home.join("Desktop"));
        }

        dirs
    }

    /// 索引文件
    pub fn index_files(&self) -> Result<()> {
        let mut entries = Vec::new();
        let index_dirs = Self::get_index_dirs();

        for dir in index_dirs {
            if dir.exists() {
                self.index_directory(&dir, &mut entries, 3)?; // 限制深度为 3
            }
        }

        // 更新索引
        if let Ok(mut inner) = self.inner.lock() {
            inner.entries = entries;
            inner.indexed = true;
        }

        Ok(())
    }

    /// 递归索引目录
    fn index_directory(&self, dir: &PathBuf, entries: &mut Vec<FileEntry>, depth: usize) -> Result<()> {
        if depth == 0 {
            return Ok(());
        }

        let read_dir = std::fs::read_dir(dir)
            .map_err(|e| VoloError::Other(format!("Failed to read directory: {}", e)))?;

        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // 跳过隐藏文件
            if name.starts_with('.') {
                continue;
            }

            let metadata = entry.metadata().ok();
            let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);

            let file_type = if is_dir { "directory" } else { "file" };
            let extension = path.extension().map(|e| e.to_string_lossy().to_string().to_lowercase());

            entries.push(FileEntry {
                path: path.clone(),
                name: name.clone(),
                file_type: file_type.to_string(),
                extension: extension.clone(),
                name_lower: name.to_lowercase(),
            });

            // 递归索引子目录
            if is_dir && depth > 1 {
                let _ = self.index_directory(&path, entries, depth - 1);
            }
        }

        Ok(())
    }

    /// 搜索文件
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<FileInfo>> {
        let inner = self.inner.lock()
            .map_err(|_| VoloError::Other("Failed to lock entries".to_string()))?;

        let query_lower = query.to_lowercase();
        let mut results: Vec<(FileEntry, i32)> = Vec::new();

        for entry in inner.entries.iter() {
            let mut score = 0;

            // 名称完全匹配
            if entry.name_lower == query_lower {
                score = 100;
            }
            // 名称开头匹配
            else if entry.name_lower.starts_with(&query_lower) {
                score = 80;
            }
            // 名称包含匹配
            else if entry.name_lower.contains(&query_lower) {
                score = 60;
            }
            // 扩展名匹配
            else if let Some(ref ext) = entry.extension {
                if ext == &query_lower {
                    score = 40;
                }
            }

            if score > 0 {
                // 文件夹优先
                if entry.file_type == "directory" {
                    score += 10;
                }

                results.push((entry.clone(), score));
            }
        }

        // 按分数排序
        results.sort_by(|a, b| b.1.cmp(&a.1));

        // 限制结果数量
        results.truncate(limit);

        // 转换为 FileInfo
        let file_infos: Vec<FileInfo> = results
            .into_iter()
            .map(|(entry, _)| {
                let metadata = std::fs::metadata(&entry.path).ok();
                let size = metadata.as_ref().and_then(|m| {
                    if m.is_file() { Some(m.len()) } else { None }
                });
                let modified = metadata.as_ref().and_then(|m| {
                    m.modified().ok().and_then(|t| {
                        let datetime: chrono::DateTime<chrono::Utc> = t.into();
                        Some(datetime.format("%Y-%m-%d %H:%M").to_string())
                    })
                });

                FileInfo {
                    path: entry.path.to_string_lossy().to_string(),
                    name: entry.name,
                    file_type: entry.file_type,
                    extension: entry.extension,
                    size,
                    modified,
                }
            })
            .collect();

        Ok(file_infos)
    }

    /// 是否已索引
    pub fn is_indexed(&self) -> bool {
        self.inner.lock().map(|i| i.indexed).unwrap_or(false)
    }

    /// 获取索引文件数量
    pub fn get_index_count(&self) -> usize {
        self.inner.lock().map(|i| i.entries.len()).unwrap_or(0)
    }
}

impl Default for FileSearcher {
    fn default() -> Self {
        Self::new()
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn file_search(
    searcher: tauri::State<'_, FileSearcher>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<FileInfo>> {
    searcher.search(&query, limit.unwrap_or(20))
}

#[tauri::command]
pub fn file_index(searcher: tauri::State<'_, FileSearcher>) -> Result<()> {
    searcher.index_files()
}

#[tauri::command]
pub fn file_index_status(searcher: tauri::State<'_, FileSearcher>) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "indexed": searcher.is_indexed(),
        "count": searcher.get_index_count()
    }))
}