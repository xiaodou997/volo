# Volo M1 Freeze

> Status: **Frozen**
>
> Freeze date: **2026-09-20**
>
> Baseline commit: `1d82b6a7d8d6a6db677a322ea4f6bd7a0c50379f`
>
> Baseline PR: **#50 — fix(macos): route notifications through UNUserNotificationCenter**

## 1. M1 Goal

M1 freezes the first real unattended Automation loop:

```text
Automation schedule
  -> due claim / nextRunAt advance
  -> saved Workflow
  -> background permission enforcement
  -> headless Plugin Tool
  -> local fs.read / fs.list
  -> AI step
  -> notification.show
  -> Workflow Run History
```

The M1 contract is intentionally narrow. It proves the end-to-end runtime and permission boundaries before expanding the background capability surface.

## 2. Frozen Semantics

At this baseline:

- Automation supports `interval` and `daily` triggers.
- Enabled Automation records persist backend-owned `nextRunAt`.
- A due occurrence advances `nextRunAt` before Workflow execution so the same occurrence is not claimed twice during normal process operation.
- Missed historical occurrences are skipped; the scheduler does not replay a backlog.
- A scheduler batch runs with bounded concurrency.
- Optional retry policy uses bounded fixed backoff and never crosses the next regular occurrence.
- Background execution requires compatible persistent/Always permission grants for non-low-risk capabilities.
- Workflow Run History records Automation provenance without persisting Workflow input, tool output, file content, or AI output.
- macOS notification delivery uses `UNUserNotificationCenter` and reports authorization/delivery failures instead of silently returning success.

## 3. Evidence Chain

### PR #45 — Workflow run provenance

- Added `source=automation`, `automationId`, exact `scheduledFor`, and optional `retryAttempt`.
- Preserved legacy/manual history compatibility.
- Kept sensitive Workflow payloads out of persisted history.

Merged as: `dcd1a65b5b982a3566f7d4d260b007cdd060c7b8`

### PR #46 — Background AI timeout

- Added a 60-second hard timeout for AI steps in background Automation mode.
- Foreground/manual Workflow behavior remains unchanged.
- Timeout becomes a normal failed execution so existing retry semantics can handle it.

Merged as: `237c013d970444576a5d3fbe2c70172a2e659193`

### PR #47 — Real M1 background contract smoke

Covers one complete deterministic contract:

```text
claim
  -> manifest / Always authorization
  -> QuickJS headless plugin
  -> fs.list / fs.read
  -> AI step
  -> notification adapter
  -> Workflow Run History
```

Also verifies that a claimed occurrence cannot be claimed twice and that `nextRunAt` advances before execution.

Merged as: `e87a6b6db8a2cb709e9b84871a64bd4a6b37f317`

### PR #48 — Native macOS notification smoke

- Added a macOS-only Tauri smoke executable through the production `notification_show` ToolRegistry path.
- Added path-scoped GitHub Actions coverage on `macos-latest`.

Merged as: `5b36ee7cf54cb451a79c24d8b6a76f7a800a58c0`

### PR #49 — Notification permission primer

- Added observable notification permission/status UI.
- Added test-notification and system-settings entry points before the final real-device check.

Merged as: `0a048fce8f8feafeb4980e6cdc4e5c70f7aab477`

### PR #50 — Modern macOS notification path

- Replaced the legacy macOS notification path with `UNUserNotificationCenter`.
- Added real authorization-state handling and explicit failure propagation.
- Added Objective-C exception bridging so invalid notification registration fails as a readable Volo error instead of aborting.
- Updated the macOS smoke to skip environments without a valid signing authority.
- Restored Linux/macOS conditional imports and aligned the tree with CI rustfmt.

Merged as / **M1 baseline**:

`1d82b6a7d8d6a6db677a322ea4f6bd7a0c50379f`

## 4. Validation at Freeze

At the M1 baseline:

- PR #50 required checks were green before merge.
- The post-merge `macOS Notification Smoke` on `main` completed successfully.
- Real-device notification delivery was observed with a properly signed Volo build.
- The recurring M1 Automation produced completed Workflow history with Automation provenance and continued advancing `nextRunAt`.

## 5. Known M1 Boundaries

These are **not M1 regressions**; they are intentionally deferred reliability work:

- A claim is process-local/storage-atomic, but not yet a durable lease.
- A crash after advancing `nextRunAt` but before completing the Workflow can lose that occurrence.
- Scheduler scanning currently waits for the current bounded execution batch to finish.
- Misfire behavior is fixed rather than user-configurable.
- Retry attempts do not yet have a durable occurrence/run lineage.
- Long-running crash/restart/soak validation belongs to M2.

## 6. M2 Entry Criteria

M2 starts from this exact baseline and focuses on scheduler reliability before expanding destructive/background capabilities:

1. durable Automation claim / lease and crash recovery;
2. scheduler scan and execution-pool decoupling;
3. explicit misfire policy;
4. retry/occurrence lineage;
5. fault-injection and soak coverage.

Capabilities such as broader `shell.open` and `fs.write` background use should be expanded after these scheduler guarantees are established.

## 7. Freeze Rule

When investigating future regressions, compare against commit:

```text
1d82b6a7d8d6a6db677a322ea4f6bd7a0c50379f
```

Changes after that commit belong to M2 or later unless this document is explicitly amended.
