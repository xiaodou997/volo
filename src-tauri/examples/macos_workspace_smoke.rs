//! macOS NSWorkspace smoke.
//!
//! Exercises the production application-icon path against Finder.app and
//! verifies behavior parity with the previous 64px sips path.

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine as _;
    use std::io;
    use volo_lib::platform::macos::get_app_icon;

    const FINDER_APP: &str = "/System/Library/CoreServices/Finder.app";
    const PREFIX: &str = "data:image/png;base64,";
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    const EXPECTED_SIZE: u32 = 64;

    let data_uri = get_app_icon(FINDER_APP)?
        .ok_or_else(|| io::Error::other("Finder.app icon was not returned"))?;
    let payload = data_uri
        .strip_prefix(PREFIX)
        .ok_or_else(|| io::Error::other("application icon is not a PNG data URI"))?;
    let png = base64::engine::general_purpose::STANDARD.decode(payload)?;

    if !png.starts_with(PNG_SIGNATURE) || png.len() < 24 || &png[12..16] != b"IHDR" {
        return Err(io::Error::other("application icon payload is not a valid PNG").into());
    }

    let width = u32::from_be_bytes(png[16..20].try_into()?);
    let height = u32::from_be_bytes(png[20..24].try_into()?);
    if (width, height) != (EXPECTED_SIZE, EXPECTED_SIZE) {
        return Err(io::Error::other(format!(
            "expected {EXPECTED_SIZE}x{EXPECTED_SIZE} icon, got {width}x{height}"
        ))
        .into());
    }

    println!(
        "macOS workspace smoke passed: native Finder.app icon is {width}x{height} PNG ({} bytes)",
        png.len()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("macos_workspace_smoke must run on macOS");
}
