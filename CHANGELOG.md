# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] 1.7.0

### Added
- Agent 多轮对话：会话历史存于内存，AgentView 完成后可基于上下文追问；启动器"问 AI"入口始终开启新会话，AgentView 顶栏提供"新对话"按钮
- 会话历史回放：AgentView 顶栏"历史"入口列出过往会话（时间 + 首问预览），点击进入只读回放时间线
- 新命令：`agent_new_session` / `agent_list_sessions` / `agent_read_session`（session_id 防目录穿越校验）
- Agent 并发防护：会话进行中重复调用 agent_ask 直接报错

### Changed
- run_agent_loop 改为调用方持有 messages（完整对话历史跨轮延续）
- 会话日志 model_response 的 content 改为完整记录（回放保真），tool_result 仍截断

## [Unreleased] 1.6.0

### Added
- MCP client：stdio transport 手写 JSON-RPC 2.0（initialize / tools/list / tools/call），无新增第三方依赖
- config.json 新增 `mcpServers` 配置（command/args/env/enabled）；惰性连接、跨会话复用、退出时统一回收子进程
- MCP 工具接入 Agent：`mcp__{server}__{tool}` 命名空间，`AgentToolExecutor` 统一分派（内置 / MCP / 插件三层工具来源）
- 内置工具扩充：`fs_write`（fs.write/High 审批）、`shell_open`（shell.open/Medium 审批）、`clipboard_write`（clipboard.write/Low）
- 设置页"MCP 服务器"区块：列表/添加/删除/启用开关，配置即信任的提示文案

### Fixed
- 修复设置页 `save_config` 调用参数名错误（`config` → `newConfig`）导致主题/快捷键等设置保存静默失败的问题
- 修复路径类工具（fs_read / fs_write / shell_open）不展开 `~` 主目录简写导致写入失败的问题，展开提前到权限审批前，弹窗展示真实路径

## [Unreleased] 1.5.0

### Added
- Tool（AI 工具）扩展类型：Manifest v2 `contributes.tools`，插件向内置 Agent 贡献 LLM function-calling 工具
- 工具命名空间：`<pluginId>__<toolId>` 清洗注册（如 `volo-uuid-gen__gen_uuid`），跨插件防重名
- 工具执行模型：隐藏 iframe + postMessage 桥（复用 Command 执行环境），30s 超时自动销毁
- `plugin-tool-call` / `plugin_tool_result` 前后端往返协议；`get_plugin_tool_source` 命令（路径穿越防护）
- 插件侧 `rubick.tool.onInvoke(handler)` API
- 内置插件 uuid-gen 增加 `gen_uuid` 工具示范（Agent 可自动调用生成 UUID）

### Fixed
- 修复内置插件已安装旧版本时不随应用升级覆盖更新的问题（播种改为版本对比，副本损坏自动重装）
- Agent system prompt 明确"匹配到工具必须调用，禁止凭记忆编造结果"，并声明插件工具命名规则

## [1.4.0] - 2026-08-16

### Added
- LLM 流式输出：SSE 逐字渲染，工具调用分片合并不受影响
- Agent 回答 Markdown 渲染（marked + DOMPurify 消毒）
- Agent 会话事件日志：sessions/*.jsonl，覆盖输入/工具调用/结果/回答，30 天自动清理
- 自动更新：tauri-plugin-updater + GitHub Release 签名管线（minisign），启动时静默检查 + 设置页手动检查/一键更新
- 设置页"关于与更新"区块：动态版本号、更新进度、会话日志目录入口

### Fixed
- 修复直接粘贴输入（Cmd+V / 菜单粘贴）不触发"问 AI"入口与结果刷新的问题（macOS 原生编辑菜单 + paste 事件）
- 修复结果列表高度不随内容变化刷新的问题

## [1.3.0] - 2026-08-16

### Added
- AI 初体验：启动器"问 AI"入口 + 内置 Agent（OpenAI 兼容协议，OpenAI/DeepSeek/Ollama 均可接入）
- Tool Registry：capability 暴露为 LLM function-calling 工具（clipboard_read / fs_read / notification_show）
- Agent 工具调用走权限管道：principal `agent:builtin`，中高风险操作照常弹审批
- LLM 配置：base_url/model/API key 均存本地 config.json（明文，简单通用、跨平台一致）
- AgentView：工具调用时间线渲染，可取消会话
- 设置页"AI 设置"区块

## [1.2.0] - 2026-08-16

### Added
- Command（无界面）扩展类型：Manifest v2 `contributes.commands`，搜索即得、回车即执行
- 命令执行器：隐藏 opaque-origin iframe + postMessage 桥，复用权限管道，10s 超时自动销毁
- `get_plugin_command_source` 命令，路径穿越防护
- 内置示范插件 uuid-gen（生成 UUID 到剪贴板）

### Fixed
- 修复 `Plugin.path` 缺 serde default 导致插件目录扫描静默失败的问题

## [1.1.0] - 2026-08-16

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
