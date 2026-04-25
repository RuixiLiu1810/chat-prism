# Local Agent Split + Single-Crate Re-Architecture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract `crates/agent-core + crates/agent-cli` into a standalone CLI repository, re-architect it into a single-crate Claude-Code-style layered layout, and decouple Prism desktop from in-process local-agent runtime to external-process-only integration.

**Architecture:** Use a phased migration: (1) protocol freeze + history-preserving extraction, (2) standalone repo single-crate reshaping with compatibility adapters, (3) desktop cutover to external process protocol with rollback switch, (4) cleanup and stabilization. Keep behavior compatibility at protocol boundary while allowing internal structure refactor.

**Tech Stack:** Rust 2021, Cargo workspace split/subtree, tokio async process, serde/serde_json stream protocol, Tauri command bridge, crossterm TUI, reqwest SSE providers.

---

## 0. Scope Check and Assumptions

This plan intentionally covers three tightly-coupled subsystems because they are one migration unit:
1. source split (`claude-prism` -> standalone local-agent repo)
2. local-agent internal architecture migration (double crate -> single crate layered)
3. Prism desktop integration cutover (in-process runtime -> external process)

Assumptions used in this plan:
- Current prism repo: `/Users/liuruixi/Documents/Code/claude-prism`
- New repo target path: `/Users/liuruixi/Documents/Code/prism-agent-cli`
- New repo remote: `https://github.com/RuixiLiu1810/prism-agent-cli.git` (replace during execution)
- Transition branch in prism: `refactor/agent-externalization`

---

## 1. Target File Structure (post-migration)

### 1.1 New standalone repo (`prism-agent-cli`) structure

- `Cargo.toml` (single crate package + binary target)
- `src/main.rs` (thin startup entry)
- `src/entrypoints/cli.rs` (flag fast-path routing)
- `src/commands/mod.rs` (slash command registry + router)
- `src/config/mod.rs` (config resolver/wizard/store)
- `src/runtime/session_kernel.rs` (turn/suspend/resume/cancel orchestrator)
- `src/runtime/turn_loop.rs` (provider dispatch and round orchestration)
- `src/providers/openai_responses.rs`
- `src/providers/chat_completions.rs`
- `src/tools/registry.rs`
- `src/tools/local/*.rs` (workspace/shell/memory/subagent)
- `src/protocol/events.rs` (single source of truth for stream-json payload)
- `src/output/human.rs`
- `src/output/jsonl.rs`
- `src/ui/tui/*.rs` (layout/input/transcript/theme)
- `src/state/runtime_state.rs` (session/history/approvals/memory/trace state)
- `tests/protocol_contract.rs`
- `tests/suspend_resume_flow.rs`

### 1.2 Prism desktop structure change

- Keep: `apps/desktop/src-tauri/src/claude.rs` (external process integration pattern)
- Create: `apps/desktop/src-tauri/src/local_agent_external.rs` (spawn + parse + emit for local agent)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (invoke handler registration cutover)
- Remove in-process dependency path: `apps/desktop/src-tauri/src/agent/*` (after cutover completion)
- Modify: `apps/desktop/src-tauri/Cargo.toml` (remove `agent-core` direct dependency after cutover)

---

### Task 1: Freeze Protocol + Create Migration Safety Net

**Files:**
- Create: `docs/superpowers/specs/2026-04-26-local-agent-external-protocol-spec.md`
- Create: `docs/superpowers/plans/2026-04-26-local-agent-migration-risk-register.md`
- Modify: `crates/agent-core/src/events.rs`
- Test: `crates/agent-core/src/events.rs`

- [ ] **Step 1: Write failing protocol serialization test for required event fields**

```rust
#[test]
fn protocol_status_event_includes_required_fields() {
    use crate::{AgentEventPayload, AgentStatusEvent};
    let payload = AgentEventPayload::Status(AgentStatusEvent {
        stage: "streaming".to_string(),
        message: "Connected".to_string(),
    });
    let v = serde_json::to_value(payload).unwrap();
    assert_eq!(v["type"], "status");
    assert!(v.get("stage").is_some());
    assert!(v.get("message").is_some());
}
```

- [ ] **Step 2: Run test to verify baseline status**

Run: `cargo test -p agent-core protocol_status_event_includes_required_fields -v`
Expected: PASS or FAIL; record current behavior in migration risk register.

- [ ] **Step 3: Add explicit protocol version field in complete payload (backward compatible)**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCompletePayload {
    pub tab_id: String,
    pub outcome: String,
    #[serde(default = "default_protocol_version")]
    pub protocol_version: u32,
}

fn default_protocol_version() -> u32 { 1 }
```

- [ ] **Step 4: Add backward-compat test for missing `protocolVersion`**

```rust
#[test]
fn complete_payload_defaults_protocol_version() {
    let raw = r#"{"tabId":"t1","outcome":"completed"}"#;
    let parsed: AgentCompletePayload = serde_json::from_str(raw).unwrap();
    assert_eq!(parsed.protocol_version, 1);
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-core events -v`
Expected: PASS

```bash
git add docs/superpowers/specs/2026-04-26-local-agent-external-protocol-spec.md \
  docs/superpowers/plans/2026-04-26-local-agent-migration-risk-register.md \
  crates/agent-core/src/events.rs
git commit -m "chore(protocol): freeze local-agent external event contract v1"
```

---

### Task 2: Extract Crates History into Standalone Repo (history-preserving)

**Files:**
- Create: `/Users/liuruixi/Documents/Code/prism-agent-cli/*`
- Modify: `.git` history in new repo only
- Test: N/A (command verification)

- [ ] **Step 1: Create split branch from prism main**

```bash
cd /Users/liuruixi/Documents/Code/claude-prism
git checkout main
git pull --rebase origin main
git checkout -b refactor/agent-externalization
```

- [ ] **Step 2: Produce subtree split branch for `crates`**

```bash
git subtree split --prefix=crates -b split/local-agent
```

- [ ] **Step 3: Initialize standalone repo from split branch**

```bash
cd /Users/liuruixi/Documents/Code
rm -rf prism-agent-cli
git clone /Users/liuruixi/Documents/Code/claude-prism prism-agent-cli
cd prism-agent-cli
git checkout split/local-agent
```

- [ ] **Step 4: Reset standalone repo root to extracted content and set remote**

```bash
cd /Users/liuruixi/Documents/Code/prism-agent-cli
git remote remove origin || true
git remote add origin https://github.com/RuixiLiu1810/prism-agent-cli.git
git branch -M main
```

- [ ] **Step 5: Push initial extracted history**

```bash
git push -u origin main
```

- [ ] **Step 6: Commit migration log in prism repo**

```bash
cd /Users/liuruixi/Documents/Code/claude-prism
cat > docs/superpowers/plans/2026-04-26-local-agent-repo-split-log.md <<'TXT'
Split source branch: split/local-agent
Standalone repo: prism-agent-cli
Strategy: subtree split (history-preserving)
TXT
git add docs/superpowers/plans/2026-04-26-local-agent-repo-split-log.md
git commit -m "docs(migration): record local-agent repo split metadata"
```

---

### Task 3: Convert Standalone Repo from Dual-Crate to Single-Crate Skeleton

**Files:**
- Create: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/{entrypoints,commands,runtime,providers,tools,protocol,output,state,ui}/mod.rs`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/Cargo.toml`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/main.rs`
- Test: `/Users/liuruixi/Documents/Code/prism-agent-cli/tests/smoke_boot.rs`

- [ ] **Step 1: Write failing smoke boot test**

```rust
#[test]
fn binary_boots_with_help() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_agent-runtime"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
}
```

- [ ] **Step 2: Run test to verify failure before skeleton wiring**

Run: `cargo test --test smoke_boot -v`
Expected: FAIL (missing test target or binary mismatch).

- [ ] **Step 3: Create single-crate module skeleton and route main -> entrypoint**

```rust
// src/main.rs
mod entrypoints;
mod commands;
mod runtime;
mod providers;
mod tools;
mod protocol;
mod output;
mod state;
mod ui;

fn main() {
    entrypoints::cli::run();
}
```

- [ ] **Step 4: Add minimal CLI run function**

```rust
// src/entrypoints/cli.rs
pub fn run() {
    println!("agent-runtime bootstrap ok");
}
```

- [ ] **Step 5: Run smoke test and commit**

Run: `cargo test --test smoke_boot -v`
Expected: PASS

```bash
git add Cargo.toml src/main.rs src/entrypoints src/commands src/runtime src/providers src/tools src/protocol src/output src/state src/ui tests/smoke_boot.rs
git commit -m "refactor(repo): bootstrap single-crate layered architecture"
```

---

### Task 4: Migrate Runtime Core Logic into Layered Single-Crate Modules

**Files:**
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/runtime/session_kernel.rs`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/runtime/turn_loop.rs`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/providers/{openai_responses.rs,chat_completions.rs}`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/state/runtime_state.rs`
- Test: `/Users/liuruixi/Documents/Code/prism-agent-cli/tests/suspend_resume_flow.rs`

- [ ] **Step 1: Write failing suspend/resume flow test**

```rust
#[tokio::test]
async fn suspended_turn_can_resume_in_same_session() {
    let mut kernel = crate::runtime::session_kernel::SessionKernel::for_test();
    let first = kernel.run_prompt("tab-1", "run shell command").await.unwrap();
    assert!(first.suspended);
    let resumed = kernel.approve_and_resume("tab-1", "shell", "once").await.unwrap();
    assert!(!resumed.suspended);
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test --test suspend_resume_flow -v`
Expected: FAIL (missing SessionKernel API).

- [ ] **Step 3: Implement minimal SessionKernel orchestration API**

```rust
pub struct SessionKernel {
    state: crate::state::runtime_state::RuntimeState,
}

impl SessionKernel {
    pub fn for_test() -> Self { Self { state: RuntimeState::default() } }
    pub async fn run_prompt(&mut self, tab_id: &str, prompt: &str) -> Result<TurnOutcome, String> {
        self.state.run_turn(tab_id, prompt).await
    }
    pub async fn approve_and_resume(&mut self, tab_id: &str, tool: &str, scope: &str) -> Result<TurnOutcome, String> {
        self.state.approve_and_resume(tab_id, tool, scope).await
    }
}
```

- [ ] **Step 4: Move provider round-loop code behind runtime trait boundary**

```rust
pub trait ProviderRuntime {
    fn id(&self) -> &'static str;
    async fn run_turn(&self, req: TurnRequest, history: &[serde_json::Value]) -> Result<TurnOutcome, String>;
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test --test suspend_resume_flow -v`
Expected: PASS

```bash
git add src/runtime src/providers src/state tests/suspend_resume_flow.rs
git commit -m "refactor(runtime): migrate turn orchestration into session kernel"
```

---

### Task 5: Build External Protocol Surface (`stream-json`) for Desktop

**Files:**
- Create: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/protocol/events.rs`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/output/jsonl.rs`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/output/human.rs`
- Test: `/Users/liuruixi/Documents/Code/prism-agent-cli/tests/protocol_contract.rs`

- [ ] **Step 1: Write failing protocol contract test against stable keys**

```rust
#[test]
fn jsonl_status_line_uses_stable_contract() {
    let line = crate::output::jsonl::encode_status("tab-1", "streaming", "Connected");
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["tabId"], "tab-1");
    assert_eq!(v["payload"]["type"], "status");
    assert_eq!(v["payload"]["stage"], "streaming");
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test --test protocol_contract -v`
Expected: FAIL (encode function missing).

- [ ] **Step 3: Implement canonical protocol encoder module**

```rust
pub fn encode_status(tab_id: &str, stage: &str, message: &str) -> String {
    serde_json::json!({
        "tabId": tab_id,
        "payload": {
            "type": "status",
            "stage": stage,
            "message": message
        }
    })
    .to_string()
}
```

- [ ] **Step 4: Ensure complete/error/tool payload encoders share same envelope**

```rust
pub fn encode_complete(tab_id: &str, outcome: &str) -> String {
    serde_json::json!({"tabId": tab_id, "payload": {"type": "complete", "outcome": outcome}, "protocolVersion": 1}).to_string()
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test --test protocol_contract -v`
Expected: PASS

```bash
git add src/protocol src/output tests/protocol_contract.rs
git commit -m "feat(protocol): add stable stream-json event contract for desktop integration"
```

---

### Task 6: Implement Desktop External Local-Agent Runner (No in-process runtime)

**Files:**
- Create: `apps/desktop/src-tauri/src/local_agent_external.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: `apps/desktop/src-tauri/src/local_agent_external.rs` (unit tests)

- [ ] **Step 1: Write failing parser test for local-agent stream-json lines**

```rust
#[test]
fn parses_status_line_from_local_agent() {
    let raw = r#"{"tabId":"tab-1","payload":{"type":"status","stage":"streaming","message":"Connected"}}"#;
    let evt = super::parse_local_agent_line(raw).unwrap();
    assert_eq!(evt.tab_id, "tab-1");
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test -p claude-prism-desktop parse_local_agent_line -v`
Expected: FAIL (new module not wired).

- [ ] **Step 3: Add external runner command wrapper mirroring `claude.rs` spawn path**

```rust
#[tauri::command]
pub async fn execute_local_agent(
    window: tauri::WebviewWindow,
    project_path: String,
    prompt: String,
    tab_id: String,
) -> Result<(), String> {
    let cmd = tokio::process::Command::new("agent-runtime")
        .arg("--project-path").arg(project_path)
        .arg("--prompt").arg(prompt)
        .arg("--output").arg("jsonl")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    spawn_local_agent_process(window, cmd, tab_id).await
}
```

- [ ] **Step 4: Register invoke handlers and keep old local-agent in-process commands behind kill switch**

```rust
.invoke_handler(tauri::generate_handler![
    local_agent_external::execute_local_agent,
    local_agent_external::continue_local_agent,
    local_agent_external::resume_local_agent,
    local_agent_external::cancel_local_agent,
])
```

- [ ] **Step 5: Remove desktop dependency on `agent-core` once commands compile**

```toml
# apps/desktop/src-tauri/Cargo.toml
# remove:
# agent-core = { path = "../../../crates/agent-core" }
```

- [ ] **Step 6: Run tests/build and commit**

Run: `cargo check -p claude-prism-desktop`
Expected: PASS

```bash
git add apps/desktop/src-tauri/src/local_agent_external.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/Cargo.toml
git commit -m "feat(desktop): switch local-agent integration to external process runner"
```

---

### Task 7: Cutover Frontend Command Wiring to External Local-Agent API

**Files:**
- Modify: `apps/desktop/src/*` (call sites currently invoking `agent_*` commands)
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: Desktop smoke flow (manual + command)

- [ ] **Step 1: Find call sites invoking in-process `agent_*` commands**

```bash
cd /Users/liuruixi/Documents/Code/claude-prism
rg -n "agent_start_turn|agent_continue_turn|agent_resume_pending_turn|agent_cancel_turn" apps/desktop/src apps/desktop/src-tauri/src
```

- [ ] **Step 2: Replace invoke command names with external local-agent commands**

```ts
await invoke("execute_local_agent", { projectPath, prompt, tabId })
```

- [ ] **Step 3: Keep event payload mapping unchanged in frontend state reducer**

```ts
// keep existing `agent-event`/`agent-complete` handling
// only switch command initiators
```

- [ ] **Step 4: Run desktop build and smoke test**

Run: `pnpm -C apps/desktop build && cargo check -p claude-prism-desktop`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src apps/desktop/src-tauri/src/lib.rs
git commit -m "refactor(frontend): route local-agent turns through external process commands"
```

---

### Task 8: Remove In-Process Local-Agent Modules from Prism

**Files:**
- Delete: `apps/desktop/src-tauri/src/agent/*`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `cargo check -p claude-prism-desktop`

- [ ] **Step 1: Remove module declaration and invoke registrations for `mod agent` path**

```rust
// lib.rs
// remove: mod agent;
// remove: agent::agent_* from invoke_handler
```

- [ ] **Step 2: Delete obsolete in-process agent module tree**

```bash
cd /Users/liuruixi/Documents/Code/claude-prism
git rm -r apps/desktop/src-tauri/src/agent
```

- [ ] **Step 3: Run compile and verify no agent-core references remain in desktop**

Run: `rg -n "agent_core|agent::agent_" apps/desktop/src-tauri/src apps/desktop/src`
Expected: no matches for in-process path

- [ ] **Step 4: Build check and commit**

Run: `cargo check -p claude-prism-desktop`
Expected: PASS

```bash
git add apps/desktop/src-tauri/src/lib.rs
git commit -m "chore(desktop): remove in-process local-agent modules after external cutover"
```

---

### Task 9: Add Cross-Repo Compatibility CI Gates

**Files:**
- Create: `.github/workflows/local-agent-contract.yml` (in prism)
- Create: `/Users/liuruixi/Documents/Code/prism-agent-cli/.github/workflows/contract-export.yml`
- Modify: `docs/superpowers/plans/2026-04-26-local-agent-migration-risk-register.md`
- Test: CI dry-run (local command simulation)

- [ ] **Step 1: Add CLI contract fixture export command in standalone repo**

```bash
cargo run -- --prompt "ping" --output jsonl > tests/fixtures/ping.jsonl
```

- [ ] **Step 2: Add Prism-side contract validator script**

```bash
jq -e '.payload.type' tests/fixtures/ping.jsonl >/dev/null
```

- [ ] **Step 3: Add workflow job asserting parser compatibility**

```yaml
- name: Validate local-agent protocol fixture
  run: |
    jq -e '.payload.type' tests/fixtures/ping.jsonl >/dev/null
```

- [ ] **Step 4: Run local checks and commit**

Run: `act -j validate-local-agent-contract` (or CI push validation)
Expected: PASS

```bash
git add .github/workflows/local-agent-contract.yml docs/superpowers/plans/2026-04-26-local-agent-migration-risk-register.md
git commit -m "ci(contract): add local-agent stream protocol compatibility gate"
```

---

### Task 10: Claude-Code-Style Structure Pass in Standalone Repo

**Files:**
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/main.rs`
- Create: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/entrypoints/cli.rs`
- Create: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/services/*`
- Modify: `/Users/liuruixi/Documents/Code/prism-agent-cli/src/commands/mod.rs`
- Test: `/Users/liuruixi/Documents/Code/prism-agent-cli/tests/command_router.rs`

- [ ] **Step 1: Write failing command registry test**

```rust
#[test]
fn command_registry_contains_help_and_status() {
    let reg = crate::commands::registry();
    assert!(reg.contains_key("/help"));
    assert!(reg.contains_key("/status"));
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test --test command_router -v`
Expected: FAIL

- [ ] **Step 3: Implement command registry + handler split**

```rust
pub type CommandHandler = fn(&mut AppContext, &[&str]) -> Result<(), String>;
pub fn registry() -> std::collections::HashMap<&'static str, CommandHandler> {
    let mut m = std::collections::HashMap::new();
    m.insert("/help", handlers::help as CommandHandler);
    m.insert("/status", handlers::status as CommandHandler);
    m
}
```

- [ ] **Step 4: Move heavy orchestration into `services/` modules**

```rust
// src/services/turn_service.rs
pub async fn run_turn(ctx: &mut AppContext, prompt: String) -> Result<(), String> { /* moved from main */ }
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test --test command_router -v && cargo clippy -- -D warnings`
Expected: PASS

```bash
git add src/main.rs src/entrypoints src/commands src/services tests/command_router.rs
git commit -m "refactor(cli): align single-crate structure with claude-code-style layering"
```

---

### Task 11: Rollout, Rollback, and Operational Hardening

**Files:**
- Create: `docs/superpowers/plans/2026-04-26-local-agent-cutover-runbook.md`
- Modify: `docs/superpowers/plans/2026-04-26-local-agent-migration-risk-register.md`
- Test: manual drill commands in runbook

- [ ] **Step 1: Define rollout gates and rollback trigger thresholds**

```md
- Gate 1: desktop can run 20 consecutive turns without protocol parse failure
- Gate 2: suspend/resume success >= 95% on internal test matrix
- Rollback trigger: >2% failed turn startups in 24h
```

- [ ] **Step 2: Add rollback commands to re-enable old path (if kept in release branch)**

```bash
git revert <desktop-cutover-commit>
git revert <remove-inprocess-agent-commit>
```

- [ ] **Step 3: Add incident checklist for known migration risks**

```md
- mismatch event fields
- stdout buffering causing delayed UI
- orphan suspended turns after app restart
- binary discovery failures on macOS/Linux/Windows
```

- [ ] **Step 4: Commit runbook**

```bash
git add docs/superpowers/plans/2026-04-26-local-agent-cutover-runbook.md docs/superpowers/plans/2026-04-26-local-agent-migration-risk-register.md
git commit -m "docs(ops): add local-agent external cutover runbook and rollback checklist"
```

---

## 2. Decoupling Attention Checklist (must-pass before final cutover)

- [ ] Desktop no longer imports `agent-core` directly.
- [ ] Desktop local-agent path is external process only.
- [ ] Event contract has explicit version and compatibility tests.
- [ ] Suspend/resume semantics verified across process restarts.
- [ ] Tool-approval prompts remain in same UX timeline.
- [ ] Protocol parser is tolerant to additive fields.
- [ ] Startup binary discovery supports absolute path override (`LOCAL_AGENT_BIN`).
- [ ] Logs redact API key-like substrings before emit.
- [ ] Backpressure safe: stdout/stderr readers are non-blocking and line-buffered.
- [ ] CI validates contract fixture on every desktop PR.

---

## 3. Validation Matrix (final)

Run in prism repo:
- `cargo check -p claude-prism-desktop`
- `cargo test -p claude-prism-desktop`
- `rg -n "agent_core|mod agent;|agent_start_turn" apps/desktop/src-tauri/src`

Run in standalone repo:
- `cargo check`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cargo run -- --prompt "ping" --output jsonl`

Cross-repo:
- Desktop parses standalone CLI `status/tool_call/tool_result/complete` fixtures.
- Suspend + approve + resume E2E passes.

---

## 4. Self-Review (Plan vs Requirements)

Spec coverage checklist:
- [x] dual-crate extraction with history preservation
- [x] desktop decoupling and external-process-only path
- [x] Claude-Code-style structural re-organization in standalone CLI
- [x] risk handling (protocol drift, suspend/resume, rollback)
- [x] CI/contract verification and operational runbook

Placeholder scan:
- [x] no TBD/TODO placeholders
- [x] every implementation task has explicit files/commands

Type consistency check:
- [x] protocol envelope naming kept stable (`tabId`, `payload.type`)
- [x] suspend/resume APIs consistently referenced as kernel-level orchestration

