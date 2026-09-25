#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

MODE="${1:-build}"
ENV_FILE="${VOLO_MACOS_RELEASE_ENV:-$HOME/.config/volo/release.env}"

if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$ENV_FILE"
fi

fail() {
  echo "error: $*" >&2
  exit 1
}

[[ "$(uname -s)" == "Darwin" ]] || fail "macOS release must run on macOS"
[[ "$(uname -m)" == "arm64" ]] || fail "Volo macOS release supports Apple Silicon (arm64) only"

major_version="$(sw_vers -productVersion | cut -d. -f1)"
[[ "$major_version" =~ ^[0-9]+$ ]] || fail "cannot determine macOS version"
(( major_version >= 26 )) || fail "macOS 26.0+ is required for the release gate"

for command in security xcrun pnpm cargo lipo; do
  command -v "$command" >/dev/null 2>&1 || fail "required command not found: $command"
done

identity="${APPLE_SIGNING_IDENTITY:-}"
if [[ -z "$identity" ]]; then
  identity="$(security find-identity -v -p codesigning |
    sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p' |
    head -n 1)"
fi

[[ -n "$identity" ]] || fail "no Developer ID Application identity found in the login keychain"
security find-identity -v -p codesigning |
  grep -F ""$identity"" >/dev/null ||
  fail "Developer ID identity is not currently usable: $identity"

: "${APPLE_API_ISSUER:?set APPLE_API_ISSUER or define it in $ENV_FILE}"
: "${APPLE_API_KEY:?set APPLE_API_KEY or define it in $ENV_FILE}"
: "${APPLE_API_KEY_PATH:?set APPLE_API_KEY_PATH or define it in $ENV_FILE}"
[[ -f "$APPLE_API_KEY_PATH" ]] || fail "APPLE_API_KEY_PATH does not exist: $APPLE_API_KEY_PATH"

echo "Validating Apple notarization credentials..."
xcrun notarytool history   --key "$APPLE_API_KEY_PATH"   --key-id "$APPLE_API_KEY"   --issuer "$APPLE_API_ISSUER"   --output-format json >/dev/null

echo "Developer ID identity: $identity"
echo "Notarization credentials: accepted"

if [[ "$MODE" == "--preflight" || "$MODE" == "preflight" ]]; then
  echo "macOS release preflight passed."
  exit 0
fi

[[ "$MODE" == "build" ]] || fail "usage: $0 [--preflight]"

export MACOSX_DEPLOYMENT_TARGET=26.0
export APPLE_SIGNING_IDENTITY="$identity"
export APPLE_API_ISSUER
export APPLE_API_KEY
export APPLE_API_KEY_PATH

echo "Building signed + notarized Volo for macOS 26+ / arm64..."
pnpm install --frozen-lockfile
pnpm build
pnpm tauri build --target aarch64-apple-darwin

"$ROOT_DIR/scripts/verify-macos-release.sh"

echo
echo "macOS release gate passed."
echo "Upload the verified DMG from:"
find "$ROOT_DIR/src-tauri/target/aarch64-apple-darwin/release/bundle/dmg"   -maxdepth 1 -type f -name '*.dmg' -print
