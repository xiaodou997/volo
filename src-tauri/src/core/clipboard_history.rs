//! 剪贴板历史模块

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use crate::error::{Result, VoloError};

/// 剪贴板项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: String,
    pub text: String,
    pub time: i64,
}

/// 剪贴板历史管理器
pub struct ClipboardHistory {
    items: Arc<Mutex<Vec<ClipboardItem>>>,
    last_text: Arc<Mutex<String>>,
}

impl ClipboardHistory {
    pub fn new() -> Self {
        Self {
            items: Arc::new(Mutex::new(Vec::new())),
            last_text: Arc::new(Mutex::new(String::new())),
        }
    }

    /// 加载历史记录
    pub fn load(&self, app: &AppHandle) -> Result<()> {
        let db_path = app.path().app_data_dir()?.join("clipboard_history.db");
        
        if db_path.exists() {
            let conn = rusqlite::Connection::open(&db_path)?;
            let mut stmt = conn.prepare(
                "SELECT id, text, time FROM clipboard_history ORDER BY time DESC LIMIT 100"
            )?;
            
            let items: Vec<ClipboardItem> = stmt.query_map([], |row| {
                Ok(ClipboardItem {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    time: row.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
            
            if let Ok(mut guard) = self.items.lock() {
                *guard = items.clone();
            }
            
            // 设置最后文本
            if let Some(first) = items.first() {
                if let Ok(mut last) = self.last_text.lock() {
                    *last = first.text.clone();
                }
            }
        }
        
        Ok(())
    }

    /// 保存历史记录
    pub fn save(&self, app: &AppHandle) -> Result<()> {
        let db_path = app.path().app_data_dir()?.join("clipboard_history.db");
        let conn = rusqlite::Connection::open(&db_path)?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS clipboard_history (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                time INTEGER NOT NULL
            )",
            [],
        )?;
        
        // 清空并重新插入
        conn.execute("DELETE FROM clipboard_history", [])?;
        
        if let Ok(items) = self.items.lock() {
            for item in items.iter().take(100) {
                conn.execute(
                    "INSERT INTO clipboard_history (id, text, time) VALUES (?1, ?2, ?3)",
                    rusqlite::params![&item.id, &item.text, item.time],
                )?;
            }
        }
        
        Ok(())
    }

    /// 添加项目
    pub fn add(&self, text: String) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        
        let mut items = self.items.lock()
            .map_err(|_| VoloError::Other("Failed to lock items".to_string()))?;
        
        // 检查是否已存在
        if let Some(pos) = items.iter().position(|item| item.text == text) {
            // 移动到顶部
            let item = items.remove(pos);
            items.insert(0, item);
        } else {
            // 添加新项目
            items.insert(0, ClipboardItem {
                id: uuid::Uuid::new_v4().to_string(),
                text,
                time: chrono::Utc::now().timestamp_millis(),
            });
            
            // 限制数量
            if items.len() > 100 {
                items.truncate(100);
            }
        }
        
        Ok(())
    }

    /// 获取所有项目
    pub fn get_all(&self) -> Vec<ClipboardItem> {
        self.items.lock().map(|items| items.clone()).unwrap_or_default()
    }

    /// 删除项目
    pub fn remove(&self, id: &str) -> Result<()> {
        let mut items = self.items.lock()
            .map_err(|_| VoloError::Other("Failed to lock items".to_string()))?;
        items.retain(|item| item.id != id);
        Ok(())
    }

    /// 清空所有
    pub fn clear(&self) {
        if let Ok(mut items) = self.items.lock() {
            items.clear();
        }
        if let Ok(mut last) = self.last_text.lock() {
            last.clear();
        }
    }

    /// 检查剪贴板
    pub fn check_clipboard(&self, app: &AppHandle) -> Result<bool> {
        let text = app.clipboard().read_text()
            .map_err(|e| VoloError::Other(e.to_string()))?;
        
        let mut last = self.last_text.lock()
            .map_err(|_| VoloError::Other("Failed to lock last_text".to_string()))?;
        
        if text != *last {
            *last = text.clone();
            self.add(text)?;
            return Ok(true);
        }
        
        Ok(false)
    }

    /// 启动监听
    pub fn start_monitoring(&self, app: AppHandle) {
        let items = self.items.clone();
        let last_text = self.last_text.clone();
        
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                
                // 读取剪贴板
                if let Ok(text) = app.clipboard().read_text() {
                    if !text.is_empty() {
                        let mut last = last_text.lock().unwrap();
                        if text != *last {
                            *last = text.clone();
                            
                            // 添加到历史
                            if let Ok(mut guard) = items.lock() {
                                // 检查是否已存在
                                if let Some(pos) = guard.iter().position(|item| item.text == text) {
                                    let item = guard.remove(pos);
                                    guard.insert(0, item);
                                } else {
                                    guard.insert(0, ClipboardItem {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        text,
                                        time: chrono::Utc::now().timestamp_millis(),
                                    });
                                    
                                    if guard.len() > 100 {
                                        guard.truncate(100);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

impl Default for ClipboardHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ============ Tauri Commands ============

#[tauri::command]
pub fn clipboard_history_get_all(
    history: tauri::State<'_, ClipboardHistory>,
) -> Vec<ClipboardItem> {
    history.get_all()
}

#[tauri::command]
pub fn clipboard_history_remove(
    history: tauri::State<'_, ClipboardHistory>,
    id: String,
) -> Result<()> {
    history.remove(&id)
}

#[tauri::command]
pub fn clipboard_history_clear(history: tauri::State<'_, ClipboardHistory>) {
    history.clear();
}

#[tauri::command]
pub fn clipboard_history_save(
    history: tauri::State<'_, ClipboardHistory>,
    app: AppHandle,
) -> Result<()> {
    history.save(&app)
}