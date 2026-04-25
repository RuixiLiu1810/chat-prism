# CallChain Observability (Plan A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add troubleshooting-first call-chain observability for `agent-core + agent-cli` with in-memory traces and explicit JSON export.

**Architecture:** Introduce a new `callchain` module in `agent-core` as the canonical trace domain model and lifecycle manager. Integrate lightweight span instrumentation into turn execution and provider loops, store traces in `AgentRuntimeState`, and expose CLI `/trace` commands for summary/export/clear.

**Tech Stack:** Rust 2021, tokio, serde/serde_json, existing `AgentRuntimeState`, `turn_engine`, providers, REPL/TUI command router.

---

## File Structure

### Create
- `crates/agent-core/src/callchain.rs` - Trace/span domain model, stats, finalize/export helpers.

### Modify
- `crates/agent-core/src/lib.rs` - Export callchain types/functions.
- `crates/agent-core/src/session.rs` - Add in-memory trace store and APIs.
- `crates/agent-core/src/turn_engine.rs` - Add turn/tool/approval/error span instrumentation.
- `crates/agent-core/src/providers/chat_completions.rs` - Add provider round/retry markers.
- `crates/agent-core/src/providers/openai.rs` - Add provider round/retry markers.
- `crates/agent-cli/src/command_router.rs` - Add `/trace` command parsing.
- `crates/agent-cli/src/main.rs` - Execute `/trace` commands in classic REPL mode.
- `crates/agent-cli/src/tui/shell.rs` - Execute `/trace` commands in TUI mode.
- `crates/agent-cli/src/output.rs` - Optional human formatting helper(s) for trace summary.

### Test
- `crates/agent-core/src/callchain.rs` unit tests.
- `crates/agent-core/src/session.rs` tests for trace store APIs.
- `crates/agent-cli/src/command_router.rs` tests for `/trace` parser.
- `crates/agent-cli` command handling tests (where present) for `/trace show|export|clear`.

---

### Task 1: Implement `agent-core` CallChain domain model

**Files:**
- Create: `crates/agent-core/src/callchain.rs`
- Modify: `crates/agent-core/src/lib.rs`

- [ ] **Step 1: Add core types**
Create:
- `CallTrace`, `CallSpan`, `CallSpanType`, `CallSpanStatus`, `CallTraceStats`
- `schema_version` constant
- span/event attrs as `serde_json::Value`

- [ ] **Step 2: Add lifecycle helpers**
Implement:
- `CallTrace::new(...)`
- `start_span(...) -> span_id`
- `close_span(...)`
- `mark_span_error(...)`
- `finalize(...)` (close dangling running spans as `interrupted`)

- [ ] **Step 3: Add stats aggregation**
Implement deterministic stat counters for:
- tool calls
- suspends
- retries
- errors
- duration ms

- [ ] **Step 4: Add unit tests**
Test cases:
- parent/child linkage
- finalize interruption behavior
- stats count correctness

- [ ] **Step 5: Export module**
Wire into `lib.rs` with public exports.

- [ ] **Step 6: Verify**
Run: `cargo test -p agent-core callchain`
Expected: all callchain tests pass.

---

### Task 2: Add trace retention APIs to runtime state

**Files:**
- Modify: `crates/agent-core/src/session.rs`

- [ ] **Step 1: Extend runtime state storage**
Add bounded in-memory trace store keyed by tab/session context.

- [ ] **Step 2: Add APIs**
Add methods (or equivalent names):
- `start_trace(...) -> trace_id`
- `trace_start_span(...)`
- `trace_close_span(...)`
- `trace_mark_error(...)`
- `finalize_trace(...)`
- `latest_trace_snapshot(...)`
- `clear_traces_for_tab(...)`

- [ ] **Step 3: Enforce retention cap**
Evict oldest trace when per-scope count exceeds configured default.

- [ ] **Step 4: Add tests**
Add tests for:
- snapshot non-consuming behavior
- retention eviction
- clear behavior

- [ ] **Step 5: Verify**
Run: `cargo test -p agent-core session::tests`
Expected: updated session tests pass.

---

### Task 3: Instrument turn engine and providers

**Files:**
- Modify: `crates/agent-core/src/turn_engine.rs`
- Modify: `crates/agent-core/src/providers/chat_completions.rs`
- Modify: `crates/agent-core/src/providers/openai.rs`

- [ ] **Step 1: Turn-level instrumentation**
In turn flow:
- create trace at turn start
- create root `turn` span
- finalize trace on complete/error/cancel

- [ ] **Step 2: Tool execution instrumentation**
In `execute_tool_calls` and result handling:
- create `tool_batch` span per batch
- create `tool_call` span per tool
- mark status and attrs (tool name, target summary, call_id)

- [ ] **Step 3: Approval/resume instrumentation**
When approval required:
- add `approval_suspend` span
When resumed:
- add `turn_resume` span

- [ ] **Step 4: Provider round/retry instrumentation**
Add `provider_round` spans in both providers.
Record retry markers/events in attrs/events.

- [ ] **Step 5: Error path instrumentation**
Ensure failures emit `error` span markers without breaking runtime behavior.

- [ ] **Step 6: Verify**
Run: `cargo test -p agent-core`
Expected: full `agent-core` tests pass.

---

### Task 4: Add CLI `/trace` command surface

**Files:**
- Modify: `crates/agent-cli/src/command_router.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Modify: `crates/agent-cli/src/tui/shell.rs`
- (Optional) Modify: `crates/agent-cli/src/output.rs`

- [ ] **Step 1: Command parser**
Add commands:
- `/trace show`
- `/trace export [path]`
- `/trace clear`

- [ ] **Step 2: Classic REPL handlers**
Implement handlers in `main.rs`:
- show summary from latest trace
- export JSON to default or provided path
- clear traces for current tab/session

- [ ] **Step 3: TUI handlers**
Mirror the same behavior in TUI command path with concise output.

- [ ] **Step 4: Default export path**
If path omitted: `<project_path>/.agent/traces/<trace_id>.json`
Create parent directories if needed.

- [ ] **Step 5: Parser tests**
Extend parser tests for `/trace` variants and invalid usage.

- [ ] **Step 6: Verify**
Run: `cargo test -p agent-cli command_router`
Expected: parser tests pass.

---

### Task 5: Add end-to-end trace export validation

**Files:**
- Modify tests under `crates/agent-core` and/or `crates/agent-cli`

- [ ] **Step 1: Suspended->resumed scenario test**
Construct/execute a scenario that includes:
- tool call
- approval suspend
- resume
- completion

- [ ] **Step 2: Validate exported JSON shape**
Assert required keys:
- `schema_version`, `trace_id`, `spans`, `stats`, `outcome`

- [ ] **Step 3: Validate span sequence**
Assert presence of:
- `turn`, `tool_call`, `approval_suspend`, `turn_resume`

- [ ] **Step 4: Verify**
Run:
- `cargo test -p agent-core`
- `cargo test -p agent-cli`
Expected: all pass.

---

### Task 6: Final quality gate and commits

**Files:**
- Modify only files touched above.

- [ ] **Step 1: Lint/build checks**
Run:
- `cargo build -p agent-core -p agent-cli`
- `cargo clippy -p agent-core -p agent-cli -- -D warnings`

- [ ] **Step 2: Commit split**
Recommended commit split:
1. `feat(agent-core): add callchain model and runtime retention`
2. `feat(agent-core): instrument turn/provider execution spans`
3. `feat(agent-cli): add /trace show export clear commands`
4. `test(core+cli): add callchain suspension/resume export coverage`

- [ ] **Step 3: Final report**
Summarize behavior, commands, and known limitations.

---

## Implementation Notes

- Keep instrumentation additive and best-effort; no execution-path panics.
- Avoid changing existing event payload contracts in this phase.
- Use compact attrs/events to control memory footprint.
- Prefer deterministic timestamps and stable IDs in tests where possible.
