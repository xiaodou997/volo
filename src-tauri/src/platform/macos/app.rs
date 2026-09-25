//! macOS AppKit integration.
//!
//! All direct AppKit FFI belongs in this module. Callers outside `platform::macos`
//! should use these safe wrappers instead of issuing Objective-C messages directly.

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::NSData;

use crate::error::{Result, VoloError};

static APP_ICON_PNG: &[u8] = include_bytes!("../../../icons/icon.png");

/// Restore Volo's application icon after switching activation policy.
///
/// AppKit objects are main-thread-only. Returning an error here makes an
/// accidental off-main-thread call observable instead of invoking AppKit
/// through unchecked Objective-C messages.
pub fn restore_application_icon() -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        VoloError::Other("macOS AppKit icon update must run on the main thread".into())
    })?;

    let data = NSData::with_bytes(APP_ICON_PNG);
    let image = NSImage::initWithData(NSImage::alloc(mtm), &data).ok_or_else(|| {
        VoloError::Other("failed to decode embedded macOS application icon".into())
    })?;
    let app = NSApplication::sharedApplication(mtm);

    // SAFETY: `image` is a live NSImage and AppKit requires this setter on
    // the main thread, guaranteed by `MainThreadMarker`.
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }

    Ok(())
}
