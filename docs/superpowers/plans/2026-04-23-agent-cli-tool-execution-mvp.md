# Agent CLI Tool Execution MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable `agent-cli` to execute Claude-Code-like safe local tools in REPL (`read_file`, `list_files`, `search_project`, `run_shell_command` with approval) instead of always failing tool turns.

**Architecture:** Keep `agent-core` unchanged and implement a CLI-local tool runtime in `agent-cli`. Route tool calls through a new dispatcher (`local_tools`) and gate shell execution with session approvals using existing `AgentRuntimeState` approval APIs. Preserve existing output modes and command system, adding only minimal REPL commands for shell approval.

**Tech Stack:** Rust 2021, tokio async process/file APIs, agent-core tool/event types, ripgrep (`rg`) for workspace listing/search

---

## Scope Check

This plan covers one subsystem: **`agent-cli` tool execution path**. It does not add edit/write tools, pending-turn resume workflow, or change desktop runtime behavior.

## File Structure

| File | Responsibility |
|---|---|
| `crates/agent-cli/src/args.rs` | Add `ToolMode` parsing and CLI option/env wiring |
| `crates/agent-cli/src/main.rs` | Wire tool mode into request shaping and tool executor closure; add `/approve` command handling |
| `crates/agent-cli/src/command_router.rs` | Parse `/approve shell once|session|deny` command |
| `crates/agent-cli/src/tool_executor.rs` | Dispatch to new local tool runtime instead of hardcoded unsupported error |
| `crates/agent-cli/src/local_tools/mod.rs` | Central dispatcher for supported CLI tools |
| `crates/agent-cli/src/local_tools/common.rs` | Shared helpers: arg parsing, path resolution, truncation, result helpers |
| `crates/agent-cli/src/local_tools/workspace.rs` | `read_file`, `list_files`, `search_project` implementations |
| `crates/agent-cli/src/local_tools/shell.rs` | `run_shell_command` implementation with blocklist/allowlist + approval gate |
| `crates/agent-cli/src/output.rs` | Human output enhancement for approval-required tool results |
| `docs/superpowers/crates-agent-handoff.md` | Document new CLI tool capability and limits |

---

### Task 1: Add Tool Mode and Request Shaping

**Files:**
- Modify: `crates/agent-cli/src/args.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/args.rs`
- Test: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: Write failing tests for `ToolMode` parsing and default behavior**

```rust
#[test]
fn parses_tool_mode_safe_and_off() {
    let args = Args::parse_from(["agent-runtime", "--tool-mode", "safe"]);
    assert_eq!(args.tool_mode.as_deref(), Some("safe"));

    let args = Args::parse_from(["agent-runtime", "--tool-mode", "off"]);
    assert_eq!(args.tool_mode.as_deref(), Some("off"));
}

#[test]
fn parse_tool_mode_rejects_unknown_value() {
    let err = parse_tool_mode("danger").err();
    assert!(err.is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-cli args::tests::parses_tool_mode_safe_and_off -v`  
Expected: FAIL with missing `tool_mode` field and `parse_tool_mode`.

- [ ] **Step 3: Implement `ToolMode` in `args.rs`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    Off,
    Safe,
}

pub fn parse_tool_mode(raw: &str) -> Result<ToolMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(ToolMode::Off),
        "safe" => Ok(ToolMode::Safe),
        other => Err(format!(
            "Unsupported tool mode '{}'. Use 'off' or 'safe'.",
            other
        )),
    }
}

#[derive(Parser, Debug, Clone)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(long)]
    pub api_key: Option<String>,

    #[arg(long)]
    pub provider: Option<String>,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long)]
    pub base_url: Option<String>,

    #[arg(long, default_value = ".")]
    pub project_path: String,

    #[arg(long)]
    pub prompt: Option<String>,

    #[arg(long, default_value = "cli-tab")]
    pub tab_id: String,

    #[arg(long)]
    pub output: Option<String>,

    #[arg(long, env = "AGENT_TOOL_MODE")]
    pub tool_mode: Option<String>,
}
```

- [ ] **Step 4: Update request shaping in `main.rs` to only force suggestion-only when tool mode is off**

```rust
fn build_request(
    project_path: &str,
    tab_id: &str,
    model: &str,
    prompt: String,
    local_session_id: &str,
    tool_mode: args::ToolMode,
) -> AgentTurnDescriptor {
    let mut request = AgentTurnDescriptor {
        project_path: project_path.to_string(),
        prompt,
        tab_id: tab_id.to_string(),
        model: Some(model.to_string()),
        local_session_id: Some(local_session_id.to_string()),
        previous_response_id: None,
        turn_profile: None,
    };

    if tool_mode == args::ToolMode::Off {
        request.turn_profile = Some(AgentTurnProfile {
            task_kind: AgentTaskKind::SuggestionOnly,
            response_mode: AgentResponseMode::SuggestionOnly,
            ..AgentTurnProfile::default()
        });
    }

    request
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli args::tests -v`  
Expected: PASS.

Run: `cargo test -p agent-cli tests::request_requires_tools_for_selection_edit_prompts -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/args.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add tool mode parsing and request shaping switch"
```

---

### Task 2: Implement Workspace Tool Runtime (`read_file`, `list_files`, `search_project`)

**Files:**
- Create: `crates/agent-cli/src/local_tools/mod.rs`
- Create: `crates/agent-cli/src/local_tools/common.rs`
- Create: `crates/agent-cli/src/local_tools/workspace.rs`
- Modify: `crates/agent-cli/src/tool_executor.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/local_tools/workspace.rs`
- Test: `crates/agent-cli/src/tool_executor.rs`

- [ ] **Step 1: Write failing tests for workspace tool behavior**

```rust
#[tokio::test]
async fn read_file_returns_content_for_text_file() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let file = dir.path().join("src.txt");
    tokio::fs::write(&file, "hello").await.unwrap_or_else(|e| panic!("write: {e}"));

    let result = execute_read_file(
        dir.path().to_str().unwrap_or("."),
        "call-1",
        serde_json::json!({"path":"src.txt"}),
        None,
    ).await;

    assert!(!result.is_error);
    assert_eq!(result.content["content"], "hello");
}

#[tokio::test]
async fn read_file_blocks_path_traversal() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let result = execute_read_file(
        dir.path().to_str().unwrap_or("."),
        "call-1",
        serde_json::json!({"path":"../secret.txt"}),
        None,
    ).await;
    assert!(result.is_error);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli local_tools::workspace::tests::read_file_returns_content_for_text_file -v`  
Expected: FAIL because `local_tools` module does not exist.

- [ ] **Step 3: Implement shared helpers in `local_tools/common.rs`**

```rust
pub(crate) fn tool_arg_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("Missing required tool argument '{}'.", key))
}

pub(crate) fn resolve_project_path(project_root: &str, raw_path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(project_root).join(raw_path);
    let normalized = candidate
        .components()
        .fold(PathBuf::new(), |mut acc, comp| {
            match comp {
                Component::ParentDir => {
                    acc.pop();
                }
                Component::CurDir => {}
                _ => acc.push(comp.as_os_str()),
            }
            acc
        });
    if !normalized.starts_with(project_root) {
        return Err(format!("Path escapes project root: {}", raw_path));
    }
    Ok(normalized)
}

pub(crate) fn ok_result(tool_name: &str, call_id: &str, content: Value, preview: String) -> AgentToolResult {
    AgentToolResult {
        tool_name: tool_name.to_string(),
        call_id: call_id.to_string(),
        is_error: false,
        content,
        preview,
    }
}
```

- [ ] **Step 4: Implement workspace tools and dispatch**

```rust
// local_tools/workspace.rs
pub(crate) async fn execute_search_project(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let query = match tool_arg_string(&args, "query") {
        Ok(value) => value,
        Err(message) => return error_result("search_project", call_id, message),
    };
    let path = tool_arg_optional_string(&args, "path").unwrap_or_else(|| ".".to_string());
    let search_root = match resolve_project_path(project_root, &path) {
        Ok(path) => path,
        Err(message) => return error_result("search_project", call_id, message),
    };

    let mut cmd = tokio::process::Command::new("rg");
    cmd.arg("-n").arg("--no-heading").arg("--color").arg("never").arg(&query).arg(search_root);
    let out = match command_output_with_cancel(
        cmd,
        cancel_rx,
        "search_project",
        call_id,
        "Failed to run rg",
    )
    .await
    {
        Ok(output) => output,
        Err(result) => return result,
    };
    if out.status.code().unwrap_or(-1) > 1 {
        return error_result(
            "search_project",
            call_id,
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        );
    }

    let mut matches = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let truncated = matches.len() > 200;
    if truncated {
        matches.truncate(200);
    }

    ok_result(
        "search_project",
        call_id,
        serde_json::json!({
            "query": query,
            "path": path,
            "matches": matches,
            "truncated": truncated,
        }),
        format!("Matches: {}", matches.len()),
    )
}

// tool_executor.rs
pub fn execute_cli_tool(
    runtime_state: Arc<AgentRuntimeState>,
    tab_id: String,
    project_root: String,
    call: AgentToolCall,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> impl Future<Output = AgentToolResult> {
    async move {
        local_tools::execute_tool_call(&runtime_state, &tab_id, &project_root, call, cancel_rx).await
    }
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli local_tools::workspace::tests -v`  
Expected: PASS.

Run: `cargo test -p agent-cli tool_executor::tests -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/local_tools crates/agent-cli/src/tool_executor.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add workspace tool runtime for read/list/search"
```

---

### Task 3: Implement Safe Shell Tool with Approval Gate

**Files:**
- Create: `crates/agent-cli/src/local_tools/shell.rs`
- Modify: `crates/agent-cli/src/local_tools/mod.rs`
- Test: `crates/agent-cli/src/local_tools/shell.rs`

- [ ] **Step 1: Write failing tests for shell safety and approval behavior**

```rust
#[tokio::test]
async fn blocks_dangerous_shell_pattern() {
    let runtime = AgentRuntimeState::default();
    let result = execute_run_shell_command(
        &runtime,
        "tab-1",
        ".",
        "call-1",
        serde_json::json!({"command":"rm -rf /tmp/test"}),
        None,
    ).await;
    assert!(result.is_error);
    assert!(result.preview.contains("blocked for safety"));
}

#[tokio::test]
async fn requires_approval_for_shell() {
    let runtime = AgentRuntimeState::default();
    let result = execute_run_shell_command(
        &runtime,
        "tab-1",
        ".",
        "call-1",
        serde_json::json!({"command":"echo ok"}),
        None,
    ).await;
    assert!(result.is_error);
    assert_eq!(result.content["approvalRequired"], true);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli local_tools::shell::tests::requires_approval_for_shell -v`  
Expected: FAIL because `shell.rs` does not exist.

- [ ] **Step 3: Implement shell execution with blocklist + allowlist + approval check**

```rust
const BLOCKED_SHELL_PATTERNS: &[&str] = &[
    "rm -rf", "sudo ", "mkfs", "dd ", "curl|bash", "curl | bash", "wget|sh", "wget | sh",
];

const ALLOWED_SHELL_COMMANDS: &[&str] = &[
    "rg", "grep", "cat", "head", "tail", "ls", "find", "git", "echo", "wc", "sed", "awk", "python", "python3",
];

pub(crate) async fn execute_run_shell_command(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let command = match tool_arg_string(&args, "command") {
        Ok(value) => value,
        Err(message) => return error_result("run_shell_command", call_id, message),
    };
    if is_blocked_command(&command) {
        return error_result("run_shell_command", call_id, "Command blocked for safety".to_string());
    }

    let approval = runtime_state.check_tool_approval(tab_id, "run_shell_command").await;
    if !approval.allow_session && approval.allow_once_remaining == 0 {
        return approval_required_result(
            "run_shell_command",
            call_id,
            "run_shell_command requires approval before the command can continue.".to_string(),
            args,
        );
    }

    if !is_allowed_command(&command) {
        return approval_required_result(
            "run_shell_command",
            call_id,
            "Command not in safe allowlist; explicit approval required.".to_string(),
            serde_json::json!({"command": command}),
        );
    }

    run_shell_with_timeout(project_root, call_id, command, cancel_rx).await
}
```

- [ ] **Step 4: Add timeout and output truncation helper for shell process**

```rust
async fn run_shell_with_timeout(
    cwd: &str,
    call_id: &str,
    command: String,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(&command).current_dir(cwd);

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        wait_for_command_output(cmd, cancel_rx),
    ).await;

    match output {
        Ok(Ok(out)) => {
            let stdout = truncate_output(&out.stdout, 32_000);
            let stderr = truncate_output(&out.stderr, 32_000);
            ok_result(
                "run_shell_command",
                call_id,
                serde_json::json!({
                    "command": command,
                    "exitCode": out.status.code().unwrap_or(-1),
                    "stdout": stdout.0,
                    "stderr": stderr.0,
                }),
                format!("exit={} stdout={} stderr={}", out.status.code().unwrap_or(-1), stdout.0, stderr.0),
            )
        }
        Ok(Err(err)) => error_result("run_shell_command", call_id, err),
        Err(_) => error_result("run_shell_command", call_id, "Shell command timed out (30s).".to_string()),
    }
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli local_tools::shell::tests -v`  
Expected: PASS.

Run: `cargo test -p agent-cli local_tools::tests::dispatches_shell_tool -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/local_tools/shell.rs crates/agent-cli/src/local_tools/mod.rs
git commit -m "feat(agent-cli): add safe shell tool execution with approval gate"
```

---

### Task 4: Add REPL Approval Commands (`/approve shell once|session|deny`)

**Files:**
- Modify: `crates/agent-cli/src/command_router.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/command_router.rs`

- [ ] **Step 1: Write failing parser tests for `/approve`**

```rust
#[test]
fn parses_approve_shell_commands() {
    assert_eq!(
        parse_repl_command("/approve shell once"),
        ReplCommand::ApproveShellOnce
    );
    assert_eq!(
        parse_repl_command("/approve shell session"),
        ReplCommand::ApproveShellSession
    );
    assert_eq!(
        parse_repl_command("/approve shell deny"),
        ReplCommand::ApproveShellDeny
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-cli command_router::tests::parses_approve_shell_commands -v`  
Expected: FAIL because enum variants are missing.

- [ ] **Step 3: Implement command parser variants**

```rust
pub enum ReplCommand {
    Help,
    Commands,
    Config,
    Status,
    Clear,
    ModelShow,
    ModelSet(String),
    ApproveShellOnce,
    ApproveShellSession,
    ApproveShellDeny,
    Unknown { raw: String, suggestion: Option<&'static str> },
    None,
}

match command {
    "/approve" => {
        let target = parts.next().unwrap_or_default();
        let mode = parts.next().unwrap_or_default();
        match (target, mode) {
            ("shell", "once") => ReplCommand::ApproveShellOnce,
            ("shell", "session") => ReplCommand::ApproveShellSession,
            ("shell", "deny") => ReplCommand::ApproveShellDeny,
            _ => ReplCommand::Unknown {
                raw: trimmed.to_string(),
                suggestion: Some("/approve shell once"),
            },
        }
    }
    "/help" => ReplCommand::Help,
    "/commands" => ReplCommand::Commands,
    "/config" => ReplCommand::Config,
    "/status" => ReplCommand::Status,
    "/clear" => ReplCommand::Clear,
    "/model" => {
        let model = parts.collect::<Vec<_>>().join(" ");
        if model.trim().is_empty() {
            ReplCommand::ModelShow
        } else {
            ReplCommand::ModelSet(model.trim().to_string())
        }
    }
    other => ReplCommand::Unknown {
        raw: other.to_string(),
        suggestion: suggest_command(other),
    },
}
```

- [ ] **Step 4: Handle approvals in REPL loop using existing runtime state APIs**

```rust
match command_router::parse_repl_command(&prompt) {
    command_router::ReplCommand::ApproveShellOnce => {
        let _ = repl_runtime_state
            .set_tool_approval(&repl_args.tab_id, "run_shell_command", "allow_once")
            .await;
        println!("Approved shell for one command in this session.");
        return Box::pin(async { Ok(()) });
    }
    command_router::ReplCommand::ApproveShellSession => {
        let _ = repl_runtime_state
            .set_tool_approval(&repl_args.tab_id, "run_shell_command", "allow_session")
            .await;
        println!("Approved shell for this session.");
        return Box::pin(async { Ok(()) });
    }
    command_router::ReplCommand::ApproveShellDeny => {
        let _ = repl_runtime_state
            .set_tool_approval(&repl_args.tab_id, "run_shell_command", "deny_session")
            .await;
        println!("Denied shell for this session.");
        return Box::pin(async { Ok(()) });
    }
    _ => {}
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p agent-cli command_router::tests -v`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/command_router.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add repl shell approval commands"
```

---

### Task 5: Integrate Real Tool Executor and Improve Human Output

**Files:**
- Modify: `crates/agent-cli/src/tool_executor.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Modify: `crates/agent-cli/src/output.rs`
- Test: `crates/agent-cli/src/output.rs`
- Test: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: Write failing output test for approval-required tool result formatting**

```rust
#[test]
fn human_sink_prints_approval_hint_for_tool_result() {
    let sink = HumanEventSink::for_test();
    sink.emit_event(&AgentEventEnvelope {
        tab_id: "t1".to_string(),
        payload: AgentEventPayload::ToolResult(AgentToolResultEvent {
            tool_name: "run_shell_command".to_string(),
            call_id: "call-1".to_string(),
            is_error: true,
            preview: "run_shell_command requires approval".to_string(),
            content: serde_json::json!({"approvalRequired": true}),
            display: None,
        }),
    });
    let out = sink.take_test_output();
    assert!(out.contains("/approve shell once"));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p agent-cli output::tests::human_sink_prints_approval_hint_for_tool_result -v`  
Expected: FAIL because approval hint is not rendered.

- [ ] **Step 3: Replace fallback executor wiring with real local dispatcher**

```rust
let tool_runtime_state = Arc::clone(&runtime_state);
let tool_tab_id = args.tab_id.clone();
let tool_project_path = args.project_path.clone();
let tool_executor: ToolExecutorFn = Arc::new(move |call, cancel_rx| {
    let runtime_state = Arc::clone(&tool_runtime_state);
    let tab_id = tool_tab_id.clone();
    let project = tool_project_path.clone();
    Box::pin(async move {
        tool_executor::execute_cli_tool(runtime_state, tab_id, project, call, cancel_rx).await
    })
});
```

- [ ] **Step 4: Improve human sink output for approval-required tool results**

```rust
fn render_tool_result(result: &AgentToolResultEvent) -> String {
    if result
        .content
        .get("approvalRequired")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return format!(
            "\n[result] {}\n[hint] run /approve shell once or /approve shell session\n",
            result.preview
        );
    }
    format!("\n[result] {}\n", result.preview)
}
```

- [ ] **Step 5: Run full validation and commit**

Run: `cargo test -p agent-cli -v`  
Expected: PASS.

Run: `cargo build -p agent-cli`  
Expected: PASS.

Run: `cargo clippy -p agent-cli -- -D warnings`  
Expected: PASS.

Commit:

```bash
git add crates/agent-cli/src/main.rs crates/agent-cli/src/tool_executor.rs crates/agent-cli/src/output.rs crates/agent-cli/src/local_tools
git commit -m "feat(agent-cli): enable safe local tool execution in repl"
```

---

### Task 6: Docs + Manual Smoke Validation

**Files:**
- Modify: `docs/superpowers/crates-agent-handoff.md`

- [ ] **Step 1: Update crate handoff docs with new CLI capabilities and limits**

```markdown
## agent-cli Tool Execution MVP (2026-04-23)

Supported tools in standalone CLI:
- read_file
- list_files
- search_project
- run_shell_command (approval-gated)

Shell approval commands:
- /approve shell once
- /approve shell session
- /approve shell deny

Known limits:
- No edit/write tool execution in standalone CLI
- No pending-turn resume command yet
```

- [ ] **Step 2: Run manual smoke checks**

Run:

```bash
cargo run -p agent-cli -- --project-path .
```

Inside REPL, verify:
1. Prompt: `Find where ToolExecutorFn is defined` → model uses `search_project` and/or `read_file`.
2. Prompt: `Run 'git status --short' and summarize` → first run should request approval.
3. Command: `/approve shell once`
4. Repeat prompt above → shell tool should execute and return result.

Expected: tool events appear in human output; no panic.

- [ ] **Step 3: Commit docs update**

```bash
git add docs/superpowers/crates-agent-handoff.md
git commit -m "docs(agent-cli): document standalone tool execution mvp and shell approvals"
```

---

## Final Verification Checklist

- [ ] `cargo test -p agent-cli -v`
- [ ] `cargo build -p agent-cli`
- [ ] `cargo clippy -p agent-cli -- -D warnings`
- [ ] REPL smoke test confirms tool calls + shell approval loop

## Risk Notes and Guardrails

1. **No core protocol changes:** keep `agent-core` untouched for MVP speed and risk control.
2. **Unsupported tools remain explicit errors:** prevents silent failure and keeps model behavior debuggable.
3. **Shell remains gated:** default behavior is deny-unless-approved with command blocklist always enforced.
4. **Path traversal blocked:** `resolve_project_path` must reject `..` escapes before any file/shell operation.
