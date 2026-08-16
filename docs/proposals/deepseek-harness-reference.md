# DeepSeek Harness: Architecture Notes for Volo

> Status: Reference / Research  
> Project: `deepseek-ai/deepseek-harness`

## 1. Why It Is Relevant

DeepSeek Harness is an Agent Harness built around an "everything is a plugin" architecture.

Its direct product goal is different from Volo:

```text
DeepSeek Harness = Agent Harness
Volo             = Desktop Runtime / Launcher / Extension Platform
```

However, several of its architectural ideas are highly relevant to Volo.

The most valuable parts are not the Agent Loop itself.

The strongest references are:

- shared service context
- capability seams
- typed events
- dependency injection
- reversible effects
- replaceable providers
- tool execution pipeline
- approval and sandbox integration
- durable session event logs

---

## 2. Everything as Plugin

Harness treats components such as the following as plugins:

- model adapters
- tool registry
- session log
- agent loop

This produces a composable runtime rather than a monolithic agent core.

Volo should borrow the composability principle, but not necessarily the exact Cordis implementation.

Recommended Volo interpretation:

```text
Core runtime
   ↓
Service / Capability Registry
   ↓
Replaceable providers and extensions
```

---

## 3. Shared Context

Cordis uses a context that exposes services through stable keys.

Conceptually:

```ts
ctx.tools
ctx.llm
ctx.sessions
```

This reduces direct imports of concrete implementations.

Volo can adopt a similar service access pattern:

```ts
ctx.clipboard
ctx.filesystem
ctx.commands
ctx.tools
ctx.llm
```

The implementation language and mechanism can remain native to Volo.

---

## 4. Dependency Injection

Harness plugins declare which services they require.

The runtime activates a plugin when dependencies are available.

Volo can use the same idea for:

- extensions
- providers
- built-in modules
- AI runtime components

This removes manual initialization ordering from many modules.

---

## 5. Reversible Effects

Harness treats runtime registrations as reversible effects.

When a plugin unloads, registrations are automatically unwound.

This is highly valuable for Volo:

- plugin reload
- uninstall
- provider switching
- development mode
- event cleanup
- command cleanup
- tool cleanup

Volo should strongly consider making resource disposal a first-class runtime concept.

---

## 6. Capability Seams

Harness describes a capability seam using:

```text
Service Definition
      ↓
Service Provider
      ↓
Consumer
```

This is one of the most useful ideas for Volo.

Example:

```text
Filesystem Definition
      ↓
Local / Sandbox / Remote Provider
      ↓
Plugin / Tool / Agent / Workflow
```

Changing the provider should not require changing consumers.

---

## 7. Tool Pipeline

A mature tool runtime should have interception points.

Recommended conceptual pipeline:

```text
tool/call
  ↓
pre-execute
  ↓
permission
  ↓
execute
  ↓
post-execute
  ↓
tool/result
```

This makes it possible to add:

- approval
- logging
- telemetry
- sandbox wrapping
- policy
- retries
- redaction

without modifying every tool.

---

## 8. Session Event Log

Harness uses a durable event log as the source from which model-visible history can be reconstructed.

Volo should adopt a similar principle for Agent sessions:

```text
AgentSessionEvent
├── user_input
├── context
├── model_request
├── model_response
├── tool_call
├── permission_request
├── permission_result
├── tool_result
└── error
```

Benefits:

- replay
- resume
- debugging
- audit
- UI reconstruction
- token/cost analytics

A useful principle is:

> Anything that affected an AI decision should be reconstructable.

---

## 9. What Volo Should Not Copy

Do not directly copy the entire Harness architecture.

In particular:

- Volo should not make Agent Loop the product core.
- Volo does not need all Harness session/goal/subagent concepts immediately.
- Volo should not depend on Cordis unless a clear implementation advantage is demonstrated.
- Volo should not sacrifice native Rust/Tauri design just to mirror a TypeScript framework.
- Volo should not make AI mandatory for traditional plugin execution.

---

## 10. Recommended Takeaways

Strongly consider adopting the ideas of:

1. Capability registry
2. Provider abstraction
3. Service context
4. Dependency injection
5. Typed events
6. Reversible effects
7. Tool execution middleware
8. Permission/approval pipeline
9. Durable Agent session events

Do not start by implementing a full Agent Harness.

The first architecture work should strengthen the desktop runtime so both traditional extensions and AI extensions can reuse the same foundation.
