# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.10.0] - 2026-08-27

体验打磨迭代：AI 对话上下文与可操作性、搜索排序 frecency。

### Added
- AI 对话粘贴附件：追问输入框可直接粘贴截图/图片（多模态 vision 格式发送，≤10MB）和文本类文件（txt/md/json 等，内容并入消息正文带文件名标注，≤256KB）；输入栏上方附件 chips 可逐个移除；用户气泡显示图片缩略图与文件徽标；会话日志 user_input 记录 imageCount，回放页显示图片徽标，从回放续聊时历史快照保留图片上下文
- AI 回答可行动化：代码块悬停显示"复制"按钮（JS 注入，流式输出期间每次渲染后自动重新增强）；整条回答悬停气泡右上角一键复制（按钮 1.5s 反馈"已复制"）；回放页同样生效
- AI 对话交互收尾：回答气泡"引用"按钮把原文以 Markdown 引用块填入追问输入框并聚焦；工具结果悬停"复制结果"复制完整内容（显示版截断 200 字符，完整内容存 fullText）；会话报错后"重试"按钮以最后一次提问参数（含 skill 与图片附件）重新发起，末尾连续错误项自动清理
- 搜索排序 frecency：使用历史从仅记录应用泛化为任意结果项（插件功能/命令 key 为 `pluginId#id`，表结构不变零迁移，老应用数据自然兼容）；评分改为频率加成（封顶 20）+ 时间衰减（24h 内 +10、7 天内 +5）；插件功能/命令搜索从包含过滤改为分层打分排序（名称 100/80/60 > 关键词 50/40/30 > id 20，加 frecency 加成），同分时最近常用的排前面；选中插件功能/命令现在也记录使用历史；`record_app_usage` 命令更名为 `record_item_usage`

## [1.9.0] - 2026-08-27

AI-native 深化迭代：Skill Runtime（技能系统）全链路、插件热重载、MCP 远程 server 支持，另含 4 个体验修复。

### Added
- 应用图标与 logo（V 形飞燕设计，全平台图标集）
- 设置项"Dock 图标"：可关闭 Dock 栏图标只保留菜单栏托盘（macOS，切换 ActivationPolicy 立即生效）
- 从回放继续会话：每轮结束向会话日志落消息级历史快照（`history` 事件），回放页"继续对话"恢复完整上下文（含 tool 调用细节）后续聊；旧日志无快照时退化为问答对重建
- Agent 会话停止按钮：流式输出中途可中断（取消标志下沉到 SSE 读取循环，已输出内容保留，残句不入历史）
- 启动器空输入直达入口：搜索框为空时显示"AI 会话历史"，回车直接打开历史列表（可回放、继续对话），无需先发问
- List mode 二级动作面板：列表项可声明 `actions`（`[{id, title, description?, icon?}]`），Tab/→ 展开动作面板、←/Esc 收起（重触发 onRun 恢复列表）、Enter 仍是 onSelect 默认动作；插件侧新增 `rubick.command.onAction(cb)`（回传 itemId + actionId）；uuid-gen 1.3.0 示范接入
- Skill Runtime（技能系统）：技能 = 含 SKILL.md 的目录（frontmatter 声明 name/description/version + Markdown 指令正文）；Agent system prompt 按渐进披露只列技能目录，模型匹配意图后经内置工具 `skill_load`（Low 风险）加载完整指令执行；设置页"技能"区块（目录安装/列表/删除/打开目录）；附 weekly-report、translate-polish 两个示范技能
- 内置技能播种：启动时把打包的示范技能拷入应用技能目录（参照插件播种：已安装且版本一致的跳过、用户改过的正文不被覆盖，版本升级或副本损坏时覆盖重播）；skills/ 纳入打包资源
- `@技能名` 显式触发技能：搜索框输入 `@` 列出技能候选（回车补全输入），`@技能名 问题` 直达"问 AI"并跳过渐进披露——技能正文直接注入 system prompt 严格执行；Agent 页标题显示 `问 AI · @技能名`；技能不存在时报错不静默降级
- 插件热重载：监听插件目录文件变化（notify watcher + 500ms 防抖），自动重扫插件并广播 `plugins-changed`；打开中的插件视图强制重建（重新拉取 HTML 并重发 onPluginEnter），插件管理器列表自动刷新；目录安装插件后无需重启即可搜索到
- MCP Streamable HTTP transport：MCP server 配置新增 `url` 字段——非空走远程 HTTP（单 endpoint POST JSON-RPC，JSON / SSE 两种响应都支持，自动回带 Mcp-Session-Id），为空保持 stdio 本地子进程，老配置完全兼容；设置页表单可加远程 server（URL 校验）

### Fixed
- 修复设置项"失焦时隐藏"（hideOnBlur）保存后不生效的问题：失焦隐藏逻辑接入共享配置（App 启动时加载、设置保存后实时同步，无需重启）
- 修复设置页"关于与更新"区块使用字母 V 占位而非真实 logo 的问题
- 修复 Dock 图标关闭再开启后显示为终端 exec 图标的问题（Accessory→Regular 切换后重设 NSApplication 图标）
- 修复原生文件/目录选择框弹出时主窗口被失焦隐藏的问题：弹框前激活聚焦主窗口，且对话框打开期间全局抑制失焦隐藏（影响设置页技能安装、插件管理器目录安装）

## [1.8.0] - 2026-08-17

首个公开迭代发布（累积 v1.4–v1.8 的全部变更）。

### Added
- AI 助手全链路：流式输出（SSE 逐字渲染）、回答 Markdown 渲染（marked + DOMPurify 消毒）、多轮对话与上下文追问（AgentView 底部输入框，启动器入口始终新会话）
- 会话历史：sessions/*.jsonl 事件日志（30 天自动清理）、AgentView"历史"列表 + 只读回放时间线、`agent_new_session` / `agent_list_sessions` / `agent_read_session` 命令
- Tool（AI 工具）扩展类型：Manifest v2 `contributes.tools`，插件向内置 Agent 贡献 LLM function-calling 工具；`<pluginId>__<toolId>` 命名空间清洗注册；隐藏 iframe 执行，30s 超时；插件侧 `rubick.tool.onInvoke(handler)` API
- MCP client：stdio transport 手写 JSON-RPC 2.0（initialize / tools/list / tools/call），无新增第三方依赖；config.json `mcpServers` 配置；惰性连接、跨会话复用、退出统一回收子进程；`mcp__{server}__{tool}` 命名空间接入 Agent 统一分派
- 内置 Agent 工具扩充：`clipboard_write`（Low）、`fs_write`（High 审批）、`shell_open`（Medium 审批）
- Command 扩展 list mode：`contributes.commands[].mode: "list"`，命令常驻隐藏 iframe 向启动器返回可选择的结果列表；插件侧 `rubick.command.setList(items)` / `rubick.command.onSelect(cb)`；输入过滤防抖 150ms 重触发 onRun
- 自动更新：tauri-plugin-updater + GitHub Release 签名管线（minisign），启动时静默检查 + 设置页手动检查/一键更新
- 设置页："MCP 服务器"管理区块（配置即信任提示）、"关于与更新"区块（动态版本号、更新进度、会话日志入口）
- 示范插件 uuid-gen 1.2.0：command / tool / list 三种扩展形态全覆盖
- Agent 并发防护：会话进行中重复调用 agent_ask 直接报错

### Changed
- LLM 配置（base_url/model/API key）明文存本地 config.json，移除 keyring 依赖（简单通用、跨平台一致）
- run_agent_loop 改为调用方持有 messages，完整对话历史跨轮延续
- 会话日志 model_response 的 content 完整记录（回放保真）
- Agent system prompt 明确"匹配到工具必须调用，禁止凭记忆编造结果"

### Fixed
- 修复内置插件已安装旧版本时不随应用升级覆盖更新的问题（播种改为版本对比，副本损坏自动重装）
- 修复设置页 `save_config` 参数名错误（`config` → `newConfig`）导致主题/快捷键等设置保存静默失败
- 修复路径类工具（fs_read / fs_write / shell_open）不展开 `~` 主目录简写的问题，展开提前到权限审批前，弹窗展示真实路径
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
