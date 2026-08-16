# Extension Manifest v2（草案）

> Status: Draft — 仅设计，v1.1 不实现。计划随 v1.2 的 Command 扩展类型一起落地。
> 背景详见同目录 `extension-capability-model.md`。

## 目标

从"插件 = UI 应用"演进为"扩展 = 贡献点声明"。Manifest 描述扩展贡献了什么（commands/tools/views/skills...），而不是假设每个扩展都拥有一个 WebView。

## 格式草案

```json
{
  "id": "com.example.translate",
  "name": "Translate",
  "version": "1.0.0",
  "manifestVersion": 2,
  "description": "划词翻译",
  "icon": "icon.png",

  "contributes": {
    "commands": [
      {
        "id": "translate",
        "name": "翻译",
        "keywords": ["translate", "fy"],
        "mode": "no-view",
        "run": "dist/command.js"
      }
    ],
    "views": [
      {
        "id": "main",
        "name": "翻译面板",
        "main": "index.html"
      }
    ]
  },

  "permissions": [
    "clipboard.read",
    "notification.show",
    "fs.read:~/Documents/**"
  ]
}
```

## 与 v1（现行 plugin.json）的差异

| 维度 | v1 | v2 |
|------|----|----|
| 入口 | `main` + `features[]`（隐含 UI） | `contributes.*` 贡献点，UI 只是其中一种 |
| 无 UI 扩展 | 不支持 | `commands` 的 `mode: "no-view"` |
| 权限 | 字符串数组，语义松散 | 同格式，但接 Permission Engine 强校验（v1.1 已具备） |
| 身份/版本 | 有 | 增加签名字段预留（`signature`，远期） |

## 兼容策略

- v1 manifest 继续可用，内部转换为 v2 内存模型（一个 `features[]` 条目 → 一个 `views` 贡献）
- rubick/uTools 风格 API 由 `src/bridge/pluginClient.ts` 的兼容 shim 继续提供，新扩展应直接使用原生 Volo API
- 迁移节奏：v1.2 支持 v2 解析 + Command 类型；内置插件随 v1.2 迁移作示范

## 待决议

- `no-view` command 的 JS 执行环境：主进程 Node-like sandbox vs Rust 内嵌 JS 引擎（如 boa/quickjs）vs 复用隐藏 WebView——v1.2 开工前需原型验证后定
- Workflow/Skill 的 contributes 形态待 v2.0 设计时补充
