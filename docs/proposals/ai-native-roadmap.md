# Volo AI-Native Roadmap

> Status: Proposal

## Guiding Principle

Do not rebuild Volo around AI.

Instead, evolve the runtime so AI features can reuse the same capabilities, permissions, extension lifecycle, and platform services as traditional plugins.

---

## Phase A — Stabilize the Existing Platform

Goals:

- launcher remains fast
- application search is stable
- traditional plugin lifecycle is reliable
- platform APIs are clear
- Rubick compatibility boundaries are documented

Do not block current product progress on AI architecture.

---

## Phase B — Runtime Foundations

Introduce:

- Capability Registry
- Provider abstraction
- Permission Engine
- Event Bus
- reversible registration/effects
- Extension Manifest v2

Primary objective:

> Separate "what Volo can do" from "who consumes the capability."

Deliverables may include:

```text
core/capability
core/permission
core/event
core/extension
```

---

## Phase C — Tool and Model Infrastructure

Introduce:

- Tool Registry
- capability-to-tool adapter
- LLM provider interface
- model configuration
- credential storage
- MCP client

Keep the Agent runtime minimal or absent.

Example:

```text
Capability
  ↓
Tool Adapter
  ↓
Tool Registry
```

---

## Phase D — Skill Runtime

Support reusable Skills.

Possible package:

```text
skill/
├── SKILL.md
├── resources/
└── scripts/
```

Features:

- discovery
- metadata
- tool requirements
- permission requirements
- resource loading
- versioning

Skills should be independently installable from UI plugins.

---

## Phase E — Agent Runtime

Introduce:

- Agent session
- context assembly
- Agent Tool binding
- model request loop
- tool calling
- permission requests
- cancellation
- durable session events

Do not initially optimize for multi-agent complexity.

Focus on reliable single-agent execution.

---

## Phase F — Workflow and Automation

Introduce:

- workflow definition
- deterministic steps
- AI steps
- triggers
- scheduler
- background execution
- retry/recovery

Example:

```text
Trigger
  ↓
Search Files
  ↓
OCR
  ↓
Summarize
  ↓
Save Result
```

---

## Phase G — Computer Use

Add computer-control providers behind a stable capability interface.

```text
computer.*
├── macOS AX provider
├── Windows UIA provider
└── Linux accessibility provider
```

Potential capabilities:

```text
computer.apps
computer.windows
computer.inspect
computer.click
computer.type
computer.shortcut
computer.screenshot
```

This should be high-risk and approval-aware.

---

## Phase H — Extension Marketplace

Evolve Volo Store beyond a traditional plugin market.

Potential categories:

```text
Apps
Commands
Tools
Skills
Agents
Workflows
MCP
```

All packages should share:

- identity
- versioning
- signatures/trust metadata
- permission declaration
- compatibility metadata
- update lifecycle

---

## Near-Term Priorities

The highest-value architecture questions to resolve before building a large AI UI are:

1. What is a Capability in Volo?
2. How is a Capability registered and discovered?
3. How are providers swapped?
4. What is the common Extension model?
5. How are runtime registrations disposed?
6. How are permissions represented?
7. How can one capability be exposed to Plugin API, Agent Tool, MCP, and CLI?
8. What remains in the legacy Rubick compatibility layer?

These decisions have more long-term value than adding a standalone AI chat page.

---

## Recommended First Technical Prototype

Build one end-to-end prototype around three capabilities:

```text
clipboard.read
filesystem.read
notification.show
```

Expose them through:

```text
1. Native Volo API
2. Tool Registry
3. Permission Engine
```

Then add one LLM provider and allow a simple Agent to call those tools.

This prototype will validate the architecture without requiring a large feature rewrite.
