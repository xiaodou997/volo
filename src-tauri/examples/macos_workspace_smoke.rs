//! macOS NSWorkspace smoke.
//!
//! Exercises the production application-icon path against Finder.app and
//! validates that the native AppKit bridge returns an actual PNG data URI.

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine as _;
    use std::io;
    use volo_lib::platform::macos::get_app_icon;

    const FINDER_APP: &str = "/System/Library/CoreServices/Finder.app";
    const PREFIX: &str = "data:image/png;base64,";
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    let data_uri = get_app_icon(FINDER_APP)?
        .ok_or_else(|| io::Error::other("Finder.app icon was not returned"))?;
    let payload = data_uri
        .strip_prefix(PREFIX)
        .ok_or_else(|| io::Error::other("application icon is not a PNG data URI"))?;
    let png = base64::engine::general_purpose::STANDARD.decode(payload)?;

    if !png.starts_with(PNG_SIGNATURE) {
        return Err(io::Error::other("application icon payload is not PNG").into());
    }

    println!(
        "macOS workspace smoke passed: native Finder.app icon is PNG ({} bytes)",
        png.len()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("macos_workspace_smoke must run on macOS");
}
