//! 错误处理模块

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoloError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Tauri error: {0}")]
    Tauri(#[from] tauri::Error),
    
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    
    #[error("Plugin error: {0}")]
    Plugin(String),
    
    #[error("Shortcut parse error: {0}")]
    ShortcutParse(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("{0}")]
    Other(String),
}

impl Serialize for VoloError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, VoloError>;
