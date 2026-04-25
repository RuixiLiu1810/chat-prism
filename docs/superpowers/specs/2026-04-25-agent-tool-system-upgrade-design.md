# 2026-04-25 Agent Tool System Upgrade Design Spec (CallChain Observability Track)

## 1. Goal and Scope

This spec defines **Plan A** for call-chain observability in `claude-prism`, focused on `agent-core + agent-cli`.

Primary priorities:
1. Production troubleshooting (P0)
2. Developer debugging (P1)
3. Audit/export (P2)

In-scope:
- Turn-level trace model (`trace/span/event`) in `agent-core`
- Runtime in-memory trace retention and snapshot APIs
- CLI command surface for trace inspection/export
- JSON export format (versioned schema)

Out-of-scope (this phase):
- Desktop UI/adapter deep integration
- OpenTelemetry backend and external collector ingestion
- Dynamic plugin loading re-architecture
- Full enterprise approval-chain redesign

---

## 2. Current Baseline (Repository Reality)

Already present in codebase:
- Tool registry and handler abstractions in `agent-core` (`ToolHandler`, `ToolRegistry`)
- Event-driven payload model (`AgentEventPayload` and event sink abstraction)
- Approval suspend/resume lifecycle with pending turn persistence
- Single-level subagent execution support

Missing capability addressed by this spec:
- Structured, queryable call-chain trace with parent/child spans and explicit export path.

---

## 3. Design Principles

1. **Troubleshooting first**: always answer “why did this turn suspend/fail/stop?”
2. **Low coupling**: trace collection must not block turn execution
3. **Compatibility-first**: additive APIs and optional command paths
4. **YAGNI**: keep v1 schema minimal but evolvable
5. **Bounded memory**: finite in-memory ring retention by runtime/session

---

## 4. Architecture Overview

### 4.1 Core Components

1. `agent-core/src/callchain.rs` (new)
- Canonical trace domain model
- Span lifecycle helpers (`start_span`, `close_span`, `mark_error`, `finalize`)
- Export DTO generation

2. `AgentRuntimeState` integration
- In-memory trace store keyed by turn/session context
- Snapshot APIs for CLI inspection/export

3. Instrumentation points
- `turn_engine.rs`: turn root, tool batches/calls, approval suspend/resume, tool errors
- `providers/chat_completions.rs` + `providers/openai.rs`: provider round spans and retry markers
- `turn_runner.rs`: resumed turn linkage

4. `agent-cli` commands
- `/trace show`
- `/trace export [path]`
- `/trace clear`

### 4.2 Non-blocking Strategy

Trace recording uses best-effort writes against in-memory state.
If trace update fails, agent execution continues and a soft status can be emitted in debug paths.

---

## 5. Data Model

### 5.1 IDs and Relationships

- `trace_id`: unique per turn execution
- `span_id`: unique per span
- `parent_span_id`: optional
- `turn_id`: derived from tab/session context for convenience

### 5.2 Span Types (v1)

- `turn`
- `provider_round`
- `tool_batch`
- `tool_call`
- `approval_suspend`
- `turn_resume`
- `error`

### 5.3 Span Status

- `running`
- `ok`
- `error`
- `cancelled`
- `interrupted`

### 5.4 Required Span Fields

- `id`
- `trace_id`
- `parent_span_id`
- `span_type`
- `name`
- `status`
- `started_at`
- `ended_at`
- `attrs` (JSON object)
- `events` (array)

### 5.5 Trace Aggregate Fields

- `schema_version` (start with `1`)
- `trace_id`
- `tab_id`
- `local_session_id`
- `project_path`
- `provider`
- `model`
- `started_at`
- `ended_at`
- `outcome`
- `spans[]`
- `stats`:
  - `duration_ms`
  - `tool_call_count`
  - `approval_suspend_count`
  - `retry_count`
  - `error_count`

---

## 6. Runtime Storage and Lifecycle

### 6.1 Retention

- Default in-memory retention per tab/session: keep latest `N` traces (config constant)
- Evict oldest on overflow

### 6.2 Lifecycle Hooks

- Turn start: create trace + root `turn` span
- Provider attempt/round: add `provider_round`
- Tool execution:
  - batch start/end span
  - each tool call span with target summary and status
- Approval-required result:
  - add `approval_suspend` span
- Resume path:
  - add `turn_resume` span linked to new/continued turn trace (v1 allows same trace chain via attrs)
- Terminal completion/error/cancel:
  - close open spans
  - finalize trace outcome

### 6.3 Finalization Safety

`finalize_trace` closes any dangling running spans and marks them `interrupted`.
This guarantees exportable consistency even after abrupt termination.

---

## 7. CLI Contract

### 7.1 `/trace show`

Outputs concise summary for active/latest trace:
- trace_id
- outcome/status
- total duration
- tool/retry/suspend/error counts
- top-level span breakdown

### 7.2 `/trace export [path]`

- Default path if omitted:
  - `<project_path>/.agent/traces/<trace_id>.json`
- Creates parent dirs as needed
- Writes full structured JSON trace
- Returns deterministic error on path/permission/serialization failure

### 7.3 `/trace clear`

- Clears in-memory traces for current tab/session scope
- Does not delete previously exported files

---

## 8. Error Handling

1. Trace mutation failure:
- Do not fail turn
- Optionally emit debug status line

2. Export failure:
- Return explicit CLI error with code and message

3. Inconsistent state:
- `finalize` before export
- never panic on missing parent span; record orphan marker in attrs

---

## 9. Testing Strategy

### 9.1 Unit Tests (`agent-core`)

- Span tree parent/child construction
- Status transitions (`running -> ok/error/cancelled/interrupted`)
- Finalize auto-closes dangling spans
- Stats aggregation correctness

### 9.2 Integration Tests (`agent-core` + `agent-cli`)

- Turn with tool call + approval suspend + resume produces expected span sequence
- Retry path increments retry markers and stats

### 9.3 CLI Tests (`agent-cli`)

- `/trace show` prints expected summary fields
- `/trace export` creates file and valid JSON schema
- `/trace clear` empties in-memory snapshot for scope

---

## 10. Migration Plan

### Phase 1 (P0): Troubleshooting Minimum
- Introduce callchain domain model and runtime retention
- Instrument turn root, tool calls, approval suspend/resume, errors
- Add `/trace show` and `/trace export`

### Phase 2 (P1): Debugging Deepening
- Add provider round and retry spans
- Improve attrs for tool input summary and result footprint

### Phase 3 (P2): Audit Readiness
- Stabilize export schema with version guarantees
- Add stronger finalize behavior and edge-case tests

---

## 11. Compatibility Notes

- Existing event payload contracts remain valid
- Existing turn execution semantics unchanged
- New commands are additive and backward-compatible

---

## 12. Definition of Done

1. `core+cli` builds and tests pass
2. A suspended-and-resumed turn can be exported as one coherent trace JSON
3. `/trace show`, `/trace export`, `/trace clear` work in human mode
4. No regression in existing approval workflow and tool execution paths

