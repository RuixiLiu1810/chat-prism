# Agent CLI Full TUI (S3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a default full-screen REPL TUI for `agent-cli` (`L1 + I1`) with semantic-first timeline output and expandable technical details, while preserving existing `jsonl` and classic fallback behavior.

**Architecture:** Keep `agent-core` untouched. Implement a new `tui` module in `agent-cli` with clear boundaries: event ingestion (`EventSink` channel), semantic mapping (`event_bridge`), state reduction (`view_model`), rendering (`renderer`), and keyboard/input orchestration (`input` + `shell`). Route REPL to this TUI by default in human mode, with explicit classic fallback.

**Tech Stack:** Rust 2021, tokio, crossterm (alternate screen + keyboard events), existing `agent-core` events and turn loop, clap-based CLI parsing

---

## Scope Check

This plan covers one subsystem: **`agent-cli` interactive runtime UX**.  
It does not change `agent-core` protocols, provider logic, or desktop runtime behavior.

## File Structure

| File | Responsibility |
|---|---|
| `crates/agent-cli/Cargo.toml` | Add TUI runtime dependency (`crossterm`) |
| `crates/agent-cli/src/args.rs` | Add `UiMode` parsing (`tui` / `classic`) |
| `crates/agent-cli/src/main.rs` | Dispatch REPL path to TUI shell, preserve fallback and jsonl behavior |
| `crates/agent-cli/src/tui/mod.rs` | TUI module exports |
| `crates/agent-cli/src/tui/types.rs` | Shared TUI domain types (`UiLine`, `UiFocus`, `ViewUpdate`, etc.) |
| `crates/agent-cli/src/tui/view_model.rs` | Reducer/state transitions for semantic timeline + detail expansion |
| `crates/agent-cli/src/tui/event_bridge.rs` | Map `AgentEventPayload` to semantic `ViewUpdate` |
| `crates/agent-cli/src/tui/input.rs` | Key mapping and input/history/focus actions |
| `crates/agent-cli/src/tui/renderer.rs` | L1 frame rendering (header/timeline/input) |
| `crates/agent-cli/src/tui/shell.rs` | Fullscreen event loop, turn execution integration, approval handling |
| `docs/superpowers/crates-agent-handoff.md` | Update crate-level operator docs for TUI runtime and fallback |

---

### Task 1: Add UI Mode Plumbing and Dependency

**Files:**
- Modify: `crates/agent-cli/Cargo.toml`
- Modify: `crates/agent-cli/src/args.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/args.rs`
- Test: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: Write failing tests for `UiMode` parsing**

```rust
#[test]
fn parses_ui_mode_tui_and_classic() {
    assert_eq!(parse_ui_mode("tui").unwrap_or_else(|e| panic!("{e}")), UiMode::Tui);
    assert_eq!(parse_ui_mode("classic").unwrap_or_else(|e| panic!("{e}")), UiMode::Classic);
}

#[test]
fn rejects_unknown_ui_mode() {
    let err = parse_ui_mode("fancy").err();
    assert!(err.is_some());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p agent-cli args::tests::parses_ui_mode_tui_and_classic -v`  
Expected: FAIL with missing `UiMode` / `parse_ui_mode`.

- [ ] **Step 3: Add `UiMode` to `args.rs` and CLI/env wiring**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Tui,
    Classic,
}

pub fn parse_ui_mode(raw: &str) -> Result<UiMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "tui" => Ok(UiMode::Tui),
        "classic" => Ok(UiMode::Classic),
        other => Err(format!(
            "Unsupported ui mode '{}'. Use 'tui' or 'classic'.",
            other
        )),
    }
}

#[derive(Parser, Debug, Clone)]
pub struct Args {
    // ...
    #[arg(long, env = "AGENT_UI_MODE")]
    pub ui_mode: Option<String>,
}
```

- [ ] **Step 4: Add dependency and dispatch helper skeleton in `main.rs`**

```rust
fn resolve_ui_mode(args: &Args) -> Result<args::UiMode, String> {
    args::parse_ui_mode(args.ui_mode.as_deref().unwrap_or("tui"))
}

fn should_use_tui(run_mode: RunMode, output_mode: OutputMode, ui_mode: args::UiMode) -> bool {
    run_mode == RunMode::Repl && output_mode == OutputMode::Human && ui_mode == args::UiMode::Tui
}
```

```toml
# crates/agent-cli/Cargo.toml
[dependencies]
agent-core = { path = "../agent-core" }
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
clap = { version = "4", features = ["derive", "env"] }
crossterm = "0.28"
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli args::tests::parses_ui_mode_tui_and_classic -v`  
Expected: PASS.

Run: `cargo test -p agent-cli tests::completion_outcome_uses_completed_for_non_suspended_turns -v`  
Expected: PASS (no regression).

Commit:

```bash
git add crates/agent-cli/Cargo.toml crates/agent-cli/src/args.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add ui mode plumbing for tui/classic dispatch"
```

---

### Task 2: Create TUI Domain Types and View Reducer

**Files:**
- Create: `crates/agent-cli/src/tui/mod.rs`
- Create: `crates/agent-cli/src/tui/types.rs`
- Create: `crates/agent-cli/src/tui/view_model.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/tui/view_model.rs`

- [ ] **Step 1: Write failing reducer tests for semantic timeline + detail expansion**

```rust
#[test]
fn appends_user_assistant_semantic_lines() {
    let mut vm = TuiViewModel::new("session-1".to_string());
    vm.push_user_prompt("read one file".to_string());
    vm.apply_update(ViewUpdate::AssistantDelta("I will inspect now.".to_string()));
    vm.apply_update(ViewUpdate::Semantic {
        text: "Read 1 file".to_string(),
        detail: "tool=read_file path=src/main.rs".to_string(),
    });

    assert_eq!(vm.lines.len(), 3);
    assert_eq!(vm.lines[0].prefix, "›");
    assert_eq!(vm.lines[1].prefix, "●");
    assert_eq!(vm.lines[2].prefix, "└");
}

#[test]
fn toggles_detail_only_for_semantic_line() {
    let mut vm = TuiViewModel::new("session-1".to_string());
    vm.apply_update(ViewUpdate::Semantic {
        text: "Waiting for approval".to_string(),
        detail: "run /approve shell once".to_string(),
    });
    vm.focus = UiFocus::Timeline;
    vm.selected_line = 0;
    vm.toggle_detail();
    assert!(vm.lines[0].expanded);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli tui::view_model::tests::appends_user_assistant_semantic_lines -v`  
Expected: FAIL because `tui` module does not exist.

- [ ] **Step 3: Add shared TUI types**

```rust
// crates/agent-cli/src/tui/types.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiFocus {
    Input,
    Timeline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiLineKind {
    User,
    Assistant,
    Semantic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiLine {
    pub kind: UiLineKind,
    pub prefix: String,
    pub text: String,
    pub details: Vec<String>,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewUpdate {
    AssistantDelta(String),
    Semantic { text: String, detail: String },
    WaitingApproval(String),
    TurnOutcome(String),
    Error(String),
}
```

- [ ] **Step 4: Implement reducer in `view_model.rs`**

```rust
pub struct TuiViewModel {
    pub session_id: String,
    pub lines: Vec<UiLine>,
    pub focus: UiFocus,
    pub selected_line: usize,
    pub input_buffer: String,
    pub input_history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub waiting_for_approval: bool,
}

impl TuiViewModel {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            lines: Vec::new(),
            focus: UiFocus::Input,
            selected_line: 0,
            input_buffer: String::new(),
            input_history: Vec::new(),
            history_cursor: None,
            waiting_for_approval: false,
        }
    }

    pub fn push_user_prompt(&mut self, prompt: String) {
        self.input_history.push(prompt.clone());
        self.history_cursor = None;
        self.lines.push(UiLine {
            kind: UiLineKind::User,
            prefix: "›".to_string(),
            text: prompt,
            details: Vec::new(),
            expanded: false,
        });
        self.selected_line = self.lines.len().saturating_sub(1);
    }

    pub fn apply_update(&mut self, update: ViewUpdate) {
        match update {
            ViewUpdate::AssistantDelta(delta) => self.lines.push(UiLine {
                kind: UiLineKind::Assistant,
                prefix: "●".to_string(),
                text: delta,
                details: Vec::new(),
                expanded: false,
            }),
            ViewUpdate::Semantic { text, detail } => self.lines.push(UiLine {
                kind: UiLineKind::Semantic,
                prefix: "└".to_string(),
                text,
                details: vec![detail],
                expanded: false,
            }),
            ViewUpdate::WaitingApproval(hint) => {
                self.waiting_for_approval = true;
                self.lines.push(UiLine {
                    kind: UiLineKind::Semantic,
                    prefix: "└".to_string(),
                    text: "Waiting for approval".to_string(),
                    details: vec![hint],
                    expanded: false,
                });
            }
            ViewUpdate::TurnOutcome(outcome) => {
                self.waiting_for_approval = outcome == "suspended";
            }
            ViewUpdate::Error(message) => self.lines.push(UiLine {
                kind: UiLineKind::Semantic,
                prefix: "└".to_string(),
                text: format!("Error: {}", message),
                details: Vec::new(),
                expanded: false,
            }),
        }
        self.selected_line = self.lines.len().saturating_sub(1);
    }

    pub fn toggle_detail(&mut self) {
        if self.focus != UiFocus::Timeline {
            return;
        }
        if let Some(line) = self.lines.get_mut(self.selected_line) {
            if line.kind == UiLineKind::Semantic && !line.details.is_empty() {
                line.expanded = !line.expanded;
            }
        }
    }
}
```

- [ ] **Step 5: Wire module exports and commit**

```rust
// crates/agent-cli/src/tui/mod.rs
pub mod types;
pub mod view_model;
```

Run: `cargo test -p agent-cli tui::view_model::tests -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/tui/mod.rs crates/agent-cli/src/tui/types.rs crates/agent-cli/src/tui/view_model.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add tui domain types and view reducer"
```

---

### Task 3: Build Event Bridge and Channel Event Sink

**Files:**
- Create: `crates/agent-cli/src/tui/event_bridge.rs`
- Modify: `crates/agent-cli/src/tui/mod.rs`
- Test: `crates/agent-cli/src/tui/event_bridge.rs`

- [ ] **Step 1: Write failing tests for semantic mapping**

```rust
#[test]
fn maps_tool_result_to_semantic_update() {
    let payload = AgentEventPayload::ToolResult(AgentToolResultEvent {
        tool_name: "read_file".to_string(),
        call_id: "call-1".to_string(),
        is_error: false,
        preview: "Read src/main.rs".to_string(),
        content: serde_json::json!({"path":"src/main.rs"}),
        display: serde_json::Value::Null,
    });

    let update = map_payload(&payload).unwrap_or_else(|| panic!("must map"));
    assert!(matches!(update, ViewUpdate::Semantic { .. }));
}

#[test]
fn maps_approval_required_to_waiting_approval() {
    let payload = AgentEventPayload::ToolResult(AgentToolResultEvent {
        tool_name: "run_shell_command".to_string(),
        call_id: "call-2".to_string(),
        is_error: true,
        preview: "requires approval".to_string(),
        content: serde_json::json!({"approvalRequired": true}),
        display: serde_json::Value::Null,
    });
    let update = map_payload(&payload).unwrap_or_else(|| panic!("must map"));
    assert!(matches!(update, ViewUpdate::WaitingApproval(_)));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli tui::event_bridge::tests::maps_tool_result_to_semantic_update -v`  
Expected: FAIL because `event_bridge` module is missing.

- [ ] **Step 3: Implement payload mapping**

```rust
pub fn map_payload(payload: &AgentEventPayload) -> Option<ViewUpdate> {
    match payload {
        AgentEventPayload::MessageDelta(delta) => {
            let text = delta.delta.trim();
            if text.is_empty() {
                None
            } else {
                Some(ViewUpdate::AssistantDelta(text.to_string()))
            }
        }
        AgentEventPayload::ToolResult(result) => {
            if result
                .content
                .get("approvalRequired")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                Some(ViewUpdate::WaitingApproval(
                    "run /approve shell once or /approve shell session".to_string(),
                ))
            } else {
                Some(ViewUpdate::Semantic {
                    text: result.preview.clone(),
                    detail: format!(
                        "tool={} call_id={} is_error={}",
                        result.tool_name, result.call_id, result.is_error
                    ),
                })
            }
        }
        AgentEventPayload::Error(err) => Some(ViewUpdate::Error(err.message.clone())),
        AgentEventPayload::Status(status) => {
            if status.stage == "awaiting_approval" {
                Some(ViewUpdate::WaitingApproval(status.message.clone()))
            } else {
                None
            }
        }
        _ => None,
    }
}
```

- [ ] **Step 4: Implement channel sink for TUI runtime**

```rust
#[derive(Debug, Clone)]
pub enum TuiRuntimeEvent {
    AgentEvent(AgentEventEnvelope),
    AgentComplete(AgentCompletePayload),
}

pub struct ChannelEventSink {
    tx: tokio::sync::mpsc::UnboundedSender<TuiRuntimeEvent>,
}

impl ChannelEventSink {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<TuiRuntimeEvent>) -> Self {
        Self { tx }
    }
}

impl EventSink for ChannelEventSink {
    fn emit_event(&self, envelope: &AgentEventEnvelope) {
        let _ = self.tx.send(TuiRuntimeEvent::AgentEvent(envelope.clone()));
    }

    fn emit_complete(&self, payload: &AgentCompletePayload) {
        let _ = self.tx.send(TuiRuntimeEvent::AgentComplete(payload.clone()));
    }
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli tui::event_bridge::tests -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/tui/event_bridge.rs crates/agent-cli/src/tui/mod.rs
git commit -m "feat(agent-cli): add tui event bridge and channel event sink"
```

---

### Task 4: Implement L1 Renderer (Header + Timeline + I1 Input)

**Files:**
- Create: `crates/agent-cli/src/tui/renderer.rs`
- Modify: `crates/agent-cli/src/tui/mod.rs`
- Test: `crates/agent-cli/src/tui/renderer.rs`

- [ ] **Step 1: Write failing renderer tests**

```rust
#[test]
fn renders_l1_header_with_required_fields() {
    let snapshot = CliStatusSnapshot {
        provider: "minimax".to_string(),
        model: "MiniMax-M1".to_string(),
        project_path: "/tmp/p".to_string(),
        git_branch: "main".to_string(),
        git_dirty: true,
        session_id: "session-1".to_string(),
        output_mode: "human".to_string(),
    };
    let vm = TuiViewModel::new("session-1".to_string());
    let lines = render_frame(&snapshot, &vm, 100, 24);
    assert!(lines.iter().any(|l| l.contains("minimax/MiniMax-M1")));
    assert!(lines.iter().any(|l| l.contains("main*")));
}

#[test]
fn renders_expanded_detail_under_semantic_line() {
    let mut vm = TuiViewModel::new("session-1".to_string());
    vm.apply_update(ViewUpdate::Semantic {
        text: "Read src/main.rs".to_string(),
        detail: "tool=read_file path=src/main.rs".to_string(),
    });
    vm.focus = UiFocus::Timeline;
    vm.selected_line = 0;
    vm.toggle_detail();
    let snapshot = CliStatusSnapshot::collect("minimax", "MiniMax-M1", ".", "session-1", "human");
    let lines = render_frame(&snapshot, &vm, 120, 30);
    assert!(lines.iter().any(|l| l.contains("tool=read_file")));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli tui::renderer::tests::renders_l1_header_with_required_fields -v`  
Expected: FAIL because `renderer` module does not exist.

- [ ] **Step 3: Implement frame renderer**

```rust
pub fn render_header(snapshot: &CliStatusSnapshot, width: usize) -> String {
    let dirty = if snapshot.git_dirty { "*" } else { "" };
    let text = format!(
        "{} / {} | {} | {}{} | {} | {}",
        snapshot.provider,
        snapshot.model,
        snapshot.project_path,
        snapshot.git_branch,
        dirty,
        snapshot.session_id,
        snapshot.output_mode
    );
    truncate_to_width(&text, width)
}

pub fn render_frame(
    snapshot: &CliStatusSnapshot,
    vm: &TuiViewModel,
    width: u16,
    height: u16,
) -> Vec<String> {
    let width_usize = width as usize;
    let mut out = Vec::new();
    out.push(render_header(snapshot, width_usize));
    out.push("─".repeat(width_usize));

    let body_height = height.saturating_sub(4) as usize;
    let mut body_lines = Vec::new();
    for line in &vm.lines {
        body_lines.push(truncate_to_width(&format!("{} {}", line.prefix, line.text), width_usize));
        if line.expanded {
            for detail in &line.details {
                body_lines.push(truncate_to_width(&format!("  {}", detail), width_usize));
            }
        }
    }
    if body_lines.len() > body_height {
        body_lines = body_lines[body_lines.len() - body_height..].to_vec();
    }
    out.extend(body_lines);
    while out.len() < (height as usize).saturating_sub(2) {
        out.push(String::new());
    }
    out.push("─".repeat(width_usize));
    out.push(truncate_to_width(&format!("> {}", vm.input_buffer), width_usize));
    out
}
```

- [ ] **Step 4: Export renderer module**

```rust
// crates/agent-cli/src/tui/mod.rs
pub mod event_bridge;
pub mod renderer;
pub mod types;
pub mod view_model;
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli tui::renderer::tests -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/tui/renderer.rs crates/agent-cli/src/tui/mod.rs
git commit -m "feat(agent-cli): add l1 tui renderer with semantic detail expansion"
```

---

### Task 5: Implement Input Controller (I1 + Focus + History)

**Files:**
- Create: `crates/agent-cli/src/tui/input.rs`
- Modify: `crates/agent-cli/src/tui/mod.rs`
- Test: `crates/agent-cli/src/tui/input.rs`

- [ ] **Step 1: Write failing tests for key-to-action mapping**

```rust
#[test]
fn maps_ctrl_j_and_ctrl_k_to_timeline_navigation() {
    let down = to_action(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    let up = to_action(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    assert_eq!(down, Some(UiAction::FocusNextLine));
    assert_eq!(up, Some(UiAction::FocusPrevLine));
}

#[test]
fn maps_enter_to_submit_when_input_focused() {
    let action = to_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(action, Some(UiAction::Enter));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli tui::input::tests::maps_ctrl_j_and_ctrl_k_to_timeline_navigation -v`  
Expected: FAIL because `input` module does not exist.

- [ ] **Step 3: Implement `UiAction` mapping**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Enter,
    Backspace,
    InsertChar(char),
    HistoryUp,
    HistoryDown,
    FocusNextLine,
    FocusPrevLine,
    ToggleDetail,
    FocusInput,
    ClearScreen,
    Exit,
    Noop,
}

pub fn to_action(key: crossterm::event::KeyEvent) -> Option<UiAction> {
    use crossterm::event::{KeyCode, KeyModifiers};
    match (key.code, key.modifiers) {
        (KeyCode::Enter, _) => Some(UiAction::Enter),
        (KeyCode::Backspace, _) => Some(UiAction::Backspace),
        (KeyCode::Up, _) => Some(UiAction::HistoryUp),
        (KeyCode::Down, _) => Some(UiAction::HistoryDown),
        (KeyCode::Esc, _) => Some(UiAction::FocusInput),
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => Some(UiAction::ClearScreen),
        (KeyCode::Char('j'), KeyModifiers::CONTROL) => Some(UiAction::FocusNextLine),
        (KeyCode::Char('k'), KeyModifiers::CONTROL) => Some(UiAction::FocusPrevLine),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(UiAction::Exit),
        (KeyCode::Char(ch), KeyModifiers::NONE) => Some(UiAction::InsertChar(ch)),
        _ => Some(UiAction::Noop),
    }
}
```

- [ ] **Step 4: Add reducer helper for applying input actions**

```rust
pub fn apply_input_action(vm: &mut TuiViewModel, action: UiAction) -> Option<String> {
    match action {
        UiAction::InsertChar(ch) => {
            vm.input_buffer.push(ch);
            None
        }
        UiAction::Backspace => {
            vm.input_buffer.pop();
            None
        }
        UiAction::Enter => {
            if vm.focus == UiFocus::Timeline {
                vm.toggle_detail();
                return None;
            }
            let prompt = vm.input_buffer.trim().to_string();
            vm.input_buffer.clear();
            if prompt.is_empty() {
                None
            } else {
                Some(prompt)
            }
        }
        UiAction::FocusNextLine => {
            vm.focus = UiFocus::Timeline;
            vm.selected_line = (vm.selected_line + 1).min(vm.lines.len().saturating_sub(1));
            None
        }
        UiAction::FocusPrevLine => {
            vm.focus = UiFocus::Timeline;
            vm.selected_line = vm.selected_line.saturating_sub(1);
            None
        }
        UiAction::FocusInput => {
            vm.focus = UiFocus::Input;
            None
        }
        _ => None,
    }
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli tui::input::tests -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/tui/input.rs crates/agent-cli/src/tui/mod.rs
git commit -m "feat(agent-cli): add tui input controller and key action mapping"
```

---

### Task 6: Implement Fullscreen TUI Shell Runtime

**Files:**
- Create: `crates/agent-cli/src/tui/shell.rs`
- Modify: `crates/agent-cli/src/tui/mod.rs`
- Modify: `crates/agent-cli/src/tui/types.rs`
- Test: `crates/agent-cli/src/tui/shell.rs`

- [ ] **Step 1: Write failing tests for shell-level state decisions**

```rust
#[test]
fn keeps_same_session_on_suspended_outcome() {
    let mut vm = TuiViewModel::new("tab-session".to_string());
    vm.apply_update(ViewUpdate::TurnOutcome("suspended".to_string()));
    assert!(vm.waiting_for_approval);
    assert_eq!(vm.session_id, "tab-session");
}

#[test]
fn completes_turn_clears_waiting_flag() {
    let mut vm = TuiViewModel::new("tab-session".to_string());
    vm.apply_update(ViewUpdate::WaitingApproval("approve shell".to_string()));
    vm.apply_update(ViewUpdate::TurnOutcome("completed".to_string()));
    assert!(!vm.waiting_for_approval);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli tui::shell::tests::keeps_same_session_on_suspended_outcome -v`  
Expected: FAIL because `shell` module does not exist.

- [ ] **Step 3: Implement alternate-screen lifecycle and draw loop**

```rust
pub async fn run_tui_shell(
    args: Args,
    resolved: ResolvedConfig,
    runtime_state: Arc<AgentRuntimeState>,
    tool_mode: ToolMode,
) -> Result<(), String> {
    use crossterm::{execute, terminal::{EnterAlternateScreen, LeaveAlternateScreen, enable_raw_mode, disable_raw_mode}};
    let mut stdout = std::io::stdout();
    enable_raw_mode().map_err(|e| format!("enable_raw_mode failed: {e}"))?;
    execute!(stdout, EnterAlternateScreen).map_err(|e| format!("enter alt screen failed: {e}"))?;

    let result = run_tui_loop(args, resolved, runtime_state, tool_mode).await;

    let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    result
}
```

- [ ] **Step 4: Implement event loop with turn execution and approval continuity**

```rust
async fn run_tui_loop(
    args: Args,
    resolved: ResolvedConfig,
    runtime_state: Arc<AgentRuntimeState>,
    tool_mode: ToolMode,
) -> Result<(), String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TuiRuntimeEvent>();
    let sink: Arc<dyn EventSink> = Arc::new(ChannelEventSink::new(tx));
    let mut vm = TuiViewModel::new(format!("{}-session", args.tab_id));

    loop {
        // 1) poll key input
        if crossterm::event::poll(std::time::Duration::from_millis(16))
            .map_err(|e| format!("poll failed: {e}"))?
        {
            if let crossterm::event::Event::Key(key) =
                crossterm::event::read().map_err(|e| format!("read failed: {e}"))?
            {
                if let Some(prompt) = apply_input_action(&mut vm, to_action(key).unwrap_or(UiAction::Noop)) {
                    vm.push_user_prompt(prompt.clone());
                    spawn_turn(prompt, &args, &resolved, Arc::clone(&runtime_state), Arc::clone(&sink), tool_mode, vm.session_id.clone());
                }
            }
        }

        // 2) drain runtime events
        while let Ok(event) = rx.try_recv() {
            match event {
                TuiRuntimeEvent::AgentEvent(envelope) => {
                    if let Some(update) = map_payload(&envelope.payload) {
                        vm.apply_update(update);
                    }
                }
                TuiRuntimeEvent::AgentComplete(done) => {
                    vm.apply_update(ViewUpdate::TurnOutcome(done.outcome));
                }
            }
        }

        // 3) draw frame
        draw_frame(&vm, &args, &resolved)?;
    }
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli tui::shell::tests -v`  
Expected: PASS.

Run: `cargo build -p agent-cli`  
Expected: SUCCESS.

Commit:

```bash
git add crates/agent-cli/src/tui/shell.rs crates/agent-cli/src/tui/mod.rs crates/agent-cli/src/tui/types.rs
git commit -m "feat(agent-cli): add fullscreen tui shell runtime"
```

---

### Task 7: Wire Main Dispatch and Preserve Classic/JSONL Fallback

**Files:**
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: Write failing tests for TUI dispatch conditions**

```rust
#[test]
fn chooses_tui_for_repl_human_tui_mode() {
    assert!(should_use_tui(RunMode::Repl, OutputMode::Human, UiMode::Tui));
}

#[test]
fn keeps_classic_for_jsonl_even_when_tui_mode() {
    assert!(!should_use_tui(RunMode::Repl, OutputMode::Jsonl, UiMode::Tui));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli tests::chooses_tui_for_repl_human_tui_mode -v`  
Expected: FAIL if helper not visible or behavior not implemented.

- [ ] **Step 3: Extract classic REPL path and wire TUI path**

```rust
async fn run_repl_classic(...) -> ExitCode {
    // existing repl::run_repl logic moved here without behavior change
}

// in main:
let ui_mode = match resolve_ui_mode(&args) {
    Ok(mode) => mode,
    Err(err) => {
        eprintln!("agent-runtime error: {}", err);
        return ExitCode::FAILURE;
    }
};

if should_use_tui(args.run_mode(), output_mode, ui_mode) {
    if let Err(err) = tui::shell::run_tui_shell(args.clone(), resolved.clone(), Arc::clone(&runtime_state), tool_mode).await {
        eprintln!("agent-runtime error: {}", err);
        return ExitCode::FAILURE;
    }
    return ExitCode::SUCCESS;
}
```

- [ ] **Step 4: Keep non-interactive paths unchanged**

```rust
// single-turn and jsonl path remain using existing sinks and turn_runner
// no change to ToolMode::Off guardrails
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli tests::chooses_tui_for_repl_human_tui_mode -v`  
Expected: PASS.

Run: `cargo test -p agent-cli -v`  
Expected: PASS (all existing + new tests).

Commit:

```bash
git add crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): route repl human mode to full tui with classic fallback"
```

---

### Task 8: Error Handling Hardening, Docs, and Final Verification

**Files:**
- Modify: `crates/agent-cli/src/tui/event_bridge.rs`
- Modify: `crates/agent-cli/src/tui/shell.rs`
- Modify: `docs/superpowers/crates-agent-handoff.md`
- Test: `crates/agent-cli/src/tui/event_bridge.rs`
- Test: `crates/agent-cli/src/tui/shell.rs`

- [ ] **Step 1: Add failing tests for error semanticization**

```rust
#[test]
fn maps_error_payload_to_semantic_error_line() {
    let payload = AgentEventPayload::Error(AgentErrorEvent {
        code: "turn_loop_failed".to_string(),
        message: "network down".to_string(),
    });
    let update = map_payload(&payload).unwrap_or_else(|| panic!("must map"));
    assert!(matches!(update, ViewUpdate::Error(_)));
}
```

- [ ] **Step 2: Run tests to verify failure (if mapping absent/incomplete)**

Run: `cargo test -p agent-cli tui::event_bridge::tests::maps_error_payload_to_semantic_error_line -v`  
Expected: FAIL before hardening.

- [ ] **Step 3: Harden shell cleanup and fallback-on-render-error behavior**

```rust
pub async fn run_tui_shell(...) -> Result<(), String> {
    let setup = setup_terminal();
    if let Err(err) = setup {
        return Err(format!("tui setup failed: {}", err));
    }

    let loop_result = run_tui_loop(...).await;
    let cleanup_result = cleanup_terminal();
    if let Err(err) = cleanup_result {
        return Err(format!("tui cleanup failed: {}", err));
    }
    loop_result
}
```

- [ ] **Step 4: Update crate handoff docs with TUI behavior and fallback**

```md
## agent-cli TUI Runtime (S3)

- Default REPL (`--output human`) starts fullscreen TUI.
- Use `--ui-mode classic` to force classic line-based REPL.
- `--output jsonl` is unchanged and bypasses TUI.
- Semantic timeline is default (`›/●/└`); detail lines are expandable from timeline focus.
- `suspended` keeps same session context and waits for `/approve ...` in-place.
```

- [ ] **Step 5: Final validation and commit**

Run: `cargo test -p agent-cli -v`  
Expected: PASS.

Run: `cargo clippy -p agent-cli -- -D warnings`  
Expected: PASS.

Run: `cargo build -p agent-cli`  
Expected: SUCCESS.

Run: `cargo run -p agent-cli -- --provider minimax --model MiniMax-M1 --api-key test --output human --ui-mode classic --prompt "hello"`  
Expected: non-TUI path still works.

Commit:

```bash
git add crates/agent-cli/src/tui/event_bridge.rs crates/agent-cli/src/tui/shell.rs docs/superpowers/crates-agent-handoff.md
git commit -m "docs+hardening(agent-cli): finalize tui s3 behavior and fallback guarantees"
```

---

## Spec Coverage Self-Review

1. **Spec coverage check**
- L1 + I1 layout: Task 4 + Task 6.
- Semantic default + expandable detail: Task 2 + Task 3 + Task 4.
- Suspended same-session continuity: Task 2 + Task 6.
- Fullscreen TUI shell: Task 6.
- Classic/jsonl fallback preserved: Task 1 + Task 7 + Task 8.
- Error handling and tests: Task 8.

2. **Placeholder scan**
- No `TODO/TBD/implement later/similar to`.
- All test/code/command steps contain concrete snippets or commands.

3. **Type/signature consistency**
- `UiMode` and `parse_ui_mode` used consistently across Task 1 and Task 7.
- `ViewUpdate` variants introduced in Task 2 and consumed consistently in Task 3/6.
- `TuiRuntimeEvent` used consistently between `ChannelEventSink` and shell loop.

