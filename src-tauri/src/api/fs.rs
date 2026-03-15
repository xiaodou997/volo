//! 文件系统 API

use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;
use tauri::AppHandle;
use crate::error::Result;
use base64::Engine;

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PickOptions {
    pub multiple: Option<bool>,
    pub filters: Option<Vec<FileFilter>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// 读取文本文件
#[tauri::command]
pub async fn fs_read(path: String) -> Result<String> {
    let content = tokio::fs::read_to_string(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(content)
}

/// 读取二进制文件（返回 base64）
#[tauri::command]
pub async fn fs_read_binary(path: String) -> Result<String> {
    let content = tokio::fs::read(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    let base64 = base64::engine::general_purpose::STANDARD.encode(&content);
    Ok(base64)
}

/// 写入文本文件
#[tauri::command]
pub async fn fs_write(path: String, content: String) -> Result<()> {
    // 确保父目录存在
    if let Some(parent) = std::path::Path::new(&path).parent() {
        tokio::fs::create_dir_all(parent).await
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    }
    tokio::fs::write(&path, &content).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(())
}

/// 写入二进制文件（从 base64）
#[tauri::command]
pub async fn fs_write_binary(path: String, content: String) -> Result<()> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&content)
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;

    // 确保父目录存在
    if let Some(parent) = std::path::Path::new(&path).parent() {
        tokio::fs::create_dir_all(parent).await
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    }

    tokio::fs::write(&path, &bytes).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(())
}

/// 检查文件是否存在
#[tauri::command]
pub async fn fs_exists(path: String) -> Result<bool> {
    let exists = tokio::fs::try_exists(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(exists)
}

/// 创建目录
#[tauri::command]
pub async fn fs_mkdir(path: String) -> Result<()> {
    tokio::fs::create_dir_all(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    Ok(())
}

/// 删除文件或目录
#[tauri::command]
pub async fn fs_remove(path: String) -> Result<()> {
    let meta = tokio::fs::metadata(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;

    if meta.is_dir() {
        tokio::fs::remove_dir_all(&path).await
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    } else {
        tokio::fs::remove_file(&path).await
            .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    }
    Ok(())
}

/// 列出目录内容
#[tauri::command]
pub async fn fs_list(path: String) -> Result<Vec<FileInfo>> {
    let mut entries = tokio::fs::read_dir(&path).await
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;

    let mut files = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(|e| crate::error::VoloError::Other(e.to_string()))? {
        let path_str = entry.path().to_string_lossy().to_string();
        let name = entry.file_name().to_string_lossy().to_string();

        let meta = entry.metadata().await.ok();
        let file_type = meta.as_ref().map(|m| if m.is_dir() { "directory" } else { "file" }).unwrap_or("unknown");
        let size = meta.as_ref().and_then(|m| if m.is_file() { Some(m.len()) } else { None });
        let modified = meta.as_ref().and_then(|m| {
            m.modified().ok().and_then(|t| {
                let datetime: chrono::DateTime<chrono::Utc> = t.into();
                Some(datetime.to_rfc3339())
            })
        });

        files.push(FileInfo {
            path: path_str,
            name,
            file_type: file_type.to_string(),
            size,
            modified,
        });
    }

    Ok(files)
}

/// 选择文件
#[tauri::command]
pub async fn fs_pick_file(app: AppHandle, options: Option<PickOptions>) -> Result<Option<String>> {
    let mut dialog = app.dialog().file();

    // 添加过滤器
    if let Some(opts) = options {
        if let Some(filters) = opts.filters {
            for filter in filters {
                let exts: Vec<&str> = filter.extensions.iter().map(|s| s.as_str()).collect();
                dialog = dialog.add_filter(filter.name, &exts);
            }
        }
    }

    let file_path = dialog.blocking_pick_file();
    Ok(file_path.map(|p| p.to_string()))
}

/// 选择多个文件
#[tauri::command]
pub async fn fs_pick_files(app: AppHandle, options: Option<PickOptions>) -> Result<Vec<String>> {
    let mut dialog = app.dialog().file();

    if let Some(opts) = options {
        if let Some(filters) = opts.filters {
            for filter in filters {
                let exts: Vec<&str> = filter.extensions.iter().map(|s| s.as_str()).collect();
                dialog = dialog.add_filter(filter.name, &exts);
            }
        }
    }

    let file_paths = dialog.blocking_pick_files();
    Ok(file_paths.map(|paths| paths.into_iter().map(|p| p.to_string()).collect()).unwrap_or_default())
}

/// 选择文件夹
#[tauri::command]
pub async fn fs_pick_folder(app: AppHandle) -> Result<Option<String>> {
    let folder_path = app.dialog().file().blocking_pick_folder();
    Ok(folder_path.map(|p| p.to_string()))
}