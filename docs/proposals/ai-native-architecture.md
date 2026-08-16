# Volo AI-Native Architecture

> Status: Proposal  
> Scope: Long-term architecture direction for Volo

## 1. Positioning

Volo should not be limited to being a lightweight Tauri rewrite of Rubick.

The recommended long-term positioning is:

> **Volo is an AI-native extensible desktop runtime for tools, plugins, automation, and agents.**

Volo should preserve the strengths of a traditional launcher and plugin platform while introducing an architecture that can also support AI-era extension types such as Skills, MCP servers, Agent Tools, Agents, and Workflows.

The key principle is:

> **AI is an extension consumer of Volo capabilities, not the foundation of the operating model.**

This allows Volo to remain useful and technically coherent even if the AI ecosystem, protocols, or model providers change.

---

## 2. Design Goals

Volo should evolve toward the following capabilities:

- Fast desktop launcher and search
- Traditional plugin runtime
- Unified system capability layer
- Explicit permission and approval model
- Extensible event and service architecture
- AI Tool registry
- LLM provider abstraction
- Skill runtime
- MCP integration
- Agent runtime
- Workflow and automation runtime
- Extension marketplace

The architecture should support deterministic and probabilistic features side by side.

```text
Deterministic                          AI-driven

Command                                Skill
Plugin                                 Agent
Tool                                   LLM
Workflow                               MCP
Search                                 Computer Use
```

Neither side should depend on the other.

---

## 3. Core Architecture

```text
                         Volo
                          │
                ┌─────────┴─────────┐
                │                   │
             Launcher             Runtime
                │                   │
        Search / Command      Capability Registry
                                    │
          ┌─────────────┬───────────┼─────────────┐
          │             │           │             │
       Plugin         Skill        MCP          Agent
          │             │           │             │
          └─────────────┴───────────┴─────────────┘
                                    │
                            Permission Engine
                                    │
                         Platform / Tauri / Rust
```

The most important long-term abstraction should be **Capability**, not Plugin.

---

## 4. Architectural Layers

### 4.1 Core

The core should own only stable platform primitives:

```text
core/
├── runtime
├── capability
├── permission
├── event
├── config
└── extension
```

Responsibilities:

- Runtime lifecycle
- Capability registration and discovery
- Permission checks
- Extension lifecycle
- Event dispatch
- Configuration
- Resource cleanup

The core should not contain model-provider-specific logic.

---

### 4.2 Capabilities

System functions should be modeled as capabilities.

Possible built-in capabilities:

```text
capabilities/
├── app
├── clipboard
├── filesystem
├── database
├── shell
├── screen
├── notification
├── browser
├── network
├── automation
├── llm
└── computer
```

Examples:

```text
clipboard.read
clipboard.write

filesystem.read
filesystem.write
filesystem.search

shell.execute

screen.capture

notification.show
```

A capability is platform functionality that can be consumed by multiple extension types.

---

### 4.3 Extensions

Volo should eventually support a unified extension model:

```text
Extension
├── Command
├── Plugin
├── Tool
├── Skill
├── MCP
├── Agent
└── Workflow
```

These extension types differ in behavior but share common platform concerns:

- identity
- version
- lifecycle
- permissions
- dependencies
- contributed capabilities
- consumed capabilities
- installation
- update
- discovery

---

## 5. Dependency Direction

The dependency direction should remain:

```text
Plugin
   ↓
Capability

Workflow
   ↓
Capability

Agent
   ↓
Capability

MCP Adapter
   ↓
Capability
```

Do not make system capabilities depend on Agent or LLM layers.

Bad:

```text
filesystem
   ↓
agent runtime
```

Recommended:

```text
agent runtime
   ↓
filesystem capability
```

This keeps the desktop platform independent from current AI implementations.

---

## 6. Search and AI

The main launcher should remain a fast deterministic interface.

Recommended routing:

```text
User Input
   ↓
Search / Intent Router
   │
   ├── Application
   ├── File
   ├── Command
   ├── Plugin
   ├── Tool
   └── AI Intent
```

AI should be used when deterministic routing is insufficient or when the task explicitly requires reasoning.

Example:

```text
> vscode
Visual Studio Code

> json
JSON Formatter

> translate hello
Translation command

> Compress the five newest photos on my desktop
AI task
```

Volo should not turn the launcher into a mandatory chat interface.

---

## 7. Provider Abstractions

Capabilities that may have multiple backends should use provider abstractions.

Example:

```text
FilesystemCapability
├── LocalFilesystemProvider
├── SandboxFilesystemProvider
└── RemoteFilesystemProvider
```

LLM:

```text
LLMCapability
├── OpenAIProvider
├── AnthropicProvider
├── GeminiProvider
├── DeepSeekProvider
└── LocalModelProvider
```

Computer control:

```text
ComputerCapability
├── MacAXProvider
├── WindowsUIAProvider
└── LinuxATProvider
```

Consumers should depend on the capability interface, not a concrete provider.

---

## 8. AI Runtime

The AI runtime should be an optional upper layer:

```text
ai/
├── llm
├── tool-registry
├── context
├── session
├── skill
├── agent
└── workflow
```

The first AI features should not require rewriting the core runtime.

A recommended progression is:

1. Tool Registry
2. LLM Provider
3. MCP Client
4. Skill Runtime
5. Session/Event Log
6. Agent Runtime
7. Workflow
8. Computer Use

---

## 9. Capability-to-Tool Bridge

A major Volo advantage can come from defining a capability once and exposing it through multiple interfaces.

```text
Capability Definition
        │
        ├── JavaScript SDK
        ├── Agent Tool
        ├── MCP Tool
        └── CLI
```

Example:

```text
clipboard.read
```

Traditional plugin:

```ts
await volo.clipboard.read()
```

Agent tool:

```json
{
  "name": "clipboard_read",
  "description": "Read clipboard content"
}
```

The same underlying capability should be reused.

---

## 10. Non-Goals

Volo should avoid the following architectural traps:

- Do not make every feature AI-driven.
- Do not make MCP the internal architecture.
- Do not hard-code a single LLM vendor.
- Do not replace the existing plugin system merely to add AI.
- Do not put the Agent Loop into the lowest-level core.
- Do not make the launcher a chat-only interface.
- Do not allow extensions to bypass the permission system.

---

## 11. Proposed Long-Term Structure

```text
volo/
├── core/
│   ├── runtime/
│   ├── capability/
│   ├── permission/
│   ├── event/
│   ├── config/
│   └── extension/
│
├── capabilities/
│   ├── app/
│   ├── clipboard/
│   ├── filesystem/
│   ├── database/
│   ├── shell/
│   ├── screen/
│   ├── notification/
│   ├── browser/
│   ├── network/
│   └── computer/
│
├── extensions/
│   ├── plugin/
│   ├── command/
│   ├── tool/
│   ├── skill/
│   ├── mcp/
│   ├── agent/
│   └── workflow/
│
├── ai/
│   ├── llm/
│   ├── tool-registry/
│   ├── context/
│   ├── session/
│   └── agent/
│
├── search/
├── platform/
│   ├── macos/
│   ├── windows/
│   └── linux/
└── app/
```

This is a target architecture, not a requirement for immediate directory reorganization.

The migration should be incremental.
