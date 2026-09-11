//! 文件系统 API

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::core::permission::{require, PermissionEngine};
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

/// 插件文件 API 的路径解析模式。
///
/// 系统/主窗口调用（plugin_id=None）保持历史行为；只有插件调用会经过下面的真实路径解析，
/// 确保 PermissionEngine 检查的 resource 与最终 I/O 实际使用的路径一致。
#[derive(Debug, Clone, Copy)]
enum PluginPathMode {
    /// 目标必须存在，并跟随最终符号链接。适用于 read/list。
    ExistingFollow,
    /// 目标可不存在；解析所有已存在祖先和符号链接。适用于 write/mkdir/exists。
    CreationFollow,
    /// 只解析父目录，保留最终目录项本身。适用于 remove，避免删除符号链接时误删其目标。
    EntryNoFollow,
}

/// 插件路径禁止显式 `..`。
///
/// 即便某个 `..` 最终仍落在授权目录内，也不接受这种含糊表达：权限提示、授权 resource 和
/// 实际 I/O 必须指向同一个规范路径。相对路径会先锚定到当前工作目录。
fn absolute_plugin_path(input: &str) -> Result<PathBuf> {
    let path = PathBuf::from(input);
    if path.as_os_str().is_empty() {
        return Err(VoloError::PermissionDenied(
            "Plugin filesystem path cannot be empty".to_string(),
        ));
    }

    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(VoloError::PermissionDenied(format!(
            "Plugin filesystem path contains parent traversal: {}",
            input
        )));
    }

    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// 规范化一个必须存在的插件路径，并解析最终符号链接。
fn canonicalize_existing_plugin_path(input: &str) -> Result<PathBuf> {
    let path = absolute_plugin_path(input)?;
    std::fs::canonicalize(&path).map_err(|e| {
        VoloError::Other(format!(
            "Failed to resolve plugin filesystem path '{}': {}",
            input, e
        ))
    })
}

/// 规范化一个可能尚不存在的插件路径。
///
/// 从目标向上找到第一个可通过 `symlink_metadata` 观察到的目录项：
/// - 普通已存在路径：canonicalize，解析所有祖先 symlink；
/// - symlink：也必须 canonicalize 成真实目标；broken symlink 直接拒绝；
/// - 目标不存在：把缺失尾部逐段追加到已 canonicalize 的最近祖先。
///
/// 这样 `/allowed/link/new.txt` 中若 `link -> /outside`，最终 resource 会成为
/// `/outside/new.txt`，不会再通过 `/allowed/**` 的字符串权限范围。
fn canonicalize_creation_plugin_path(input: &str) -> Result<PathBuf> {
    let path = absolute_plugin_path(input)?;
    let mut cursor = path.as_path();
    let mut missing: Vec<OsString> = Vec::new();

    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) => {
                let mut resolved = std::fs::canonicalize(cursor).map_err(|e| {
                    VoloError::PermissionDenied(format!(
                        "Plugin filesystem path resolves through an unavailable symlink '{}': {}",
                        input, e
                    ))
                })?;
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let name = cursor.file_name().ok_or_else(|| {
                    VoloError::Other(format!(
                        "Unable to find an existing ancestor for plugin path '{}'",
                        input
                    ))
                })?;
                missing.push(name.to_os_string());
                cursor = cursor.parent().ok_or_else(|| {
                    VoloError::Other(format!(
                        "Unable to resolve parent for plugin path '{}'",
                        input
                    ))
                })?;
            }
            Err(e) => {
                return Err(VoloError::Other(format!(
                    "Failed to inspect plugin filesystem path '{}': {}",
                    input, e
                )));
            }
        }
    }
}

/// 规范化父目录但保留最终目录项本身。
///
/// 删除 `/allowed/link`（link -> /outside/dir）时应删除 link，而不是把 path canonicalize 后
/// 递归删除 `/outside/dir`。同时父目录若自身是 symlink，仍会解析到真实位置以防 scope 逃逸。
fn canonicalize_entry_no_follow_plugin_path(input: &str) -> Result<PathBuf> {
    let path = absolute_plugin_path(input)?;
    let file_name = path.file_name().ok_or_else(|| {
        VoloError::PermissionDenied(format!(
            "Plugin filesystem path has no removable final component: {}",
            input
        ))
    })?;
    let parent = path.parent().ok_or_else(|| {
        VoloError::PermissionDenied(format!(
            "Plugin filesystem path has no parent: {}",
            input
        ))
    })?;

    let mut resolved_parent = std::fs::canonicalize(parent).map_err(|e| {
        VoloError::Other(format!(
            "Failed to resolve parent for plugin filesystem path '{}': {}",
            input, e
        ))
    })?;
    resolved_parent.push(file_name);
    Ok(resolved_parent)
}

/// 仅对插件调用启用安全路径解析；主窗口自身保持原路径语义。
fn resolve_io_path(
    plugin_id: Option<&str>,
    path: &str,
    mode: PluginPathMode,
) -> Result<PathBuf> {
    if plugin_id.is_none() {
        return Ok(PathBuf::from(path));
    }

    match mode {
        PluginPathMode::ExistingFollow => canonicalize_existing_plugin_path(path),
        PluginPathMode::CreationFollow => canonicalize_creation_plugin_path(path),
        PluginPathMode::EntryNoFollow => canonicalize_entry_no_follow_plugin_path(path),
    }
}

async fn require_path(
    app: &AppHandle,
    engine: &PermissionEngine,
    plugins: &PluginState,
    plugin_id: Option<&str>,
    capability: &str,
    path: &Path,
) -> Result<()> {
    let resource = path.to_string_lossy();
    require(
        app,
        engine,
        plugins,
        plugin_id,
        capability,
        Some(resource.as_ref()),
    )
    .await
}

/// 弹原生文件对话框前的准备：激活并聚焦主窗口。
/// 主窗口是 alwaysOnTop + skipTaskbar 的无边框浮动窗，独立弹出的文件面板
/// 可能被压在下层、或因焦点切换触发启动器的失焦隐藏
fn prepare_dialog(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

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
pub async fn fs_read(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
) -> Result<String> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::ExistingFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.read",
        &resolved,
    )
    .await?;

    let content = tokio::fs::read_to_string(&resolved)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;
    Ok(content)
}

/// 读取二进制文件（返回 base64）
#[tauri::command]
pub async fn fs_read_binary(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
) -> Result<String> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::ExistingFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.read",
        &resolved,
    )
    .await?;

    let content = tokio::fs::read(&resolved)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;
    let base64 = base64::engine::general_purpose::STANDARD.encode(&content);
    Ok(base64)
}

/// 写入文本文件
#[tauri::command]
pub async fn fs_write(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
    content: String,
) -> Result<()> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::CreationFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.write",
        &resolved,
    )
    .await?;

    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| VoloError::Other(e.to_string()))?;
    }
    tokio::fs::write(&resolved, &content)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;
    Ok(())
}

/// 写入二进制文件（从 base64）
#[tauri::command]
pub async fn fs_write_binary(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
    content: String,
) -> Result<()> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::CreationFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.write",
        &resolved,
    )
    .await?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&content)
        .map_err(|e| VoloError::Other(e.to_string()))?;

    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| VoloError::Other(e.to_string()))?;
    }

    tokio::fs::write(&resolved, &bytes)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;
    Ok(())
}

/// 检查文件是否存在
#[tauri::command]
pub async fn fs_exists(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
) -> Result<bool> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::CreationFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.read",
        &resolved,
    )
    .await?;

    let exists = tokio::fs::try_exists(&resolved)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;
    Ok(exists)
}

/// 创建目录
#[tauri::command]
pub async fn fs_mkdir(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
) -> Result<()> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::CreationFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.write",
        &resolved,
    )
    .await?;

    tokio::fs::create_dir_all(&resolved)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;
    Ok(())
}

/// 删除文件或目录。
/// 最终符号链接只删除 link 本身，不会在 canonicalize 后误删 link target。
#[tauri::command]
pub async fn fs_remove(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
) -> Result<()> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::EntryNoFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.write",
        &resolved,
    )
    .await?;

    let meta = tokio::fs::symlink_metadata(&resolved)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;

    if meta.file_type().is_symlink() {
        // Unix 上 remove_file 可删除目录 symlink；Windows 对目录 symlink 的行为不同，失败时回退 remove_dir。
        if let Err(file_err) = tokio::fs::remove_file(&resolved).await {
            tokio::fs::remove_dir(&resolved).await.map_err(|dir_err| {
                VoloError::Other(format!(
                    "Failed to remove symlink (file: {}; dir: {})",
                    file_err, dir_err
                ))
            })?;
        }
    } else if meta.is_dir() {
        tokio::fs::remove_dir_all(&resolved)
            .await
            .map_err(|e| VoloError::Other(e.to_string()))?;
    } else {
        tokio::fs::remove_file(&resolved)
            .await
            .map_err(|e| VoloError::Other(e.to_string()))?;
    }
    Ok(())
}

/// 列出目录内容
#[tauri::command]
pub async fn fs_list(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    path: String,
) -> Result<Vec<FileInfo>> {
    let resolved = resolve_io_path(
        plugin_id.as_deref(),
        &path,
        PluginPathMode::ExistingFollow,
    )?;
    require_path(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.read",
        &resolved,
    )
    .await?;

    let mut entries = tokio::fs::read_dir(&resolved)
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?;

    let mut files = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| VoloError::Other(e.to_string()))?
    {
        let path_str = entry.path().to_string_lossy().to_string();
        let name = entry.file_name().to_string_lossy().to_string();

        let meta = entry.metadata().await.ok();
        let file_type = meta
            .as_ref()
            .map(|m| if m.is_dir() { "directory" } else { "file" })
            .unwrap_or("unknown");
        let size = meta
            .as_ref()
            .and_then(|m| if m.is_file() { Some(m.len()) } else { None });
        let modified = meta.as_ref().and_then(|m| {
            m.modified().ok().map(|t| {
                let datetime: chrono::DateTime<chrono::Utc> = t.into();
                datetime.to_rfc3339()
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
pub async fn fs_pick_file(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    options: Option<PickOptions>,
) -> Result<Option<String>> {
    require(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.pick",
        None,
    )
    .await?;

    prepare_dialog(&app);
    let mut dialog = app.dialog().file();

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
pub async fn fs_pick_files(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
    options: Option<PickOptions>,
) -> Result<Vec<String>> {
    require(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.pick",
        None,
    )
    .await?;

    prepare_dialog(&app);
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
    Ok(file_paths
        .map(|paths| paths.into_iter().map(|p| p.to_string()).collect())
        .unwrap_or_default())
}

/// 选择文件夹
#[tauri::command]
pub async fn fs_pick_folder(
    app: AppHandle,
    engine: State<'_, PermissionEngine>,
    plugins: State<'_, PluginState>,
    plugin_id: Option<String>,
) -> Result<Option<String>> {
    require(
        &app,
        &engine,
        &plugins,
        plugin_id.as_deref(),
        "fs.pick",
        None,
    )
    .await?;

    prepare_dialog(&app);
    let folder_path = app.dialog().file().blocking_pick_folder();
    Ok(folder_path.map(|p| p.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "volo_fs_scope_test_{}_{}",
            name,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_plugin_path_rejects_parent_traversal() {
        let result = canonicalize_creation_plugin_path("allowed/../secret.txt");
        assert!(matches!(result, Err(VoloError::PermissionDenied(_))));
    }

    #[test]
    fn test_creation_path_uses_canonical_existing_ancestor() {
        let root = temp_dir("creation");
        let allowed = root.join("allowed");
        std::fs::create_dir_all(&allowed).unwrap();

        let requested = allowed.join("new/nested.txt");
        let resolved = canonicalize_creation_plugin_path(&requested.to_string_lossy()).unwrap();
        let expected = std::fs::canonicalize(&allowed)
            .unwrap()
            .join("new")
            .join("nested.txt");
        assert_eq!(resolved, expected);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn test_creation_path_resolves_symlink_parent_outside_scope() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("symlink_parent");
        let allowed = root.join("allowed");
        let outside = root.join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, allowed.join("link")).unwrap();

        let requested = allowed.join("link/new.txt");
        let resolved = canonicalize_creation_plugin_path(&requested.to_string_lossy()).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&outside).unwrap().join("new.txt"));
        assert!(!resolved.starts_with(std::fs::canonicalize(&allowed).unwrap()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn test_creation_path_rejects_broken_symlink() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("broken_symlink");
        let allowed = root.join("allowed");
        std::fs::create_dir_all(&allowed).unwrap();
        symlink(root.join("missing-target"), allowed.join("link")).unwrap();

        let requested = allowed.join("link/new.txt");
        let result = canonicalize_creation_plugin_path(&requested.to_string_lossy());
        assert!(matches!(result, Err(VoloError::PermissionDenied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_path_preserves_final_symlink_entry() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("remove_symlink");
        let allowed = root.join("allowed");
        let outside = root.join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let target = outside.join("target.txt");
        std::fs::write(&target, "secret").unwrap();
        let link = allowed.join("link.txt");
        symlink(&target, &link).unwrap();

        let resolved =
            canonicalize_entry_no_follow_plugin_path(&link.to_string_lossy()).unwrap();
        let expected = std::fs::canonicalize(&allowed).unwrap().join("link.txt");
        assert_eq!(resolved, expected);
        assert_ne!(resolved, std::fs::canonicalize(&target).unwrap());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_system_path_is_not_rewritten() {
        let raw = PathBuf::from("relative/../system-path");
        let resolved = resolve_io_path(None, raw.to_str().unwrap(), PluginPathMode::CreationFollow)
            .unwrap();
        assert_eq!(resolved, raw);
    }
}
