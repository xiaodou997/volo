# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] 1.2.0

### Added
- Command（无界面）扩展类型：Manifest v2 `contributes.commands`，搜索即得、回车即执行
- 命令执行器：隐藏 opaque-origin iframe + postMessage 桥，复用权限管道，10s 超时自动销毁
- `get_plugin_command_source` 命令，路径穿越防护
- 内置示范插件 uuid-gen（生成 UUID 到剪贴板）

### Fixed
- 修复 `Plugin.path` 缺 serde default 导致插件目录扫描静默失败的问题

## [Unreleased] 1.1.0

### Added
- Capability Registry：系统能力统一登记，带风险分级（Low/Medium/High/Critical）
- Permission Engine：allow/ask/deny 决策 × once/session/always 授权范围
- 运行时权限审批弹窗：插件触发中高风险能力时需用户确认
- 权限审计日志：Medium 及以上决策写入 audit.db（principal/capability/decision/scope/时间戳）
- 设置页"权限管理"区块：查看与撤销已授予的权限
- Always 授权持久化（permissions.json），Session 授权仅存活于内存

### Changed
- **安全边界重构**：插件 iframe 移除 `allow-same-origin`，插件 JS 不再能直接访问主窗口 `__TAURI__`
- 插件 API 全部改经 postMessage 桥转发，调用方身份（pluginId）由宿主附加，插件无法伪造
- 25 个插件面 API 命令接入统一权限守卫（`core/permission::require`）
- db_* 命令改用验证后的调用方身份分库，修复插件可伪造他人 pluginId 读库的问题
- 未在 plugin.json 声明的权限调用一律拒绝并记入审计

## [1.0.0] - 2024-03-15

### Added
- 应用搜索功能，支持 macOS 和 Windows
- 拼音和首字母搜索支持
- 文件搜索功能，索引 Documents/Downloads/Desktop
- 增量索引机制，优化搜索性能
- 插件系统，支持 iframe 插件
- 插件管理界面，支持安装/卸载/启用/禁用
- 剪贴板历史内置插件
- 设置页面，支持主题切换和快捷键配置
- 全局快捷键 Alt+R 呼出
- 系统托盘支持
- 多平台支持：macOS、Windows、Linux
- 启动优化，分阶段并行初始化

### Changed
- 优化搜索索引，使用 SQLite 持久化
- 优化启动速度，延迟加载非关键模块
- 改进窗口定位和大小调整

### Fixed
- 修复窗口失焦自动隐藏问题
- 修复 Retina 屏幕高度计算问题
- 修复快捷键重复触发问题

## [0.3.0] - 2024-03-10

### Added
- 文件搜索功能
- 插件系统基础实现
- 设置页面

## [0.2.0] - 2024-03-05

### Added
- 应用搜索功能
- 拼音搜索支持
- 全局快捷键

## [0.1.0] - 2024-03-01

### Added
- 项目初始化
- 基础窗口管理
- 系统托盘
