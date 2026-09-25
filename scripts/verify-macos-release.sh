#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/src-tauri/target/aarch64-apple-darwin/release/bundle"
APP="${1:-$TARGET_DIR/macos/Volo.app}"
DMG="${2:-}"

fail() {
  echo "error: $*" >&2
  exit 1
}

[[ "$(uname -s)" == "Darwin" ]] || fail "release verification must run on macOS"
[[ -d "$APP" ]] || fail "Volo.app not found: $APP"

if [[ -z "$DMG" ]]; then
  DMG="$(find "$TARGET_DIR/dmg" -maxdepth 1 -type f -name '*.dmg' -print -quit 2>/dev/null || true)"
fi
[[ -n "$DMG" && -f "$DMG" ]] || fail "DMG not found"

PLIST="$APP/Contents/Info.plist"
[[ -f "$PLIST" ]] || fail "Info.plist not found"

minimum_version="$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$PLIST")"
[[ "$minimum_version" == "26.0" ]] ||
  fail "expected LSMinimumSystemVersion=26.0, got $minimum_version"

executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$PLIST")"
archs="$(lipo -archs "$APP/Contents/MacOS/$executable")"
[[ "$archs" == "arm64" ]] || fail "expected arm64-only Mach-O, got: $archs"

echo "Verifying Developer ID signature..."
codesign --verify --deep --strict --verbose=2 "$APP"
sign_info="$(codesign -dv --verbose=4 "$APP" 2>&1)"
printf '%s\n' "$sign_info"
printf '%s\n' "$sign_info" | grep -F "Authority=Developer ID Application:" >/dev/null ||
  fail "Developer ID Application authority not found"

echo "Verifying notarization ticket and Gatekeeper assessment..."
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=4 "$APP"

mount_point="$(mktemp -d /tmp/volo-dmg.XXXXXX)"
cleanup() {
  hdiutil detach "$mount_point" >/dev/null 2>&1 || true
  rmdir "$mount_point" >/dev/null 2>&1 || true
}
trap cleanup EXIT

hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$mount_point" >/dev/null
mounted_app="$mount_point/Volo.app"
[[ -d "$mounted_app" ]] || fail "Volo.app not found inside DMG"

codesign --verify --deep --strict --verbose=2 "$mounted_app"
xcrun stapler validate "$mounted_app"
spctl --assess --type execute --verbose=4 "$mounted_app"

receipt="$TARGET_DIR/macos/Volo-release-verification.txt"
{
  echo "Volo macOS release verification"
  echo "verified_at_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "macos=$(sw_vers -productVersion)"
  echo "architecture=$archs"
  echo "minimum_system_version=$minimum_version"
  echo "app=$APP"
  echo "dmg=$DMG"
  shasum -a 256 "$DMG"
} > "$receipt"

echo "macOS release verification passed."
echo "Receipt: $receipt"
