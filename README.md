# Volo

<p><img src="assets/logo-1024.png" width="128" alt="Volo logo"></p>

桌面效率启动器（Tauri 2 + Rust + Vue 3）：应用/文件搜索、可扩展插件系统、内置 AI 助手。

## 功能

- **启动器**：`Alt+R` 全局呼出，应用搜索（拼音/首字母）、文件搜索、使用历史排序
- **插件系统**：三种扩展形态——UI 插件（Webview 界面）、Command（一次性 / list mode 无界面命令）、Tool（向 AI 贡献工具）；权限系统（风险分级 + 运行时审批 + 审计日志），iframe 沙箱隔离
- **AI 助手**：OpenAI 兼容协议（OpenAI/DeepSeek/Ollama 均可接入），流式输出、Markdown 渲染、多轮对话、历史回放；工具调用走权限管道
- **MCP**：stdio MCP client，`config.json` 配置即可接入 MCP 工具生态
- **自动更新**：GitHub Release 签名管线，应用内一键更新

## 下载安装

从 [Releases](https://github.com/xiaodou997/volo/releases) 下载对应平台安装包。

**macOS 注意**：当前版本未做 Apple 开发者签名/公证，首次打开可能提示"已损坏"或"无法验证开发者"。安装后执行一次：

```bash
xattr -cr /Applications/Volo.app
```

即可正常打开（这是清除下载隔离属性，不影响应用本身）。

## 开发

```bash
pnpm install
pnpm tauri:dev        # 开发模式
pnpm build            # 前端构建
cd src-tauri && cargo test   # 后端测试
```

## 插件开发

见插件协议文档与内置示范插件 `plugins/uuid-gen/`（command / tool / list 三种形态全覆盖）。

## License

[Apache-2.0](LICENSE)
