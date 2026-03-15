#!/bin/bash

# Volo 发布脚本
# 用于构建和发布 Volo 应用

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 版本号
VERSION=$(grep '"version"' src-tauri/tauri.conf.json | head -1 | sed 's/.*: "\(.*\)".*/\1/')
echo -e "${GREEN}Building Volo v${VERSION}${NC}"

# 检查依赖
echo -e "${YELLOW}Checking dependencies...${NC}"

if ! command -v pnpm &> /dev/null; then
    echo -e "${RED}pnpm is not installed. Please install it first.${NC}"
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Rust/Cargo is not installed. Please install it first.${NC}"
    exit 1
fi

# 清理旧的构建
echo -e "${YELLOW}Cleaning old builds...${NC}"
rm -rf src-tauri/target/release/bundle

# 安装依赖
echo -e "${YELLOW}Installing dependencies...${NC}"
pnpm install

# 构建前端
echo -e "${YELLOW}Building frontend...${NC}"
pnpm run build

# 构建 Tauri 应用
echo -e "${YELLOW}Building Tauri app...${NC}"
cd src-tauri

# 检测平台
PLATFORM=$(uname -s)
ARCH=$(uname -m)

echo -e "${GREEN}Building for ${PLATFORM} (${ARCH})${NC}"

# 构建发布版本
cargo tauri build

# 检查构建结果
if [ ! -d "target/release/bundle" ]; then
    echo -e "${RED}Build failed: bundle directory not found${NC}"
    exit 1
fi

echo -e "${GREEN}Build completed successfully!${NC}"

# 显示构建结果
echo -e "${YELLOW}Build artifacts:${NC}"
find target/release/bundle -type f -name "*.dmg" -o -name "*.app" -o -name "*.msi" -o -name "*.exe" -o -name "*.deb" -o -name "*.rpm" -o -name "*.AppImage" 2>/dev/null | while read -r file; do
    echo "  - $(basename "$file") ($(du -h "$file" | cut -f1))"
done

# 创建发布目录
RELEASE_DIR="../releases/v${VERSION}"
mkdir -p "${RELEASE_DIR}"

# 复制构建产物
echo -e "${YELLOW}Copying artifacts to ${RELEASE_DIR}...${NC}"
find target/release/bundle -type f \( -name "*.dmg" -o -name "*.app" -o -name "*.msi" -o -name "*.exe" -o -name "*.deb" -o -name "*.rpm" -o -name "*.AppImage" \) -exec cp {} "${RELEASE_DIR}/" \; 2>/dev/null || true

# 生成校验和
echo -e "${YELLOW}Generating checksums...${NC}"
cd "${RELEASE_DIR}"
shasum -a 256 * > checksums.txt 2>/dev/null || sha256sum * > checksums.txt 2>/dev/null || true

echo -e "${GREEN}Release v${VERSION} is ready in ${RELEASE_DIR}${NC}"
echo -e "${YELLOW}Files:${NC}"
ls -lh "${RELEASE_DIR}"

# 生成发布说明
cat > "${RELEASE_DIR}/RELEASE_NOTES.md" << EOF
# Volo v${VERSION} Release Notes

## 下载

### macOS
- DMG: Volo_${VERSION}_x64.dmg (Intel)
- DMG: Volo_${VERSION}_aarch64.dmg (Apple Silicon)

### Windows
- MSI: Volo_${VERSION}_x64_en-US.msi
- NSIS: Volo_${VERSION}_x64-setup.exe

### Linux
- DEB: volo_${VERSION}_amd64.deb
- RPM: volo-${VERSION}-1.x86_64.rpm
- AppImage: volo_${VERSION}_amd64.AppImage

## 校验和
See checksums.txt for SHA256 checksums.

## 安装说明

### macOS
1. 下载 .dmg 文件
2. 双击打开并将 Volo 拖到 Applications 文件夹
3. 首次运行可能需要右键点击并选择"打开"

### Windows
1. 下载 .msi 或 .exe 安装程序
2. 双击运行安装程序
3. 按照向导完成安装

### Linux
#### Debian/Ubuntu
\`\`\`bash
sudo dpkg -i volo_${VERSION}_amd64.deb
sudo apt-get install -f  # 修复依赖
\`\`\`

#### Fedora/RHEL
\`\`\`bash
sudo rpm -i volo-${VERSION}-1.x86_64.rpm
\`\`\`

#### AppImage
\`\`\`bash
chmod +x volo_${VERSION}_amd64.AppImage
./volo_${VERSION}_amd64.AppImage
\`\`\`

## 更新日志

See CHANGELOG.md for full changelog.
EOF

echo -e "${GREEN}Release notes generated: ${RELEASE_DIR}/RELEASE_NOTES.md${NC}"
