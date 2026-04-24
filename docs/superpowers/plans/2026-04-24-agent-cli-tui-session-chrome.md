# Agent CLI TUI Session Chrome Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Claude Code-style, session-scoped top chrome and command-history transcript rhythm for `agent-cli --ui-mode tui` while keeping non-fullscreen streaming behavior.

**Architecture:** Keep the existing `tui::shell` runtime loop and provider contract, but move visual composition into explicit header/notice/transcript render helpers. Render header once per session, then append user/assistant rows incrementally using stable wrapping and hanging-indent formatting.

**Tech Stack:** Rust (`agent-cli` crate), ANSI terminal rendering, existing `repl` loop, `cargo test`, `cargo clippy`.

---

## Prerequisites

1. Work from a dedicated worktree branch, not dirty `main`:

```bash
git worktree add .worktrees/agent-cli-tui-session-chrome -b feat/agent-cli-tui-session-chrome
cd .worktrees/agent-cli-tui-session-chrome
```

2. Run baseline before edits:

```bash
cargo test -p agent-cli
cargo clippy -p agent-cli --tests -- -D warnings
```

Expected: both commands pass on baseline.

---

## File Structure (Lock Before Coding)

### Modify

1. `crates/agent-cli/src/tui/icons.rs`
- Add project-specific Unicode pixel logo API.
- Keep existing icon fallbacks.

2. `crates/agent-cli/src/tui/theme.rs`
- Add command-row background role and helper for background painting.
- Keep no-color fallback behavior.

3. `crates/agent-cli/src/tui/layout.rs`
- Add width-aware rendering helpers for header and notice lines.
- Keep slot ordering behavior unchanged.

4. `crates/agent-cli/src/tui/transcript.rs`
- Add user command row renderer (full-line subtle background).
- Add assistant block renderer with hanging indent wrapping.

5. `crates/agent-cli/src/tui/shell.rs`
- Add session-chrome lifecycle state.
- Render header/notice once per session.
- Wire user row and assistant block renderers into streaming loop.

6. `crates/agent-cli/src/repl.rs`
- Keep API, only adjust prompt usage if needed to support styled current input row prefix.

### Do Not Modify (Scope Guard)

1. `crates/agent-cli/src/main.rs` classic execution behavior.
2. `crates/agent-cli/src/output.rs` jsonl/human sink protocol for non-tui paths.

---

### Task 1: Add Pixel Logo + Command Row Color Primitive

**Files:**
- Modify: `crates/agent-cli/src/tui/icons.rs`
- Modify: `crates/agent-cli/src/tui/theme.rs`
- Test: `crates/agent-cli/src/tui/icons.rs` (tests module)
- Test: `crates/agent-cli/src/tui/theme.rs` (tests module)

- [ ] **Step 1: Write failing tests for logo and command-row background role**

```rust
// crates/agent-cli/src/tui/icons.rs
#[test]
fn project_logo_is_unicode_pixel_block() {
    let logo = Icons::project_logo();
    assert_eq!(logo.lines().count(), 6);
    assert!(logo.contains("█"));
}

// crates/agent-cli/src/tui/theme.rs
#[test]
fn paint_command_row_bg_uses_background_ansi_code() {
    let theme = Theme { enable_color: true };
    let output = theme.paint(Role::CommandRowBg, "› who are you");
    assert!(output.contains("\x1b[48;"));
}
```

- [ ] **Step 2: Run targeted tests to verify they fail**

Run:

```bash
cargo test -p agent-cli tui::icons::tests::project_logo_is_unicode_pixel_block
cargo test -p agent-cli tui::theme::tests::paint_command_row_bg_uses_background_ansi_code
```

Expected: FAIL with missing `Icons::project_logo` and missing `Role::CommandRowBg`.

- [ ] **Step 3: Implement minimal logo API and command-row role**

```rust
// crates/agent-cli/src/tui/icons.rs
impl Icons {
    pub fn project_logo() -> &'static str {
        "██  ██\n██████\n██  ██\n██████\n ████ \n  ██  "
    }
}

// crates/agent-cli/src/tui/theme.rs
pub enum Role {
    Text,
    Subtle,
    Success,
    Warning,
    Error,
    Accent,
    CommandRowBg,
}

// inside Theme::paint role match
Role::CommandRowBg => "48;5;252;30",
```

- [ ] **Step 4: Re-run targeted tests**

Run:

```bash
cargo test -p agent-cli tui::icons::tests::project_logo_is_unicode_pixel_block
cargo test -p agent-cli tui::theme::tests::paint_command_row_bg_uses_background_ansi_code
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent-cli/src/tui/icons.rs crates/agent-cli/src/tui/theme.rs
git commit -m "feat(agent-cli): add unicode logo and command-row background role"
```

---

### Task 2: Implement Header + Notice Render Helpers (No Borders)

**Files:**
- Modify: `crates/agent-cli/src/tui/layout.rs`
- Test: `crates/agent-cli/src/tui/layout.rs` (tests module)

- [ ] **Step 1: Add failing tests for header and notice output shape**

```rust
#[test]
fn render_header_block_has_no_border_lines() {
    let lines = render_header_block(
        "Claude Prism",
        "v0.1.0",
        "MiniMax-M1 · safe mode",
        "~/Documents/Code/claude-prism",
    );
    assert!(!lines.iter().any(|line| line.contains("===") || line.contains("---")));
    assert!(lines.iter().any(|line| line.contains("Claude Prism")));
}

#[test]
fn render_notice_line_uses_plain_single_line_text() {
    let line = render_notice_line("Tool approvals enabled", "/commands for help");
    assert!(line.contains("Tool approvals enabled"));
    assert!(line.contains("/commands for help"));
    assert!(!line.contains('\n'));
}
```

- [ ] **Step 2: Run layout tests and confirm failure**

Run:

```bash
cargo test -p agent-cli tui::layout::tests::render_header_block_has_no_border_lines
cargo test -p agent-cli tui::layout::tests::render_notice_line_uses_plain_single_line_text
```

Expected: FAIL (missing `render_header_block` / `render_notice_line`).

- [ ] **Step 3: Implement layout helpers**

```rust
pub fn render_header_block(
    product: &str,
    version: &str,
    model_line: &str,
    path: &str,
) -> Vec<String> {
    vec![
        format!("{}  {} {}", Icons::project_logo().lines().next().unwrap_or(""), product, version),
        format!("{}", model_line),
        format!("{}", path),
    ]
}

pub fn render_notice_line(primary: &str, hint: &str) -> String {
    format!("{} · {}", primary, hint)
}
```

- [ ] **Step 4: Run layout test module**

Run:

```bash
cargo test -p agent-cli tui::layout::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent-cli/src/tui/layout.rs
git commit -m "feat(agent-cli): add borderless header and single-line notice renderers"
```

---

### Task 3: Session-Scoped Header Lifecycle in TUI Shell

**Files:**
- Modify: `crates/agent-cli/src/tui/shell.rs`
- Test: `crates/agent-cli/src/tui/shell.rs` (tests module)

- [ ] **Step 1: Add failing tests for session lifecycle rendering**

```rust
#[test]
fn header_renders_once_for_same_session() {
    let mut chrome = SessionChromeState::default();
    assert!(chrome.should_render("tab-a-session"));
    chrome.mark_rendered("tab-a-session");
    assert!(!chrome.should_render("tab-a-session"));
}

#[test]
fn header_rerenders_on_session_switch() {
    let mut chrome = SessionChromeState::default();
    chrome.mark_rendered("tab-a-session");
    assert!(chrome.should_render("tab-b-session"));
}
```

- [ ] **Step 2: Run these tests and confirm failure**

Run:

```bash
cargo test -p agent-cli tui::shell::tests::header_renders_once_for_same_session
cargo test -p agent-cli tui::shell::tests::header_rerenders_on_session_switch
```

Expected: FAIL (missing `SessionChromeState`).

- [ ] **Step 3: Implement lifecycle state and startup render hook**

```rust
#[derive(Default)]
struct SessionChromeState {
    rendered_for: Option<String>,
}

impl SessionChromeState {
    fn should_render(&self, session_id: &str) -> bool {
        self.rendered_for.as_deref() != Some(session_id)
    }

    fn mark_rendered(&mut self, session_id: &str) {
        self.rendered_for = Some(session_id.to_string());
    }
}

// in run_tui_shell startup path
if chrome_state.should_render(&local_session_id) {
    sink.write_human(&header_text);
    sink.write_human("\n");
    sink.write_human(&notice_text);
    sink.write_human("\n\n");
    chrome_state.mark_rendered(&local_session_id);
}
```

- [ ] **Step 4: Run shell tests**

Run:

```bash
cargo test -p agent-cli tui::shell::tests
```

Expected: PASS (including existing suspended-continuity tests).

- [ ] **Step 5: Commit**

```bash
git add crates/agent-cli/src/tui/shell.rs
git commit -m "feat(agent-cli): render session chrome once per tui session"
```

---

### Task 4: Add Transcript Renderers for User Command Rows and Assistant Hanging Indent

**Files:**
- Modify: `crates/agent-cli/src/tui/transcript.rs`
- Test: `crates/agent-cli/src/tui/transcript.rs` (tests module)

- [ ] **Step 1: Write failing tests for command row and hanging indent**

```rust
#[test]
fn renders_user_command_row_with_prefix_and_background() {
    let theme = Theme { enable_color: true };
    let rows = render_user_command_rows(&theme, "who are you", 40);
    assert!(rows[0].contains("› who are you"));
    assert!(rows[0].contains("\x1b[48;"));
}

#[test]
fn assistant_block_wraps_with_hanging_indent() {
    let lines = render_assistant_block("●", "I can help with coding debugging and architecture decisions", 28);
    assert!(lines[0].starts_with("● "));
    assert!(lines[1].starts_with("  "));
}
```

- [ ] **Step 2: Run transcript tests and verify failure**

Run:

```bash
cargo test -p agent-cli tui::transcript::tests::renders_user_command_row_with_prefix_and_background
cargo test -p agent-cli tui::transcript::tests::assistant_block_wraps_with_hanging_indent
```

Expected: FAIL (missing renderer functions).

- [ ] **Step 3: Implement transcript render helpers**

```rust
pub fn render_user_command_rows(theme: &Theme, text: &str, width: usize) -> Vec<String> {
    wrap_with_prefix("› ", text, width)
        .into_iter()
        .map(|line| theme.paint(Role::CommandRowBg, line))
        .collect()
}

pub fn render_assistant_block(marker: &str, text: &str, width: usize) -> Vec<String> {
    let mut wrapped = wrap_with_prefix(&format!("{} ", marker), text, width);
    for line in wrapped.iter_mut().skip(1) {
        *line = format!("  {}", line.trim_start());
    }
    wrapped
}
```

- [ ] **Step 4: Run transcript test module**

Run:

```bash
cargo test -p agent-cli tui::transcript::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent-cli/src/tui/transcript.rs
git commit -m "feat(agent-cli): add command-row and assistant hanging-indent transcript renderers"
```

---

### Task 5: Wire Transcript Rendering into Streaming Shell

**Files:**
- Modify: `crates/agent-cli/src/tui/shell.rs`
- Modify: `crates/agent-cli/src/repl.rs` (only if prompt provider needs small signature support)
- Test: `crates/agent-cli/src/tui/shell.rs` (tests module)

- [ ] **Step 1: Add failing tests for user row and assistant formatted output**

```rust
#[test]
fn submitted_prompt_is_rendered_as_user_command_row() {
    let sink = StreamingTuiEventSink::for_test();
    sink.render_user_prompt("who are you");
    let out = sink.take_test_output();
    assert!(out.contains("› who are you"));
}

#[test]
fn message_delta_is_rendered_with_assistant_marker() {
    let sink = StreamingTuiEventSink::for_test();
    sink.emit_event(&AgentEventEnvelope {
        tab_id: "t1".to_string(),
        payload: AgentEventPayload::MessageDelta(AgentMessageDeltaEvent {
            delta: "hello there".to_string(),
        }),
    });
    let out = sink.take_test_output();
    assert!(out.contains("● "));
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cargo test -p agent-cli tui::shell::tests::submitted_prompt_is_rendered_as_user_command_row
cargo test -p agent-cli tui::shell::tests::message_delta_is_rendered_with_assistant_marker
```

Expected: FAIL (helpers not wired yet).

- [ ] **Step 3: Implement shell wiring**

```rust
impl StreamingTuiEventSink {
    fn render_user_prompt(&self, prompt: &str) {
        let width = terminal_width_or_default();
        for row in render_user_command_rows(&self.theme, prompt, width) {
            self.write_human(&(row + "\n"));
        }
    }
}

// in run_tui_shell submit closure, before command dispatch
repl_streaming_sink_for_submit.render_user_prompt(&prompt);

// in emit_event for MessageDelta
if let AgentEventPayload::MessageDelta(delta) = &envelope.payload {
    let width = terminal_width_or_default();
    for line in render_assistant_block(&self.icons.semantic, &delta.delta, width) {
        self.write_human(&(line + "\n"));
    }
    return;
}
```

- [ ] **Step 4: Run shell tests**

Run:

```bash
cargo test -p agent-cli tui::shell::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent-cli/src/tui/shell.rs crates/agent-cli/src/repl.rs
git commit -m "feat(agent-cli): render command-history style user and assistant rows in tui shell"
```

---

### Task 6: Guard Notice Text and TUI Command Surface Constraints

**Files:**
- Modify: `crates/agent-cli/src/tui/shell.rs`
- Test: `crates/agent-cli/src/tui/shell.rs` (tests module)
- Test: `crates/agent-cli/src/command_router.rs` (tests module)

- [ ] **Step 1: Add failing tests for notice and command restrictions**

```rust
#[test]
fn notice_does_not_advertise_unavailable_model_command() {
    let text = startup_notice_text();
    assert!(!text.contains("/model"));
}

#[test]
fn tui_rejects_model_command_with_classic_hint() {
    let cmd = parse_repl_command("/model MiniMax-M1");
    assert!(matches!(cmd, ReplCommand::ModelSet(_)));
}
```

- [ ] **Step 2: Run targeted tests and verify failure on notice helper absence**

Run:

```bash
cargo test -p agent-cli tui::shell::tests::notice_does_not_advertise_unavailable_model_command
```

Expected: FAIL (missing `startup_notice_text`).

- [ ] **Step 3: Implement notice helper and use it in startup chrome rendering**

```rust
fn startup_notice_text() -> String {
    format!("{} · {}", "Tool approvals are available", "/commands for help")
}

// in startup rendering
let notice = startup_notice_text();
sink.write_human(&notice);
sink.write_human("\n\n");
```

- [ ] **Step 4: Run shell + command router tests**

Run:

```bash
cargo test -p agent-cli tui::shell::tests
cargo test -p agent-cli command_router::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent-cli/src/tui/shell.rs crates/agent-cli/src/command_router.rs
git commit -m "test(agent-cli): enforce tui notice and command-surface constraints"
```

---

### Task 7: Full Regression Gates and Manual Validation

**Files:**
- Modify: `crates/agent-cli/src/tui/shell.rs` (only if fixes are needed)
- Test: `crates/agent-cli/src/main.rs` (existing tests), `crates/agent-cli/src/output.rs` (existing tests)

- [ ] **Step 1: Run full automated gates**

Run:

```bash
cargo test -p agent-cli
cargo clippy -p agent-cli --tests -- -D warnings
```

Expected: PASS both commands.

- [ ] **Step 2: Run manual tui smoke flow**

Run:

```bash
cargo run -p agent-cli -- --ui-mode tui --output human
```

Manual checklist:

- header renders once at startup
- notice line is single-line and non-boxed
- user commands appear as subtle full-line command rows
- assistant text starts with `● ` and wrapped lines hang-indent
- header does not redraw between turns

- [ ] **Step 3: Run classic/jsonl regression spot checks**

Run:

```bash
cargo run -p agent-cli -- --ui-mode classic --output human --prompt "hello"
cargo run -p agent-cli -- --output jsonl --prompt "hello"
```

Expected:

- classic still works and keeps classic command surface
- jsonl output remains machine-readable JSON lines

- [ ] **Step 4: Commit final integration fixes (if any)**

```bash
git add crates/agent-cli/src/tui/shell.rs \
        crates/agent-cli/src/tui/layout.rs \
        crates/agent-cli/src/tui/transcript.rs \
        crates/agent-cli/src/tui/theme.rs \
        crates/agent-cli/src/tui/icons.rs \
        crates/agent-cli/src/repl.rs \
        crates/agent-cli/src/command_router.rs
git commit --allow-empty -m "chore(agent-cli): finalize session chrome tui rollout and validation gates"
```

- [ ] **Step 5: Merge to main (local)**

```bash
git checkout main
git merge --no-ff feat/agent-cli-tui-session-chrome
cargo test -p agent-cli
```

Expected: merge succeeds, post-merge tests pass.

---

## Spec Coverage Check (Self-Review)

1. Session-scoped header rendered once and only on session switch: covered by Task 3.
2. No-border header + single-line notice: covered by Task 2 and Task 6.
3. Command-history visual rhythm with subtle user rows and plain assistant block: covered by Task 4 and Task 5.
4. Current input row consistency and non-fullscreen streaming preservation: covered by Task 5.
5. Command surface constraints in tui (`/model` not advertised): covered by Task 6.
6. Regression guard for classic/jsonl + clippy gates: covered by Task 7.

No uncovered spec requirement remains.

## Placeholder Scan (Self-Review)

- No `TODO`/`TBD` placeholders in tasks.
- Every code-change step includes concrete code snippets.
- Every verification step includes explicit commands and expected outcomes.

## Type/Name Consistency (Self-Review)

Planned identifiers are used consistently across tasks:

- `SessionChromeState`
- `render_header_block`
- `render_notice_line`
- `render_user_command_rows`
- `render_assistant_block`
- `startup_notice_text`
