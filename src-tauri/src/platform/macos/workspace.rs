//! macOS workspace integration.
//!
//! Finder reveal and application-icon lookup are backed by AppKit directly.
//! Do not reintroduce `open`, `sips`, or other helper subprocesses here.

use base64::Engine as _;
use block2::RcBlock;
use objc2::runtime::{AnyObject, Bool};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSImage, NSWorkspace,
};
use objc2_foundation::{NSArray, NSDictionary, NSRect, NSSize, NSString, NSURL};
use std::path::Path;
use tracing::debug;

use crate::error::{Result, VoloError};

const ICON_SIZE: f64 = 64.0;
const PNG_DATA_URI_PREFIX: &str = "data:image/png;base64,";

/// 获取应用图标（返回 64x64 base64 PNG）。
///
/// 使用 NSWorkspace 取得 Finder 实际展示的应用图标，因此可以覆盖 asset catalog、
/// bundle icon fallback 等现代 macOS 图标来源；随后在 AppKit 内原生重采样为 64x64，
/// 保持旧 sips --resampleWidth 64 路径的传输体积与调用契约。
pub fn get_app_icon(app_path: &str) -> Result<Option<String>> {
    let path = Path::new(app_path);

    if path.extension().is_none_or(|ext| ext != "app") || !path.is_dir() {
        return Ok(None);
    }

    let info_plist = path.join("Contents").join("Info.plist");
    if !info_plist.is_file() {
        debug!("Info.plist not found: {:?}", info_plist);
        return Ok(None);
    }

    let workspace = NSWorkspace::sharedWorkspace();
    let source = workspace.iconForFile(&NSString::from_str(app_path));

    let drawing_block: RcBlock<dyn Fn(NSRect) -> Bool> = RcBlock::new(move |rect: NSRect| {
        source.drawInRect(rect);
        Bool::YES
    });
    let resized = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(ICON_SIZE, ICON_SIZE),
        false,
        &drawing_block,
    );

    let tiff = resized.TIFFRepresentation().ok_or_else(|| {
        VoloError::Other(format!(
            "failed to obtain resized macOS application icon representation: {app_path}"
        ))
    })?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff).ok_or_else(|| {
        VoloError::Other(format!(
            "failed to decode resized macOS application icon representation: {app_path}"
        ))
    })?;

    let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
    let png = unsafe {
        // SAFETY: an empty properties dictionary is valid for PNG encoding and
        // contains no values whose Objective-C type could violate the generic contract.
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }
    .ok_or_else(|| {
        VoloError::Other(format!(
            "failed to encode macOS application icon as PNG: {app_path}"
        ))
    })?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(png.to_vec());
    Ok(Some(format!("{PNG_DATA_URI_PREFIX}{encoded}")))
}

/// 在 Finder 中显示并选中指定路径。
///
/// NSWorkspace 是 `open -R` 对应的原生 AppKit API；调用本身是异步的，
/// 与旧实现“成功提交给系统即返回”的语义一致。
pub fn show_in_finder(path: &str) -> Result<()> {
    let url = NSURL::from_file_path(path)
        .ok_or_else(|| VoloError::Other(format!("invalid macOS file path: {path}")))?;
    let urls = NSArray::from_retained_slice(&[url]);

    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    Ok(())
}
