# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
