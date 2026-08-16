# Volo Extension and Capability Model

> Status: Proposal

## 1. Why This Model Exists

Traditional desktop plugin systems usually assume:

```text
Plugin = UI application + platform APIs
```

That model is increasingly insufficient because Volo may need to support:

- UI plugins
- launcher commands
- system tools
- AI Skills
- MCP integrations
- Agents
- Workflows
- background services

A single `Plugin` abstraction should therefore not be responsible for every extension type.

The recommended model separates:

- **Capability** — what the platform can do
- **Extension** — what is installed or contributed
- **Provider** — how a capability is implemented
- **Permission** — whether an extension may use it

---

## 2. Capability

A Capability represents a stable platform function.

Examples:

```text
clipboard.read
filesystem.read
filesystem.write
shell.execute
screen.capture
notification.show
llm.chat
computer.click
```

Capabilities should have:

- stable identifier
- input schema
- output schema
- permission metadata
- risk level
- provider binding
- optional tool schema
- audit policy

Conceptual definition:

```ts
interface Capability<I, O> {
  id: string
  description: string
  risk: RiskLevel

  execute(input: I, context: CapabilityContext): Promise<O>
}
```

---

## 3. Provider

A Provider implements one or more capability interfaces.

Example:

```text
filesystem
  ├── local provider
  ├── sandbox provider
  └── remote provider
```

The consumer should not need to know which provider is active.

Provider replacement enables:

- testing
- sandboxing
- remote execution
- platform-specific implementations
- enterprise policies

---

## 4. Extension

The common extension metadata should be small and stable.

Example:

```json
{
  "id": "com.example.translate",
  "name": "Translate",
  "version": "1.0.0",
  "contributes": {
    "commands": [],
    "tools": [],
    "skills": [],
    "agents": []
  },
  "permissions": [
    "clipboard.read",
    "network",
    "llm.chat"
  ]
}
```

The manifest should describe contribution points rather than assuming every extension owns a WebView.

---

## 5. Extension Types

### Command

A small executable action exposed to the launcher.

Characteristics:

- usually no persistent UI
- deterministic
- quick startup
- keyboard-oriented

---

### Plugin

A traditional Volo application extension.

Characteristics:

- may have UI
- can use WebView
- can consume capabilities
- may expose commands and tools

---

### Tool

A structured callable function.

Characteristics:

- typed input/output
- callable by Agent, Workflow, Plugin, or CLI
- should be auditable
- permission checks apply

---

### Skill

A reusable AI instruction package.

Possible contents:

```text
skill/
├── SKILL.md
├── resources/
└── scripts/
```

A Skill may reference tools and capabilities but should not be treated as a normal UI plugin.

---

### MCP

MCP should be treated as an integration protocol.

Recommended relationship:

```text
MCP Server
   ↓
MCP Adapter
   ↓
Volo Tool / Capability Registry
```

Volo should not use MCP as its internal capability representation.

---

### Agent

An Agent is an AI-driven extension that can:

- receive user goals
- assemble context
- call tools
- request permissions
- maintain session state
- report progress

An Agent should consume Volo capabilities through the same permission layer as traditional extensions.

---

### Workflow

A Workflow coordinates deterministic and AI steps.

Example:

```text
Trigger
  ↓
Search Files
  ↓
OCR
  ↓
LLM Summarize
  ↓
Save Note
```

Workflow steps should reuse the Tool and Capability registries.

---

## 6. Lifecycle

A unified extension lifecycle should support:

```text
discover
  ↓
validate
  ↓
resolve dependencies
  ↓
request/resolve permissions
  ↓
mount
  ↓
activate
  ↓
deactivate
  ↓
unmount
```

Every registration made during activation should be reversible.

Examples:

- command registration
- event listener
- tool registration
- provider registration
- prompt section
- background job
- shortcut

Unmounting the extension should automatically remove these effects.

---

## 7. Reversible Effects

Volo should borrow the general idea used by Cordis and other mature plugin systems:

> Every runtime registration should return or own a disposer.

Conceptual example:

```ts
const dispose = context.commands.register(command)

onUnload(() => {
  dispose()
})
```

A higher-level runtime can manage this automatically:

```ts
context.effect(() => {
  return context.commands.register(command)
})
```

Benefits:

- hot reload
- safe uninstall
- predictable lifecycle
- easier testing
- fewer leaked listeners and resources

---

## 8. Dependency Injection

Extensions should depend on service contracts rather than concrete implementations.

Conceptually:

```ts
defineExtension({
  requires: [
    "filesystem",
    "notification"
  ],

  activate(ctx) {
    ctx.filesystem.read(...)
    ctx.notification.show(...)
  }
})
```

This makes runtime composition easier and supports provider replacement.

---

## 9. Contribution Points

Recommended contribution points:

```text
commands
tools
views
skills
agents
workflows
providers
settings
searchProviders
triggers
```

The manifest should evolve around contribution points instead of one fixed plugin shape.

---

## 10. Compatibility

Volo should keep a compatibility layer for existing Rubick/uTools-style APIs when practical.

Recommended architecture:

```text
Legacy Plugin API
       ↓
Compatibility Adapter
       ↓
Volo Capability API
```

New extensions should target native Volo APIs.

This avoids forcing an immediate ecosystem migration while preventing the legacy API from becoming the long-term core.
