# Agent CLI Claude-Code Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a minimax-first, Claude-Code-style interactive CLI for `agent-cli` with real tool execution, human-readable streaming output, slash commands, approval gating, and session persistence across process restarts.

**Architecture:** Keep `agent-core` as provider/runtime orchestration and policy source, and evolve `agent-cli` into a thin interactive shell with explicit modules for input parsing, turn orchestration, rendering, and persistence. Introduce a dedicated local-tools crate for CLI-safe coding tools (`read/list/search/edit/write/shell/memory`) and wire it through `ToolExecutorFn`. Use explicit tool-profile filtering in `agent-core` so minimax never sees unsupported academic tools in this CLI mode.

**Tech Stack:** Rust 2021, tokio, clap, serde/serde_json, reqwest SSE, tempfile, async process execution (`tokio::process`), `agent-core` runtime contracts.

---

## Implementation Options Evaluation

### Option A: Keep current single-turn CLI and only add a stdin loop in `main.rs`
- Pros: Fastest patch.
- Cons: Monolithic file, no slash-command grammar, no reliable resume/approval flow, poor testability, high regression risk.
- Verdict: Rejected.

### Option B: Copy desktop tool executor code directly into `agent-cli`
- Pros: Short-term speed.
- Cons: Creates permanent forked logic; future policy/tool fixes must be duplicated; high maintenance cost.
- Verdict: Rejected.

### Option C (Chosen): Modular CLI shell + shared local-tools crate + core tool-profile filtering
- Pros: Explicit boundaries, maintainable tests, minimax-compatible coding toolset, future extension to openai/deepseek without redesign.
- Cons: Higher upfront refactor scope.
- Verdict: Selected.

### Session persistence options
- Option 1: Persist sessions/histories in `agent-core` for all runtimes.
- Option 2 (Chosen for this scope): Add CLI sidecar persistence for session records/history while keeping existing `agent-core` persistence for approvals/pending turns/workflow/memory.
- Reason for choice: Delivers CLI restart continuity with minimal desktop regression risk.

---

## File Structure

### Create
- `crates/agent-cli/src/args.rs` - CLI argument model (`single-turn` and `repl` modes, output mode, startup flags).
- `crates/agent-cli/src/commands.rs` - Slash command parser and typed command enum.
- `crates/agent-cli/src/output.rs` - `EventSink` implementations: human renderer and JSONL renderer.
- `crates/agent-cli/src/turn_runner.rs` - Provider dispatch, continuation metadata resolution, and outcome persistence glue.
- `crates/agent-cli/src/session_store.rs` - CLI sidecar persistence for `sessions` and `histories` snapshots.
- `crates/agent-cli/src/repl.rs` - Interactive loop orchestration and command routing.
- `crates/agent-local-tools/Cargo.toml` - Shared CLI local-tools crate manifest.
- `crates/agent-local-tools/src/lib.rs` - Public executor entrypoint and re-exports.
- `crates/agent-local-tools/src/process_utils.rs` - Command execution helper reused by shell tool.
- `crates/agent-local-tools/src/executor.rs` - Tool dispatch for coding toolset.
- `crates/agent-local-tools/src/tools/workspace.rs` - `read_file/list_files/search_project`.
- `crates/agent-local-tools/src/tools/edit.rs` - `apply_text_patch/write_file/replace_selected_text`.
- `crates/agent-local-tools/src/tools/shell.rs` - `run_shell_command` with safety policy.
- `crates/agent-local-tools/src/tools/memory.rs` - `remember_fact`.

### Modify
- `Cargo.toml` - Add `crates/agent-local-tools` workspace member.
- `crates/agent-cli/Cargo.toml` - Add dependencies (`agent-local-tools`, `dirs`, `tempfile` dev-dep).
- `crates/agent-cli/src/main.rs` - Replace monolithic flow with module-driven entry and mode dispatch.
- `crates/agent-cli/src/tool_executor.rs` - Replace fallback error executor with local-tools executor adapter.
- `crates/agent-core/src/tools.rs` - Add tool profile enum and profile-aware tool-spec selection.
- `crates/agent-core/src/config.rs` - Add `tool_profile` runtime config field with backward-compatible default.
- `crates/agent-core/src/providers/chat_completions.rs` - Use profile-aware tool schemas and project storage dir.
- `crates/agent-core/src/providers/openai.rs` - Same profile-aware tool schema behavior for parity.
- `apps/desktop/src-tauri/src/settings/mod.rs` - Populate `tool_profile` default (`full_academic`) when loading runtime config.
- `docs/superpowers/crates-agent-handoff.md` - Update CLI boundary section after implementation.

### Test
- `crates/agent-cli/src/commands.rs` (unit tests)
- `crates/agent-cli/src/output.rs` (unit tests)
- `crates/agent-cli/src/session_store.rs` (unit tests with tempdir)
- `crates/agent-cli/src/turn_runner.rs` (unit tests for request/session mutation helpers)
- `crates/agent-local-tools/src/executor.rs` (unit tests)
- `crates/agent-local-tools/src/tools/shell.rs` (unit tests)
- `crates/agent-core/src/tools.rs` (tool profile tests)

---

### Task 1: CLI Arguments and Slash Command Grammar

**Files:**
- Create: `crates/agent-cli/src/args.rs`
- Create: `crates/agent-cli/src/commands.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/commands.rs`

- [ ] **Step 1: Write failing command-parser tests**

```rust
#[cfg(test)]
mod tests {
    use super::{ApprovalScope, CliCommand};

    #[test]
    fn parses_approve_once_with_tool() {
        let cmd = CliCommand::parse("/approve once run_shell_command").unwrap();
        assert_eq!(cmd, CliCommand::Approve {
            scope: ApprovalScope::Once,
            tool: "run_shell_command".to_string(),
        });
    }

    #[test]
    fn parses_session_select() {
        let cmd = CliCommand::parse("/session select 123").unwrap();
        assert_eq!(cmd, CliCommand::SessionSelect { local_session_id: "123".to_string() });
    }

    #[test]
    fn rejects_unknown_slash_command() {
        let err = CliCommand::parse("/oops").unwrap_err();
        assert!(err.contains("Unknown command"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-cli commands::tests::parses_approve_once_with_tool -v`
Expected: FAIL with unresolved `CliCommand::parse` or missing module.

- [ ] **Step 3: Implement command enum + parser**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalScope {
    Once,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Help,
    Exit,
    Status,
    ModelShow,
    ModelSet { model: String },
    SessionList,
    SessionSelect { local_session_id: String },
    SessionNew,
    Approve { scope: ApprovalScope, tool: String },
    Deny { tool: String },
    Resume,
    Clear,
}

impl CliCommand {
    pub fn parse(input: &str) -> Result<Self, String> {
        let parts = input.trim().split_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            ["/help"] => Ok(Self::Help),
            ["/exit"] | ["/quit"] => Ok(Self::Exit),
            ["/status"] => Ok(Self::Status),
            ["/model"] => Ok(Self::ModelShow),
            ["/model", "set", model] => Ok(Self::ModelSet { model: (*model).to_string() }),
            ["/session", "list"] => Ok(Self::SessionList),
            ["/session", "new"] => Ok(Self::SessionNew),
            ["/session", "select", id] => Ok(Self::SessionSelect { local_session_id: (*id).to_string() }),
            ["/approve", "once", tool] => Ok(Self::Approve { scope: ApprovalScope::Once, tool: (*tool).to_string() }),
            ["/approve", "session", tool] => Ok(Self::Approve { scope: ApprovalScope::Session, tool: (*tool).to_string() }),
            ["/deny", tool] => Ok(Self::Deny { tool: (*tool).to_string() }),
            ["/resume"] => Ok(Self::Resume),
            ["/clear"] => Ok(Self::Clear),
            [unknown, ..] if unknown.starts_with('/') => Err(format!("Unknown command: {}", unknown)),
            _ => Err("Not a slash command".to_string()),
        }
    }
}
```

- [ ] **Step 4: Add new args module and wire main entry**

```rust
#[derive(clap::Parser, Debug)]
#[command(name = "agent-runtime", version)]
pub struct Args {
    #[arg(long, env = "AGENT_API_KEY")]
    pub api_key: String,

    #[arg(long, env = "AGENT_PROVIDER", default_value = "minimax")]
    pub provider: String,

    #[arg(long, env = "AGENT_MODEL", default_value = "MiniMax-M1")]
    pub model: String,

    #[arg(long, default_value = ".")]
    pub project_path: String,

    #[arg(long)]
    pub prompt: Option<String>,

    #[arg(long, default_value = "human")]
    pub output: String,
}
```

- [ ] **Step 5: Run tests to verify parser passes**

Run: `cargo test -p agent-cli commands::tests -v`
Expected: PASS for parser tests.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/args.rs crates/agent-cli/src/commands.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add slash command grammar and modular args"
```

---

### Task 2: Human Output Renderer and JSONL Compatibility

**Files:**
- Create: `crates/agent-cli/src/output.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/output.rs`

- [ ] **Step 1: Write failing renderer tests**

```rust
#[test]
fn human_sink_renders_message_delta_without_json() {
    let sink = HumanEventSink::new_for_test();
    sink.emit_event(&AgentEventEnvelope {
        tab_id: "t1".to_string(),
        payload: AgentEventPayload::MessageDelta(AgentMessageDeltaEvent { delta: "hello".to_string() }),
    });
    let out = sink.take_buffer();
    assert!(out.contains("hello"));
    assert!(!out.contains("\"payload\""));
}

#[test]
fn jsonl_sink_keeps_machine_readable_line() {
    let sink = JsonlEventSink::new_for_test();
    sink.emit_complete(&AgentCompletePayload { tab_id: "t1".to_string(), outcome: "completed".to_string() });
    let out = sink.take_buffer();
    assert!(out.contains("\"outcome\":\"completed\""));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-cli output::tests::human_sink_renders_message_delta_without_json -v`
Expected: FAIL because sinks are not implemented.

- [ ] **Step 3: Implement sink types and routing**

```rust
pub enum OutputMode {
    Human,
    Jsonl,
}

pub fn parse_output_mode(raw: &str) -> Result<OutputMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "human" => Ok(OutputMode::Human),
        "jsonl" => Ok(OutputMode::Jsonl),
        other => Err(format!("Unsupported output mode: {}", other)),
    }
}

impl EventSink for HumanEventSink {
    fn emit_event(&self, envelope: &AgentEventEnvelope) {
        match &envelope.payload {
            AgentEventPayload::MessageDelta(delta) => print!("{}", delta.delta),
            AgentEventPayload::Status(status) => println!("\n[{}] {}", status.stage, status.message),
            AgentEventPayload::ToolCall(call) => println!("\n[tool] {} ({})", call.tool_name, call.call_id),
            AgentEventPayload::ToolResult(result) => println!("\n[result] {}", result.preview),
            AgentEventPayload::Error(err) => eprintln!("\n[error:{}] {}", err.code, err.message),
            _ => {}
        }
    }

    fn emit_complete(&self, payload: &AgentCompletePayload) {
        println!("\n[turn:{}]", payload.outcome);
    }
}
```

- [ ] **Step 4: Wire sink selection in startup path**

```rust
let output_mode = parse_output_mode(&args.output)?;
let sink: Arc<dyn EventSink> = match output_mode {
    OutputMode::Human => Arc::new(HumanEventSink::new()),
    OutputMode::Jsonl => Arc::new(JsonlEventSink::new()),
};
```

- [ ] **Step 5: Run tests to verify rendering behavior**

Run: `cargo test -p agent-cli output::tests -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/output.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add human renderer and jsonl output mode"
```

---

### Task 3: Turn Runner and Minimax-First Continuation Flow

**Files:**
- Create: `crates/agent-cli/src/turn_runner.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/turn_runner.rs`

- [ ] **Step 1: Write failing request/session helper tests**

```rust
#[test]
fn continuation_uses_last_response_id_for_existing_session() {
    let session = AgentSessionRecord {
        local_session_id: "s1".into(),
        provider: "minimax".into(),
        project_path: ".".into(),
        tab_id: "cli".into(),
        title: "x".into(),
        model: "MiniMax-M1".into(),
        previous_response_id: Some("r0".into()),
        last_response_id: Some("r1".into()),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(resolve_previous_response_id(Some(&session), None), Some("r1".into()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-cli turn_runner::tests::continuation_uses_last_response_id_for_existing_session -v`
Expected: FAIL due missing helpers.

- [ ] **Step 3: Implement turn runner with provider dispatch**

```rust
pub async fn run_turn(
    sink: &dyn EventSink,
    config_provider: &dyn ConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    prior_history: &[serde_json::Value],
    tool_executor: ToolExecutorFn,
) -> Result<agent_core::AgentTurnOutcome, String> {
    match config_provider
        .load_agent_runtime(Some(&request.project_path))?
        .provider
        .as_str()
    {
        "minimax" => agent_core::providers::chat_completions::run_turn_loop(
            sink,
            config_provider,
            runtime_state,
            request,
            prior_history,
            tool_executor,
            None,
        )
        .await,
        "deepseek" | "openai" => Err("This CLI mode is minimax-first. Switch provider only with --provider override after compatibility checks.".to_string()),
        other => Err(format!("Unsupported provider: {}", other)),
    }
}

pub fn resolve_previous_response_id(
    existing_session: Option<&AgentSessionRecord>,
    request_previous_response_id: Option<String>,
) -> Option<String> {
    request_previous_response_id.or_else(|| existing_session.and_then(|s| s.last_response_id.clone()))
}
```

- [ ] **Step 4: Persist outcome to runtime_state session/history maps**

```rust
pub async fn persist_turn_outcome(
    runtime_state: &AgentRuntimeState,
    provider: &str,
    model: &str,
    request: &AgentTurnDescriptor,
    outcome: &agent_core::AgentTurnOutcome,
) -> String {
    let mut sessions = runtime_state.sessions.lock().await;
    let local_session_id = if let Some(id) = request.local_session_id.as_ref() {
        if let Some(session) = sessions.get_mut(id) {
            session.touch_response(outcome.response_id.clone());
            id.clone()
        } else {
            let mut session = AgentSessionRecord::new(provider, request.project_path.clone(), request.tab_id.clone(), summarize_session_title(&request.prompt), model.to_string());
            session.touch_response(outcome.response_id.clone());
            let id = session.local_session_id.clone();
            sessions.insert(id.clone(), session);
            id
        }
    } else {
        let mut session = AgentSessionRecord::new(provider, request.project_path.clone(), request.tab_id.clone(), summarize_session_title(&request.prompt), model.to_string());
        session.touch_response(outcome.response_id.clone());
        let id = session.local_session_id.clone();
        sessions.insert(id.clone(), session);
        id
    };
    drop(sessions);
    runtime_state.bind_tab_state_to_session(&request.tab_id, &local_session_id).await;
    runtime_state.append_history(&local_session_id, outcome.messages.clone()).await;
    local_session_id
}
```

- [ ] **Step 5: Run test suite for turn runner helpers**

Run: `cargo test -p agent-cli turn_runner::tests -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/turn_runner.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add minimax-first turn runner and continuation helpers"
```

---

### Task 4: Restart Persistence for Sessions and Histories

**Files:**
- Create: `crates/agent-cli/src/session_store.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/session_store.rs`

- [ ] **Step 1: Write failing persistence tests**

```rust
#[tokio::test]
async fn save_then_load_roundtrip_restores_sessions_and_histories() {
    let temp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(temp.path().join("sessions.json"));

    let mut sessions = std::collections::HashMap::new();
    sessions.insert("s1".to_string(), AgentSessionRecord::new("minimax", ".".to_string(), "cli".to_string(), "chat".to_string(), "MiniMax-M1".to_string()));

    let mut histories = std::collections::HashMap::new();
    histories.insert("s1".to_string(), vec![serde_json::json!({"type":"user","content":"hello"})]);

    store.save(&sessions, &histories).await.unwrap();
    let loaded = store.load().await.unwrap();

    assert!(loaded.sessions.contains_key("s1"));
    assert_eq!(loaded.histories["s1"].len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-cli session_store::tests::save_then_load_roundtrip_restores_sessions_and_histories -v`
Expected: FAIL due missing `SessionStore`.

- [ ] **Step 3: Implement `SessionStore` JSON snapshot format**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionSnapshot {
    pub sessions: std::collections::HashMap<String, AgentSessionRecord>,
    pub histories: std::collections::HashMap<String, Vec<serde_json::Value>>,
    pub active_session_id: Option<String>,
}

pub struct SessionStore {
    path: std::path::PathBuf,
}

impl SessionStore {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }

    pub async fn load(&self) -> Result<SessionSnapshot, String> {
        if !self.path.exists() {
            return Ok(SessionSnapshot::default());
        }
        let raw = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| format!("Failed to read {}: {}", self.path.display(), e))?;
        serde_json::from_str(&raw)
            .map_err(|e| format!("Failed to decode {}: {}", self.path.display(), e))
    }

    pub async fn save(
        &self,
        sessions: &std::collections::HashMap<String, AgentSessionRecord>,
        histories: &std::collections::HashMap<String, Vec<serde_json::Value>>,
    ) -> Result<(), String> {
        let snapshot = SessionSnapshot {
            sessions: sessions.clone(),
            histories: histories.clone(),
            active_session_id: None,
        };
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
        }
        let text = serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?;
        tokio::fs::write(&self.path, text)
            .await
            .map_err(|e| format!("Failed to write {}: {}", self.path.display(), e))
    }
}
```

- [ ] **Step 4: Wire load-on-start and save-after-turn**

```rust
let project_store_dir = config_provider.project_storage_dir(&args.project_path)?;
let store = SessionStore::new(project_store_dir.join("cli-sessions.json"));
let snapshot = store.load().await?;
{
    let mut sessions = runtime_state.sessions.lock().await;
    *sessions = snapshot.sessions;
}
{
    let mut histories = runtime_state.histories.lock().await;
    *histories = snapshot.histories;
}
```

- [ ] **Step 5: Run persistence tests and full CLI tests**

Run: `cargo test -p agent-cli session_store::tests -v`
Expected: PASS.

Run: `cargo test -p agent-cli -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/session_store.rs crates/agent-cli/src/main.rs crates/agent-cli/Cargo.toml
git commit -m "feat(agent-cli): persist sessions and histories across restarts"
```

---

### Task 5: Real Tool Backend via `agent-local-tools`

**Files:**
- Create: `crates/agent-local-tools/Cargo.toml`
- Create: `crates/agent-local-tools/src/lib.rs`
- Create: `crates/agent-local-tools/src/process_utils.rs`
- Create: `crates/agent-local-tools/src/executor.rs`
- Create: `crates/agent-local-tools/src/tools/workspace.rs`
- Create: `crates/agent-local-tools/src/tools/edit.rs`
- Create: `crates/agent-local-tools/src/tools/shell.rs`
- Create: `crates/agent-local-tools/src/tools/memory.rs`
- Modify: `Cargo.toml`
- Modify: `crates/agent-cli/Cargo.toml`
- Modify: `crates/agent-cli/src/tool_executor.rs`
- Test: `crates/agent-local-tools/src/executor.rs`

- [ ] **Step 1: Write failing executor tests**

```rust
#[tokio::test]
async fn read_file_tool_returns_content() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("a.txt");
    tokio::fs::write(&file, "hello").await.unwrap();

    let call = AgentToolCall {
        tool_name: "read_file".to_string(),
        call_id: "c1".to_string(),
        arguments: r#"{"path":"a.txt"}"#.to_string(),
    };

    let result = execute_tool_call_for_cli(None, "cli", temp.path().to_str().unwrap(), call, None).await;
    assert!(!result.is_error);
    assert_eq!(result.content["content"], "hello");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-local-tools read_file_tool_returns_content -v`
Expected: FAIL (crate/module missing).

- [ ] **Step 3: Create crate manifest and executor entrypoint**

```toml
[package]
name = "agent-local-tools"
version = "0.1.0"
edition = "2021"

[dependencies]
agent-core = { path = "../agent-core" }
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
dirs = "5"

[dev-dependencies]
tempfile = "3"
```

```rust
pub async fn execute_tool_call_for_cli(
    runtime_state: Option<&AgentRuntimeState>,
    tab_id: &str,
    project_root: &str,
    call: AgentToolCall,
    cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
) -> AgentToolResult {
    executor::execute_tool_call_for_cli(runtime_state, tab_id, project_root, call, cancel_rx).await
}
```

- [ ] **Step 4: Implement coding tool dispatch only**

```rust
match call.tool_name.as_str() {
    "read_file" => tools::workspace::execute_read_file(project_root, &call.call_id, args, cancel_rx).await,
    "list_files" => tools::workspace::execute_list_files(project_root, &call.call_id, args, cancel_rx).await,
    "search_project" => tools::workspace::execute_search_project(project_root, &call.call_id, args, cancel_rx).await,
    "apply_text_patch" => tools::edit::execute_apply_text_patch(runtime_state, tab_id, project_root, &call.call_id, args, cancel_rx).await,
    "write_file" => tools::edit::execute_write_file(runtime_state, tab_id, project_root, &call.call_id, args, cancel_rx).await,
    "replace_selected_text" => tools::edit::execute_replace_selected_text(runtime_state, tab_id, project_root, &call.call_id, args, cancel_rx).await,
    "run_shell_command" => tools::shell::execute_run_shell_command(runtime_state, tab_id, project_root, &call.call_id, args, cancel_rx).await,
    "remember_fact" => tools::memory::execute_remember_fact(runtime_state, &call.call_id, args, cancel_rx).await,
    other => agent_core::tools::error_result(other, &call.call_id, format!("Unsupported CLI tool: {}", other)),
}
```

- [ ] **Step 5: Replace fallback executor in `agent-cli`**

```rust
pub async fn execute_cli_tool(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    project_root: &str,
    call: AgentToolCall,
    cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
) -> AgentToolResult {
    agent_local_tools::execute_tool_call_for_cli(
        Some(runtime_state),
        tab_id,
        project_root,
        call,
        cancel_rx,
    ).await
}
```

- [ ] **Step 6: Run crate tests and CLI tests**

Run: `cargo test -p agent-local-tools -v`
Expected: PASS.

Run: `cargo test -p agent-cli -v`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/agent-local-tools crates/agent-cli/Cargo.toml crates/agent-cli/src/tool_executor.rs
git commit -m "feat(agent-cli): add real local tool executor for coding toolset"
```

---

### Task 6: Tool Profile Filtering in `agent-core` (Avoid Unsupported Tools)

**Files:**
- Modify: `crates/agent-core/src/config.rs`
- Modify: `crates/agent-core/src/tools.rs`
- Modify: `crates/agent-core/src/providers/chat_completions.rs`
- Modify: `crates/agent-core/src/providers/openai.rs`
- Modify: `apps/desktop/src-tauri/src/settings/mod.rs`
- Test: `crates/agent-core/src/tools.rs`

- [ ] **Step 1: Write failing tool-profile tests**

```rust
#[test]
fn coding_cli_profile_excludes_literature_tools() {
    let specs = tool_specs_for_profile("coding_cli");
    let names = specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>();
    assert!(!names.contains(&"search_literature"));
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"run_shell_command"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-core coding_cli_profile_excludes_literature_tools -v`
Expected: FAIL due missing profile API.

- [ ] **Step 3: Add `tool_profile` to runtime config with default**

```rust
#[derive(Debug, Clone)]
pub struct AgentRuntimeConfig {
    pub runtime: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub domain_config: AgentDomainConfig,
    pub sampling_profiles: AgentSamplingProfilesConfig,
    pub tool_profile: String,
}

impl AgentRuntimeConfig {
    pub fn default_local_agent() -> Self {
        Self {
            // existing fields...
            tool_profile: "full_academic".to_string(),
        }
    }
}
```

- [ ] **Step 4: Implement profile-aware tool catalog API**

```rust
pub fn tool_specs_for_profile(profile: &str) -> Vec<AgentToolSpec> {
    match profile {
        "coding_cli" => build_coding_cli_tool_specs(),
        _ => build_default_tool_specs(writing_tools_enabled()),
    }
}

fn build_coding_cli_tool_specs() -> Vec<AgentToolSpec> {
    let all = build_default_tool_specs(false);
    let allow = [
        "read_file",
        "replace_selected_text",
        "apply_text_patch",
        "write_file",
        "list_files",
        "search_project",
        "run_shell_command",
        "remember_fact",
    ];
    all.into_iter()
        .filter(|spec| allow.contains(&spec.name.as_str()))
        .collect()
}
```

- [ ] **Step 5: Use profile in providers when constructing tool schemas**

```rust
let runtime_settings = config_provider.load_agent_runtime(Some(&request.project_path))?;
let tool_specs = tool_specs_for_profile(&runtime_settings.tool_profile);

body["tools"] = json!(
    tool_specs
        .iter()
        .map(|spec| to_chat_completions_tool_schema(spec, &runtime.provider))
        .collect::<Vec<_>>()
);
```

- [ ] **Step 6: Set desktop default profile explicitly**

```rust
Ok(AgentRuntimeConfig {
    // existing fields...
    tool_profile: get_string_or(
        get_in(&effective, &["integrations", "agent", "toolProfile"]),
        "full_academic",
    ),
})
```

- [ ] **Step 7: Run core tests**

Run: `cargo test -p agent-core tools::tests -v`
Expected: PASS for existing tests and new profile tests.

- [ ] **Step 8: Commit**

```bash
git add crates/agent-core/src/config.rs crates/agent-core/src/tools.rs crates/agent-core/src/providers/chat_completions.rs crates/agent-core/src/providers/openai.rs apps/desktop/src-tauri/src/settings/mod.rs
git commit -m "feat(agent-core): add tool profiles and coding_cli schema filtering"
```

---

### Task 7: Approval UX, Resume Flow, and Slash Command Actions

**Files:**
- Create: `crates/agent-cli/src/repl.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Modify: `crates/agent-cli/src/turn_runner.rs`
- Test: `crates/agent-cli/src/repl.rs`

- [ ] **Step 1: Write failing command-action tests**

```rust
#[tokio::test]
async fn approve_once_sets_tool_approval_state() {
    let state = AgentRuntimeState::default();
    handle_command(&state, "cli", CliCommand::Approve { scope: ApprovalScope::Once, tool: "run_shell_command".to_string() }).await.unwrap();
    let approval = state.check_tool_approval("cli", "run_shell_command").await;
    assert_eq!(approval.allow_once_remaining, 1);
}

#[tokio::test]
async fn deny_marks_session_denied() {
    let state = AgentRuntimeState::default();
    handle_command(&state, "cli", CliCommand::Deny { tool: "run_shell_command".to_string() }).await.unwrap();
    let approval = state.check_tool_approval("cli", "run_shell_command").await;
    assert!(approval.deny_session);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p agent-cli repl::tests::approve_once_sets_tool_approval_state -v`
Expected: FAIL due missing command handler.

- [ ] **Step 3: Implement approval command actions**

```rust
pub async fn handle_command(
    state: &AgentRuntimeState,
    tab_id: &str,
    command: CliCommand,
) -> Result<CommandAction, String> {
    match command {
        CliCommand::Approve { scope, tool } => {
            let decision = match scope {
                ApprovalScope::Once => "allow_once",
                ApprovalScope::Session => "allow_session",
            };
            state.set_tool_approval(tab_id, &tool, decision).await?;
            Ok(CommandAction::Continue)
        }
        CliCommand::Deny { tool } => {
            state.set_tool_approval(tab_id, &tool, "deny_session").await?;
            Ok(CommandAction::Continue)
        }
        CliCommand::Resume => Ok(CommandAction::ResumePending),
        CliCommand::Exit => Ok(CommandAction::Exit),
        _ => Ok(CommandAction::Continue),
    }
}
```

- [ ] **Step 4: Implement pending-turn resume in CLI**

```rust
pub async fn resume_pending_turn(
    sink: &dyn EventSink,
    config_provider: &dyn ConfigProvider,
    state: &AgentRuntimeState,
    tab_id: &str,
    tool_executor: ToolExecutorFn,
) -> Result<Option<String>, String> {
    let Some(pending) = state.take_pending_turn(tab_id).await else {
        return Ok(None);
    };

    let prior_history = if let Some(session_id) = pending.local_session_id.as_ref() {
        state.history_for_session(session_id).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    let request = AgentTurnDescriptor {
        project_path: pending.project_path.clone(),
        prompt: pending.continuation_prompt.clone(),
        tab_id: tab_id.to_string(),
        model: pending.model.clone(),
        local_session_id: pending.local_session_id.clone(),
        previous_response_id: None,
        turn_profile: pending.turn_profile.clone(),
    };

    let outcome = run_turn(sink, config_provider, state, &request, &prior_history, tool_executor).await?;
    let runtime = config_provider.load_agent_runtime(Some(&request.project_path))?;
    let session_id = persist_turn_outcome(state, &runtime.provider, &runtime.model, &request, &outcome).await;
    Ok(Some(session_id))
}
```

- [ ] **Step 5: Run CLI unit tests**

Run: `cargo test -p agent-cli repl::tests -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/repl.rs crates/agent-cli/src/main.rs crates/agent-cli/src/turn_runner.rs
git commit -m "feat(agent-cli): add approval commands and pending-turn resume flow"
```

---

### Task 8: End-to-End Verification, Docs, and Handoff Update

**Files:**
- Modify: `docs/superpowers/crates-agent-handoff.md`
- Optional Modify: `README.zh-CN.md`

- [ ] **Step 1: Add CLI usage examples and command reference**

```md
## agent-cli Claude-Code mode

### Start interactive session (human output)
AGENT_API_KEY=... cargo run -p agent-cli --bin agent-runtime -- \
  --provider minimax \
  --model MiniMax-M1 \
  --project-path .

### JSONL mode
AGENT_API_KEY=... cargo run -p agent-cli --bin agent-runtime -- \
  --provider minimax \
  --model MiniMax-M1 \
  --project-path . \
  --output jsonl

### Slash commands
/help
/session list
/session select <id>
/approve once run_shell_command
/deny run_shell_command
/resume
/exit
```

- [ ] **Step 2: Run full verification gate**

Run: `cargo build -p agent-core && cargo build -p agent-local-tools && cargo build -p agent-cli`
Expected: all crates build successfully.

Run: `cargo test -p agent-core --lib && cargo test -p agent-local-tools && cargo test -p agent-cli`
Expected: PASS.

Run: `cargo clippy -p agent-core -p agent-local-tools -p agent-cli -- -D warnings`
Expected: PASS without warnings.

- [ ] **Step 3: Manual smoke test (interactive, approval, resume)**

Run:

```bash
AGENT_API_KEY="$AGENT_API_KEY" cargo run -p agent-cli --bin agent-runtime -- \
  --provider minimax \
  --model MiniMax-M1 \
  --project-path .
```

Interactive script:
1. Input: `List files in this repo and summarize top-level architecture.`
2. Input: `Run shell command \`rg --files | head -n 5\`.`
3. Input: `/approve once run_shell_command`
4. Input: `/resume`
5. Exit and restart CLI.
6. Input: `/session list` then `/session select <previous id>` and continue chat.

Expected:
- Human-readable streamed answer.
- Approval required before shell command execution.
- `/resume` continues suspended turn.
- Session history visible after restart.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/crates-agent-handoff.md README.zh-CN.md
git commit -m "docs(agent-cli): document claude-code style minimax runtime workflow"
```

---

## Final Acceptance Criteria

1. `agent-runtime` supports both single-turn (`--prompt`) and interactive REPL (default when no `--prompt`).
2. Default output is human-readable; `--output jsonl` preserves JSONL event stream compatibility.
3. Provider path is minimax-first and stable for multi-turn continuation.
4. CLI has real coding tool execution (`read/list/search/edit/write/shell/memory`) instead of fallback errors.
5. Dangerous actions require explicit approval and can be resumed with `/resume`.
6. Session and history survive process restart.
7. Unsupported academic tools are filtered out in CLI mode through `coding_cli` tool profile.
8. All builds/tests/clippy gates pass for affected crates.

---

## Self-Review

### 1. Spec coverage check
- REPL + command-line interaction: covered in Tasks 1, 2, 7.
- Real tool execution: covered in Task 5.
- Minimax priority: covered in Task 3 and acceptance criteria.
- Human output + JSONL compatibility: covered in Task 2.
- In-memory + persisted sessions: covered in Tasks 3 and 4.
- Dangerous action confirmation: covered in Task 7.
- Complete final plan with implementation choices: covered in options evaluation and task-level code.

### 2. Placeholder scan
- No `TODO`, `TBD`, or deferred placeholders in tasks.
- Every code-change step includes concrete code snippets.
- Every test step includes exact command and expected result.

### 3. Type/signature consistency
- `AgentRuntimeConfig.tool_profile` introduced once and consumed consistently by providers and settings loader.
- `CliCommand` enum and parser are used consistently by `repl` command dispatcher.
- `persist_turn_outcome` and `resolve_previous_response_id` helpers are reused for both normal turns and resume flow.

---

## Quality Reassessment Addendum (2026-04-15)

> 这一节是“做得像 Claude Code CLI 一样好”的补充约束，优先级高于上文任务顺序。目标不只是“能跑”，而是“交互可靠、工具安全、恢复稳定、扩展可持续”。

### A. North-Star 能力目标（必须同时满足）

1. **交互质量**：默认 human REPL 输出具备稳定的流式体验（状态、delta、tool call/result、错误）且不打断输入节奏。  
2. **执行质量**：工具执行支持“按调用级别”的并发分组（而非按工具类型硬编码），并对非并发安全调用强制串行。  
3. **安全质量**：权限是分层决策（规则源优先级 + 会话授权 + deny 跟踪），默认 fail-closed。  
4. **恢复质量**：`approval` / `resume` / `cancel` 都是状态机事件，不是 ad-hoc 分支；中断后可继续而不重复前文工作。  
5. **持久化质量**：会话恢复数据采用原子写、版本化快照、向后兼容读取，避免损坏后全量不可用。  
6. **演进质量**：`agent-core` 继续保持平台无关；CLI/desktop 差异仅在 adapter、executor、output。

### B. 对标参考实现后的关键结论

对照 `reference/claude-code-main` 后，本计划必须补齐以下“质量项”：

1. **启动与加载策略**：应采用“快速路径 + 懒加载”思路，避免 REPL 冷启动被重模块拖慢（参考 `src/main.tsx` 的入口分层与延迟加载）。  
2. **对话循环语义**：需要显式 `transition reason` 与恢复路径（`max_output_tokens`、compact、tool follow-up），而不只是一层 provider dispatch。  
3. **工具编排语义**：并发安全判定应是 per-call（参数相关），并进行连续分组后并发执行（参考 `services/tools/toolOrchestration.ts`）。  
4. **权限管道语义**：规则源分层、deny/ask/allow 解释、审批原因输出、拒绝追踪都应成为 CLI 一等能力（参考 `utils/permissions/permissions.ts`）。  
5. **会话存储语义**：日志/转录/恢复要考虑大文件、兼容旧格式、临时进度消息过滤（参考 `utils/sessionStorage.ts` 的链路约束）。

### C. 关键修订（强制）

#### C1. 任务顺序重排（替代原 Task 1~8 执行顺序）

执行顺序改为：

1. `Task 1`（命令与参数）  
2. `Task 6`（tool profile/filtering）  
3. `Task 2`（输出层）  
4. `Task 3`（turn runner + continuation）  
5. `Task 5`（real local tools）  
6. `Task 7`（approve/resume/cancel 状态机）  
7. `Task 4`（持久化）  
8. `Task 8`（端到端验收与文档）

理由：先过滤工具面，再接入真实执行器，避免模型先学到“unsupported tool”坏行为。

#### C2. 审批契约（Task 5/7 前置约束）

`agent-local-tools` 的可审批工具返回体必须兼容 `agent-core` 中断逻辑，至少包含：

- `approvalRequired: true|false`
- `approvalToolName: <tool>`
- `reason: <human-readable>`
- 编辑类工具在需要 review 时必须带 `reviewArtifact: true` 与 `reviewArtifactPayload`

否则 `tool_result_requires_approval()` 与 `pending_turn` 存储链路不会生效。

#### C3. Provider 策略修订（Task 3）

“minimax-first”定义为**默认优先**，不是“其他 provider 硬失败”。  
`openai/deepseek` 应提供降级运行路径（例如禁用部分 CLI 专属能力或提示 capability 差异），保持多 provider 统一接口。

#### C4. 命令集补项（Task 1/7）

必须新增 `/cancel`，并接入 `AgentRuntimeState::cancel_tab()`，与 `/resume` 对称。  
同时 REPL 侧要显式使用 `acquire_turn_guard/release_turn_guard`，防止并发 turn 污染状态。

#### C5. 持久化质量补项（Task 4）

`SessionStore` 增加：

- `schema_version`
- 原子写（tmp + rename）
- 读取容错（坏行/坏文件回退）
- `active_session_id` 实际使用（启动恢复焦点会话）

### D. 新增验收门槛（“做得好”而不仅“能做”）

除 build/test/clippy 外，新增质量验收：

1. **交互连贯性**：连续 20 轮 REPL 对话，无明显输出错序/卡死。  
2. **审批闭环**：`shell -> approval -> resume` 全链路可重复 5 次且无悬挂 pending turn。  
3. **取消鲁棒性**：运行中 `/cancel` 后 2 秒内可重新发起下一轮，不遗留活跃锁。  
4. **并发安全性**：并发安全工具批处理可并行，非安全工具串行（有日志证据）。  
5. **恢复一致性**：重启 CLI 后 `/session list/select` 可继续上下文，且历史不重复注入。  
6. **工具面一致性**：`coding_cli` profile 下不会向模型暴露 academic-only tools。

### E. 参考实现映射（用于 code review checklist）

- 启动快路径与懒加载：`reference/claude-code-main/src/main.tsx`
- REPL 启动封装：`reference/claude-code-main/src/replLauncher.tsx`
- 对话循环状态机：`reference/claude-code-main/src/query.ts` / `src/QueryEngine.ts`
- 工具并发分组：`reference/claude-code-main/src/services/tools/toolOrchestration.ts`
- 权限分层：`reference/claude-code-main/src/utils/permissions/permissions.ts`
- 会话存储与恢复：`reference/claude-code-main/src/utils/sessionStorage.ts`
