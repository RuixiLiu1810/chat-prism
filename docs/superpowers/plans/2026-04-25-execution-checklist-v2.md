# Execution Checklist v2 (Single Entry)

> Source of truth: `2026-04-25-master-plan-v2.md`

## Global Rules

- Phase order is strict: `P-1 -> P0 -> P1 -> P2 -> P3 -> P4`
- Do not move to next phase unless current phase gate is green.
- Compatibility-first for P3: keep adapter bridge for existing executors.
- Memory strategy is remember_fact-first; `memory_write` only optional alias.

## Preflight

- [x] `git status --short` reviewed
- [x] `cargo --version` and toolchain healthy
- [ ] baseline build:
  - `cargo build --workspace`
  - note: currently blocked by `apps/desktop/src-tauri` transitive `tectonic` dependency mismatch in this environment; scope-local builds (`agent-core` + `agent-cli`) are green.

## P-1 Plan Hygiene Gate

- [x] Child plans (`P0..P4`) all contain `v2 强制修订` + `Non-goals` + `DoD（v2）` + `执行约束`
- [x] No stale critical API assumptions remain in active sections

Suggested checks:
- `rg -n "v2 强制修订|Non-goals|DoD（v2）|执行约束" docs/superpowers/plans/2026-04-25-p{0,1,2,3,4}-*.md`
- `rg -n "call\.input\b|ToolResultContentBlock" docs/superpowers/plans/2026-04-25-p{0,1,2,3,4}-*.md`

## P0 Gate (Lifecycle/Retry/Orphan)

- [x] Ctrl-C or cancel signal reaches provider streaming loop
- [x] Retry-After logic works for 429/503
- [x] orphan tool_use/tool_result pre-send repair works

Suggested checks:
- `cargo test -p agent-core`
- `cargo test -p agent-cli`

## P1 Gate (Context Engineering)

- [x] pre-send compaction loop enabled
- [x] circuit breaker stops infinite retry/compact loops
- [x] observable status/logs for compaction and breaker

Suggested checks:
- `cargo test -p agent-core`
- targeted long-context scenario run

## P2 Gate (Memory Unification)

- [x] remember_fact path hardened (key/value/path validation)
- [x] cross-session memory injection visible
- [x] no behavior split between remember_fact and optional alias

Suggested checks:
- `cargo test -p agent-core`
- `cargo test -p agent-cli`

## P3 Gate (Tool Registry)

- [x] ToolRegistry + ToolHandler landed
- [x] compatibility bridge preserved (legacy executor path still buildable)
- [ ] desktop adapter compiles without feature regression
  - note: desktop full workspace build currently blocked by `tectonic` transitive dependency mismatch in this environment.

Suggested checks:
- `cargo build --workspace`
- `cargo test --workspace`

## P4 Gate (Multi-Agent)

- [x] `run_subagent_turn` works for single-level delegation
- [x] child toolset excludes `spawn_subagent` (hard recursion guard)
- [x] parent cancel propagates to child execution

Suggested checks:
- `cargo test -p agent-core`
- `cargo test -p agent-cli`

## Final Acceptance

- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace -- -D warnings`
- [ ] smoke scenario for CLI turn lifecycle and tool calls
  - note: `cargo build --workspace` is blocked by desktop `tectonic` dependency errors; scope-local acceptance for `agent-core` + `agent-cli` is green (`build/test/clippy`).

## Rollback Policy

- Keep each phase in isolated commits.
- If phase gate fails, rollback only that phase commits.
- Never mix cross-phase refactor in one commit.
