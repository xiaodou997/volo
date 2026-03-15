//! 通知 API

use serde::Deserialize;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use crate::error::Result;

#[derive(Debug, Deserialize)]
pub struct NotificationOptions {
    pub title: Option<String>,
    pub body: String,
    pub icon: Option<String>,
}

#[tauri::command]
pub async fn notification_show(
    app: AppHandle, 
    options: NotificationOptions
) -> Result<()> {
    let title = options.title.unwrap_or_else(|| "Volo".to_string());
    
    let mut builder = app.notification().builder()
        .title(&title)
        .body(&options.body);
    
    if let Some(icon_path) = options.icon {
        builder = builder.icon(&icon_path);
    }
    
    builder.show()
        .map_err(|e| crate::error::VoloError::Other(e.to_string()))?;
    
    Ok(())
}
