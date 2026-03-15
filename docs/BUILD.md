# Volo 打包发布指南

## 本地打包

### 前置要求

- [Node.js](https://nodejs.org/) 20+
- [pnpm](https://pnpm.io/) 8+
- [Rust](https://rustup.rs/) 最新稳定版

### macOS

```bash
# 安装依赖
pnpm install

# 构建发布版本
pnpm tauri build

# 构建产物位于:
# src-tauri/target/release/bundle/macos/Volo.app
# src-tauri/target/release/bundle/dmg/Volo_*.dmg
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

1. 推送标签触发自动构建:
   ```bash
   git tag v1.0.0
   git push origin v1.0.0
   ```

2. GitHub Actions 将自动构建并创建 Release

3. 在 GitHub Releases 页面查看和编辑发布说明

## 签名配置

### macOS 代码签名

1. 在 Apple Developer 获取证书
2. 导入证书到钥匙串
3. 在 `src-tauri/tauri.conf.json` 中配置:
   ```json
   {
     "bundle": {
       "macOS": {
         "signingIdentity": "Developer ID Application: Your Name"
       }
     }
   }
   ```

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
- [ ] 创建 Git 标签
- [ ] 推送标签触发 GitHub Actions
- [ ] 验证所有平台的构建产物
- [ ] 发布 Release

## 常见问题

### macOS 构建失败

确保已安装 Xcode 命令行工具:
```bash
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
