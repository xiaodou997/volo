# Volo 打包发布指南

## 本地打包

### 前置要求

- [Node.js](https://nodejs.org/) 20+
- [pnpm](https://pnpm.io/) 8+
- [Rust](https://rustup.rs/) 最新稳定版

### macOS

Volo 的 macOS 正式发布契约是：

- macOS 26.0+
- Apple Silicon（arm64）only
- Developer ID Application 签名
- Apple notarization + stapled ticket
- Gatekeeper 验收
- 不生成 Intel / Universal 发布包

macOS 正式包暂时**不在 GitHub Actions 内构建**，而是在可信开发者 Mac 本地完成签名、公证和验收。

首次配置：

1. 把 `Developer ID Application` 证书及私钥导入 macOS Keychain。
2. 把 App Store Connect 的 `AuthKey_*.p8` 保存在仓库之外。
3. 创建本地配置文件 `~/.config/volo/release.env`：

```bash
APPLE_API_ISSUER=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
APPLE_API_KEY=XXXXXXXXXX
APPLE_API_KEY_PATH=/absolute/path/to/AuthKey_XXXXXXXXXX.p8

# 可选；不填时脚本会自动发现第一个 Developer ID Application identity。
# APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
```

只验证本地环境和 Apple 凭据：

```bash
pnpm release:mac:preflight
```

构建正式 macOS 包：

```bash
pnpm release:mac
```

该命令会依次执行：

```text
本地 Keychain Developer ID
  -> notarytool 凭据预检
  -> Tauri arm64 build
  -> Developer ID codesign
  -> Apple notarization
  -> stapled ticket
  -> spctl Gatekeeper
  -> 挂载 DMG 再验证内部 Volo.app
```

如需只验已有构建产物：

```bash
pnpm release:mac:verify
```

默认产物：

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Volo.app
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Volo_*.dmg
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Volo-release-verification.txt
```

### Windows

```bash
# 安装依赖
pnpm install

# 构建发布版本
pnpm tauri build

# 构建产物位于:
# src-tauri/target/release/bundle/msi/Volo_*.msi
# src-tauri/target/release/bundle/nsis/Volo_*-setup.exe
```

### Linux

```bash
# 安装系统依赖 (Ubuntu/Debian)
sudo apt-get update
sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.0-dev libappindicator3-dev librsvg2-dev patchelf

# 安装依赖
pnpm install

# 构建发布版本
pnpm tauri build

# 构建产物位于:
# src-tauri/target/release/bundle/deb/volo_*.deb
# src-tauri/target/release/bundle/rpm/volo-*.rpm
# src-tauri/target/release/bundle/appimage/volo_*.AppImage
```

## 使用发布脚本

```bash
# 运行发布脚本
./scripts/build-release.sh

# 构建产物将位于 releases/v{version}/ 目录
```

## GitHub Actions 自动发布

当前采用**混合发布模式**：

- Windows / Linux：GitHub Actions 自动构建并上传到 draft Release。
- macOS：可信开发者 Mac 本地运行 `pnpm release:mac`，验收通过后手动上传 DMG。

流程：

1. 对齐版本号：确认 `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`package.json` 三处版本号一致，并在 `CHANGELOG.md` 定版。
2. macOS 本地先执行 `pnpm release:mac`，保存验证 receipt。
3. 推送标签触发 Windows / Linux 自动构建：
   ```bash
   git tag v1.4.0
   git push origin v1.4.0
   ```
4. GitHub Actions 创建或更新 **draft** Release。
5. 手动把本地已经签名、公证并验收通过的 macOS DMG 上传到同一个 draft Release。
6. 检查三平台产物后再发布 Release。

> 当前阶段 GitHub Actions 不生成 macOS updater artifact，因此 macOS 自动更新发布链暂时不是 #52 的验收范围；后续把 Apple 凭据迁回 CI 时再恢复。

## 自动更新（updater）

- 更新元信息：每个 Release 需包含 `latest.json`（tauri-action 检测到签名私钥 secret 后自动生成上传）
- 签名密钥：`TAURI_SIGNING_PRIVATE_KEY` 存于 GitHub 仓库 secrets；私钥本地备份于 `~/.tauri/volo-updater.key`（**勿入库、勿丢失**，丢失后旧版本将无法验证新更新包）
- 客户端行为：启动时静默检查（有更新发系统通知），设置页"关于与更新"可手动检查并一键更新重启

## 签名配置

### macOS Developer ID + 公证

macOS 的 Apple 凭据暂时只保存在开发者本机：

- Developer ID Application 私钥：macOS Keychain
- App Store Connect API 私钥：仓库之外的本地 `.p8`
- API Issuer / Key ID / Key Path：`~/.config/volo/release.env`

仓库和 GitHub Actions **不需要**保存 `APPLE_CERTIFICATE`、证书密码或 `.p8` 内容。

`scripts/macos-release.sh` 会自动发现 Keychain 中可用的 Developer ID identity，并用本地 `APPLE_API_KEY_PATH` 调用 Apple notarization。任何一项缺失都会在本机 fail-closed。

Updater 的 `TAURI_SIGNING_PRIVATE_KEY` 与 Apple Developer ID 是两套独立签名系统。当前 #52 只冻结 macOS 本地可信发布流程；macOS updater artifact 自动化留待后续 CI 恢复时处理。

### Windows 代码签名

1. 获取代码签名证书
2. 在 `src-tauri/tauri.conf.json` 中配置:
   ```json
   {
     "bundle": {
       "windows": {
         "certificateThumbprint": "YOUR_CERT_THUMBPRINT"
       }
     }
   }
   ```

## 自动更新

要启用自动更新:

1. 生成更新密钥对:
   ```bash
   cargo tauri signer generate
   ```

2. 在 `src-tauri/tauri.conf.json` 中配置:
   ```json
   {
     "plugins": {
       "updater": {
         "active": true,
         "endpoints": ["https://your-update-server.com/{{target}}/{{arch}}/{{current_version}}"],
         "dialog": true,
         "pubkey": "YOUR_PUBLIC_KEY"
       }
     }
   }
   ```

3. 设置环境变量:
   ```bash
   export TAURI_PRIVATE_KEY="path/to/private.key"
   export TAURI_KEY_PASSWORD="your-key-password"
   ```

## 发布检查清单

- [ ] 更新版本号 (`tauri.conf.json` 和 `package.json`)
- [ ] 更新 CHANGELOG.md
- [ ] 运行测试确保功能正常
- [ ] 构建并测试安装包
- [ ] macOS: `pnpm release:mac:preflight` 通过
- [ ] macOS: `pnpm release:mac` 完成
- [ ] macOS: receipt 确认 `LSMinimumSystemVersion=26.0`、`arm64`、Developer ID、notarization、Gatekeeper 全部通过
- [ ] 创建 Git 标签
- [ ] 推送标签触发 Windows / Linux GitHub Actions
- [ ] 将本地验收后的 macOS DMG 上传到 draft Release
- [ ] 验证所有平台的构建产物
- [ ] 发布 Release

## 常见问题

### macOS 构建失败

macOS 发布构建必须在 Apple Silicon Mac 上执行，并以 macOS 26.0 为最低部署目标。先确认架构与工具链：

```bash
uname -m
# 必须输出 arm64

xcode-select --install
```

### Windows 构建失败

确保已安装 Visual Studio 2022 和 C++ 工具链。

### Linux 构建失败

确保已安装所有必要的系统依赖，参见上面的安装命令。

### 图标问题

如果图标显示不正确，确保图标文件存在于 `src-tauri/icons/` 目录，并且格式正确:
- macOS: `.icns` 格式
- Windows: `.ico` 格式
- Linux: `.png` 格式
