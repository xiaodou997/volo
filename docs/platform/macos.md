# Volo macOS Platform

> Status: **Active baseline**
>
> Minimum OS: **macOS 26.0**
>
> Architecture: **Apple Silicon (arm64) only**

## Support contract

Volo's macOS platform contract starts at macOS 26.0 and does not maintain an Intel compatibility line.

| Contract | Value |
| --- | --- |
| Minimum runtime | macOS 26.0 |
| Architecture | arm64 / Apple Silicon only |
| Intel / x86_64 | Unsupported |
| Universal binary | Not produced |
| Tauri bundle minimum | `26.0` |
| Minimum-runtime CI | GitHub `macos-26` arm64 |
| Distribution channel | Direct DMG / GitHub Release |
| App Store | Not a target while `macOSPrivateApi` is required |

## Source of truth

The minimum OS lives in:

```text
src-tauri/tauri.conf.json
└─ bundle.macOS.minimumSystemVersion = 26.0
```

Tauri uses this value for the application's `LSMinimumSystemVersion` and the macOS deployment target during bundling.

The macOS release workflow additionally sets:

```text
MACOSX_DEPLOYMENT_TARGET=26.0
target=aarch64-apple-darwin
runner=macos-26
```

This makes the release contract explicit even if runner defaults change later.

## CI contract

`.github/workflows/macos-26-baseline.yml` builds a real `.app` on the macOS 26 arm64 runner and verifies:

1. the runner architecture is `arm64`;
2. `Info.plist -> LSMinimumSystemVersion` is exactly `26.0`;
3. the main Mach-O executable contains only `arm64`.

The native notification smoke also runs on `macos-26` so the minimum supported runtime is exercised directly.

## Local release builds

Use an Apple Silicon Mac:

```bash
MACOSX_DEPLOYMENT_TARGET=26.0 pnpm tauri build --target aarch64-apple-darwin
```

The repository release helper rejects macOS release builds from an Intel host.

## Modernization sequence

This baseline is the first step of the macOS 26+ modernization track:

```text
#51 platform baseline
 -> #52 Developer ID signing + notarization
 -> #53 objc2 native-layer consolidation
 -> #54 remove open/sips subprocesses
 -> #55 ScreenCaptureKit
 -> #56 notification callback hardening
 -> #57 macOS 26 / macOS 27 SDK platform gate
 -> #58 macOS 26+ freeze
```

Until #52 lands, the existing unsigned-release installation caveat remains in the README. Signing and notarization are deliberately a separate release-contract change.
