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
| Signing | Developer ID Application |
| Notarization | Apple notary service + stapled ticket |
| Distribution channel | Signed/notarized Direct DMG / GitHub Release |
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

## Release trust contract

Official macOS release artifacts are fail-closed:

```text
Developer ID Application certificate
  -> codesign
  -> Apple notarization
  -> stapled ticket
  -> Gatekeeper assessment
  -> draft GitHub Release
```

The trusted release gate runs on a developer Apple Silicon Mac, not in GitHub Actions. `pnpm release:mac` validates the local Developer ID identity and App Store Connect API credentials, lets Tauri sign/notarize the arm64 build, then validates the resulting `.app` directly and again from the mounted DMG. An unsigned or unnotarized macOS artifact is not considered a releasable Volo build.

The updater signature remains independent from Apple code signing:

```text
TAURI_SIGNING_PRIVATE_KEY  -> updater authenticity
Developer ID + notarization -> macOS platform trust
```

For #52, Apple platform trust is enforced locally. GitHub Actions intentionally does not receive Apple certificate/private-key material and does not build the official macOS DMG.

## Local release builds

The Developer ID certificate/private key lives in macOS Keychain. The App Store Connect `.p8` stays outside the repository, with only its issuer/key/path referenced from `~/.config/volo/release.env`.

```bash
pnpm release:mac:preflight
pnpm release:mac
pnpm release:mac:verify
```

The release script requires an Apple Silicon Mac running macOS 26+, sets `MACOSX_DEPLOYMENT_TARGET=26.0`, auto-discovers a valid Developer ID identity when one is not explicitly configured, validates `notarytool` credentials, and emits a local verification receipt.

The repository release helper rejects Intel macOS release builds.

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

#52 establishes the signing/notarization release contract as a local trusted gate. Subsequent modernization work must preserve it; neither local tooling nor any future CI migration may silently fall back to ad-hoc or unsigned distribution.
