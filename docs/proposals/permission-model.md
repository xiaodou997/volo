# Volo Permission and Approval Model

> Status: Proposal

## 1. Goals

Volo's permission system should support both traditional plugins and AI-driven execution.

AI introduces a different risk profile because a model may decide which operation to perform at runtime.

The permission layer must therefore distinguish:

- what an extension is allowed to access
- how risky an operation is
- whether the user must approve it
- how long an approval is valid
- what must be logged

---

## 2. Permission Decisions

Recommended decisions:

```text
allow
ask
deny
```

Recommended approval scopes:

```text
once
session
always
```

Example UI:

```text
Volo Agent wants to execute:

shell.execute
rm ~/Downloads/test.txt

[Allow once]
[Allow for this session]
[Deny]
```

---

## 3. Risk Levels

Suggested default classification:

| Capability | Suggested Risk |
|---|---|
| `notification.show` | Low |
| `clipboard.read` | Medium |
| `screen.capture` | Medium |
| `filesystem.read` | Medium |
| `filesystem.write` | High |
| `shell.execute` | High |
| `network.request` | High |
| `email.send` | High |
| `computer.control` | Critical |

Risk level is metadata, not the final policy.

Enterprise or user policy may override it.

---

## 4. Permission Context

A permission decision should include context.

Conceptually:

```text
Principal
  +
Capability
  +
Resource
  +
Action
  +
Execution Context
```

Example:

```text
principal: agent:photo-organizer
capability: filesystem.write
resource: ~/Pictures/**
context: current-session
```

This is safer than a single global permission such as:

```text
filesystem = allowed
```

---

## 5. Principal Types

Possible principals:

```text
plugin:<id>
skill:<id>
agent:<id>
workflow:<id>
mcp:<server-id>
system
user
```

Every capability call should have an identifiable principal.

---

## 6. Execution Pipeline

Recommended flow:

```text
Caller
  ↓
Capability Registry
  ↓
Permission Policy
  ↓
Approval UI if required
  ↓
Provider
  ↓
Result
  ↓
Audit Log
```

Extensions should not directly invoke privileged Rust functions outside this pipeline.

---

## 7. Tool Calls

AI Tool execution should reuse the same pipeline:

```text
Agent
  ↓
Tool Registry
  ↓
Capability
  ↓
Permission Policy
  ↓
Provider
```

Do not create a second independent permission system for AI.

---

## 8. Auditability

High-risk actions should record:

- principal
- capability
- sanitized input summary
- timestamp
- permission decision
- provider
- success/failure
- related session
- user approval if applicable

This is especially important for:

- shell execution
- file writes
- system control
- email/message sending
- network actions using credentials

---

## 9. Sandboxing

Where possible, permission and sandboxing should be separate controls.

Permission answers:

> Is this operation allowed?

Sandbox answers:

> Even if allowed, where and how may it execute?

Example:

```text
shell.execute = allowed
```

does not imply unrestricted host shell access.

Possible providers:

```text
LocalShellProvider
SandboxShellProvider
RemoteShellProvider
```

---

## 10. Default Principle

Use least privilege.

Extensions should declare requested permissions, but runtime policy controls actual access.

A manifest declaration is a request, not an authorization.
