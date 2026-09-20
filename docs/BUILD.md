# Volo 打包发布指南

## 本地打包

### 前置要求

- [Node.js](https://nodejs.org/) 20+
- [pnpm](https://pnpm.io/) 8+
- [Rust](https://rustup.rs/) 最新稳定版

### macOS

Volo 的 macOS 发布契约是：

- macOS 26.0+
- Apple Silicon（arm64）only
- 不生成 Intel / Universal 发布包

```bash
# 安装依赖
pnpm install

# 构建 Apple Silicon 发布版本
MACOSX_DEPLOYMENT_TARGET=26.0 pnpm tauri build --target aarch64-apple-darwin

# 构建产物位于:
# src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Volo.app
# src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Volo_*.dmg
```

也可以运行 `pnpm release:mac`；该脚本目标同样固定为 `aarch64-apple-darwin`。

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

1. 对齐版本号：确认 `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`package.json` 三处版本号一致，并在 `CHANGELOG.md` 定版
2. 推送标签触发自动构建:
   ```bash
   git tag v1.4.0
   git push origin v1.4.0
   ```
3. GitHub Actions 将自动构建三平台产物并创建 **draft** Release（带 updater 签名与 latest.json）；macOS job 固定运行在 `macos-26` arm64 runner，只构建 `aarch64-apple-darwin`，并强制 Developer ID 签名 + Apple notarization
4. macOS job 必须通过 `codesign --verify`、`stapler validate` 与 `spctl --assess` 后，才把该构建视为可发布
5. 在 GitHub Releases 页面检查产物、编辑发布说明后手动发布

## 自动更新（updater）

- 更新元信息：每个 Release 需包含 `latest.json`（tauri-action 检测到签名私钥 secret 后自动生成上传）
- 签名密钥：`TAURI_SIGNING_PRIVATE_KEY` 存于 GitHub 仓库 secrets；私钥本地备份于 `~/.tauri/volo-updater.key`（**勿入库、勿丢失**，丢失后旧版本将无法验证新更新包）
- 客户端行为：启动时静默检查（有更新发系统通知），设置页"关于与更新"可手动检查并一键更新重启

## 签名配置

### macOS Developer ID + 公证

正式 macOS Release 必须同时满足：

```text
Developer ID Application 签名
        +
Apple notarization
        +
stapled ticket
        +
Gatekeeper assessment
```

CI 不在 `tauri.conf.json` 中硬编码证书持有人名称。Workflow 导入证书后自动发现 `Developer ID Application: ...` identity，并通过 `APPLE_SIGNING_IDENTITY` 交给 Tauri。

GitHub Actions 需要以下 Repository Secrets：

| Secret | 内容 |
| --- | --- |
| `APPLE_CERTIFICATE` | Developer ID Application 的 `.p12` 文件 Base64 |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 `.p12` 时设置的密码 |
| `APPLE_API_ISSUER` | App Store Connect API Issuer ID |
| `APPLE_API_KEY` | App Store Connect API Key ID |
| `APPLE_API_KEY_P8` | 对应 `AuthKey_*.p8` 的完整文本内容 |

生成 `APPLE_CERTIFICATE`：

```bash
openssl base64 -A -in DeveloperIDApplication.p12
```

`.p8` 不做 Base64；把完整内容（包括 `BEGIN PRIVATE KEY` / `END PRIVATE KEY`）保存为 `APPLE_API_KEY_P8`。

仓库提供两个保护层：

1. `macOS Signing Preflight`：在 PR 阶段只验证证书能导入且 `notarytool` 能使用 API Key，不创建 Release。
2. `Release / build-macos`：缺任一 Secret 立即失败；构建后验证 `codesign`、`stapler`、`spctl`，并重新挂载 DMG 验证其中的 `Volo.app`。

Updater 的 `TAURI_SIGNING_PRIVATE_KEY` 与 Apple Developer ID 是两套独立签名系统，两者都必须保留。

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
- [ ] macOS: 验证 `LSMinimumSystemVersion=26.0` 且主可执行文件仅含 `arm64`
- [ ] macOS: `macOS Signing Preflight` 通过
- [ ] macOS: Developer ID / notarization / Gatekeeper 三项验证通过
- [ ] 创建 Git 标签
- [ ] 推送标签触发 GitHub Actions
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
