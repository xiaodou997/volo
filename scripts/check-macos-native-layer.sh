#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

fail() {
  echo "macOS native-layer contract failed: $*" >&2
  exit 1
}

if grep -Eq '^[[:space:]]*objc[[:space:]]*=' src-tauri/Cargo.toml; then
  fail "legacy objc 0.2 direct dependency is forbidden"
fi

grep -Eq '^[[:space:]]*objc2-app-kit[[:space:]]*=' src-tauri/Cargo.toml ||
  fail "objc2-app-kit must be a direct macOS dependency"

[[ -f src-tauri/src/platform/macos/mod.rs ]] ||
  fail "platform/macos/mod.rs is missing"
[[ ! -f src-tauri/src/platform/macos.rs ]] ||
  fail "legacy flat platform/macos.rs must not return"
[[ ! -f src-tauri/src/api/notification_macos.rs ]] ||
  fail "native notification FFI must live under platform/macos"

violations="$(
  find src-tauri/src -type f -name '*.rs'     ! -path 'src-tauri/src/platform/macos/*' -print0 |
    xargs -0 grep -nE       'use[[:space:]]+(objc|objc2|objc2_[a-zA-Z0-9_]*|block2)::|msg_send!|class!|sel!'       || true
)"

if [[ -n "$violations" ]]; then
  echo "$violations" >&2
  fail "direct Objective-C/framework bindings found outside platform/macos"
fi

echo "macOS native-layer contract verified: objc2 FFI is contained in platform/macos."
