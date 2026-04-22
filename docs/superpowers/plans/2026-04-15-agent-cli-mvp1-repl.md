# Agent CLI MVP-1 REPL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a CLI-first MVP-1 runtime where `agent-runtime` supports multi-turn REPL (single-line enter-to-send), default human-readable streaming output, and preserved `--output jsonl` compatibility.

**Architecture:** Keep `agent-core` unchanged for MVP-1 and implement a modular shell in `agent-cli` (`args`, `output`, `turn_runner`, `repl`). The runtime keeps existing single-turn `--prompt` behavior while defaulting to REPL when no prompt is provided. Chat-completions providers (`minimax`/`deepseek`) remain the supported interactive path.

**Tech Stack:** Rust 2021, tokio, clap, serde/serde_json, agent-core providers (chat_completions), stdio-based event sinks

---

## Scope Check

This plan targets one subsystem only: `agent-cli` interactive runtime UX (MVP-1). It intentionally excludes tool execution backend, approval/resume commands, and session persistence.

## File Structure

| File | Responsibility |
|---|---|
| `crates/agent-cli/src/args.rs` | CLI argument model, output mode parsing, run-mode detection (`single-turn` vs `repl`) |
| `crates/agent-cli/src/output.rs` | `EventSink` implementations for human-readable output and JSONL output |
| `crates/agent-cli/src/turn_runner.rs` | One-turn orchestration for chat-completions providers and in-memory history threading |
| `crates/agent-cli/src/repl.rs` | Single-line REPL loop (`exit`/`quit`), input classification, loop driver |
| `crates/agent-cli/src/main.rs` | Thin composition root: parse args, construct sink/config/state, dispatch single-turn or repl |
| `crates/agent-cli/src/tool_executor.rs` | Keep fallback executor unchanged for MVP-1 (tools still unsupported) |

---

### Task 1: Add CLI Args Module and Run-Mode Semantics

**Files:**
- Create: `crates/agent-cli/src/args.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/args.rs`

- [ ] **Step 1: Write failing tests for run-mode and output parsing**

```rust
#[cfg(test)]
mod tests {
    use super::{parse_output_mode, Args, OutputMode, RunMode};
    use clap::Parser;

    #[test]
    fn detects_single_turn_when_prompt_is_present() {
        let args = Args::parse_from([
            "agent-runtime",
            "--api-key",
            "k",
            "--project-path",
            ".",
            "--model",
            "MiniMax-M1",
            "--prompt",
            "hello",
        ]);
        assert_eq!(args.run_mode(), RunMode::SingleTurn);
    }

    #[test]
    fn detects_repl_when_prompt_is_absent() {
        let args = Args::parse_from([
            "agent-runtime",
            "--api-key",
            "k",
            "--project-path",
            ".",
            "--model",
            "MiniMax-M1",
        ]);
        assert_eq!(args.run_mode(), RunMode::Repl);
    }

    #[test]
    fn parses_output_mode_human_and_jsonl() {
        assert_eq!(parse_output_mode("human").unwrap(), OutputMode::Human);
        assert_eq!(parse_output_mode("jsonl").unwrap(), OutputMode::Jsonl);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p agent-cli args::tests::detects_repl_when_prompt_is_absent -v`
Expected: FAIL with unresolved module/items in `args`.

- [ ] **Step 3: Implement `args.rs` with explicit run-mode behavior**

```rust
use clap::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    SingleTurn,
    Repl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Jsonl,
}

pub fn parse_output_mode(raw: &str) -> Result<OutputMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "human" => Ok(OutputMode::Human),
        "jsonl" => Ok(OutputMode::Jsonl),
        other => Err(format!("Unsupported output mode '{}'. Use 'human' or 'jsonl'.", other)),
    }
}

#[derive(Parser, Debug, Clone)]
#[command(name = "agent-runtime", version)]
pub struct Args {
    #[arg(long, env = "AGENT_API_KEY")]
    pub api_key: String,

    #[arg(long, env = "AGENT_PROVIDER", default_value = "minimax")]
    pub provider: String,

    #[arg(long, env = "AGENT_MODEL")]
    pub model: String,

    #[arg(long, env = "AGENT_BASE_URL")]
    pub base_url: Option<String>,

    #[arg(long)]
    pub project_path: String,

    #[arg(long)]
    pub prompt: Option<String>,

    #[arg(long, default_value = "cli-tab")]
    pub tab_id: String,

    #[arg(long, default_value = "human")]
    pub output: String,
}

impl Args {
    pub fn run_mode(&self) -> RunMode {
        if self.prompt.as_deref().is_some_and(|p| !p.trim().is_empty()) {
            RunMode::SingleTurn
        } else {
            RunMode::Repl
        }
    }
}
```

- [ ] **Step 4: Wire `main.rs` to import args module (compilation-only wiring)**

```rust
mod args;
mod tool_executor;

use args::{Args, RunMode};
use clap::Parser;
```

- [ ] **Step 5: Run tests and crate build**

Run: `cargo test -p agent-cli args::tests -v`
Expected: PASS.

Run: `cargo build -p agent-cli`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/args.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add run-mode aware args module for single-turn and repl"
```

---

### Task 2: Add Human and JSONL Event Sinks

**Files:**
- Create: `crates/agent-cli/src/output.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/output.rs`

- [ ] **Step 1: Write failing sink rendering tests**

```rust
#[cfg(test)]
mod tests {
    use super::{HumanEventSink, JsonlEventSink};
    use agent_core::{
        AgentCompletePayload, AgentEventEnvelope, AgentEventPayload, AgentMessageDeltaEvent,
        AgentStatusEvent, EventSink,
    };

    #[test]
    fn human_sink_formats_status_and_delta() {
        let sink = HumanEventSink::for_test();
        sink.emit_event(&AgentEventEnvelope {
            tab_id: "t1".to_string(),
            payload: AgentEventPayload::Status(AgentStatusEvent {
                stage: "thinking".to_string(),
                message: "Planning".to_string(),
            }),
        });
        sink.emit_event(&AgentEventEnvelope {
            tab_id: "t1".to_string(),
            payload: AgentEventPayload::MessageDelta(AgentMessageDeltaEvent {
                delta: "hello".to_string(),
            }),
        });

        let out = sink.take_test_output();
        assert!(out.contains("[thinking] Planning"));
        assert!(out.contains("hello"));
        assert!(!out.contains("\"payload\""));
    }

    #[test]
    fn jsonl_sink_writes_serialized_complete_payload() {
        let sink = JsonlEventSink::for_test();
        sink.emit_complete(&AgentCompletePayload {
            tab_id: "t1".to_string(),
            outcome: "completed".to_string(),
        });
        let out = sink.take_test_output();
        assert!(out.contains("\"outcome\":\"completed\""));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p agent-cli output::tests::human_sink_formats_status_and_delta -v`
Expected: FAIL because `output` module does not exist.

- [ ] **Step 3: Implement `output.rs` with two sink implementations**

```rust
use std::io::{self, Write};
use std::sync::Mutex;

use agent_core::{
    AgentCompletePayload, AgentErrorEvent, AgentEventEnvelope, AgentEventPayload,
    AgentToolCallEvent, AgentToolResultEvent, EventSink,
};

fn write_line<W: Write>(writer: &mut W, line: &str) {
    let _ = writer.write_all(line.as_bytes());
    let _ = writer.flush();
}

pub struct JsonlEventSink {
    writer: Mutex<Vec<u8>>,
    mirror_stdout: bool,
}

impl JsonlEventSink {
    pub fn stdout() -> Self {
        Self {
            writer: Mutex::new(Vec::new()),
            mirror_stdout: true,
        }
    }

    pub fn for_test() -> Self {
        Self {
            writer: Mutex::new(Vec::new()),
            mirror_stdout: false,
        }
    }

    pub fn take_test_output(&self) -> String {
        let mut guard = self.writer.lock().expect("jsonl buffer poisoned");
        let out = String::from_utf8_lossy(&guard).to_string();
        guard.clear();
        out
    }

    fn emit_json<T: serde::Serialize>(&self, value: &T) {
        if let Ok(json) = serde_json::to_string(value) {
            if let Ok(mut guard) = self.writer.lock() {
                write_line(&mut *guard, &(json.clone() + "\n"));
            }
            if self.mirror_stdout {
                let stdout = io::stdout();
                let mut handle = stdout.lock();
                write_line(&mut handle, &(json + "\n"));
            }
        }
    }
}

impl EventSink for JsonlEventSink {
    fn emit_event(&self, envelope: &AgentEventEnvelope) {
        self.emit_json(envelope);
    }

    fn emit_complete(&self, payload: &AgentCompletePayload) {
        self.emit_json(payload);
    }
}

pub struct HumanEventSink {
    writer: Mutex<Vec<u8>>,
    mirror_stdout: bool,
}

impl HumanEventSink {
    pub fn stdout() -> Self {
        Self {
            writer: Mutex::new(Vec::new()),
            mirror_stdout: true,
        }
    }

    pub fn for_test() -> Self {
        Self {
            writer: Mutex::new(Vec::new()),
            mirror_stdout: false,
        }
    }

    pub fn take_test_output(&self) -> String {
        let mut guard = self.writer.lock().expect("human buffer poisoned");
        let out = String::from_utf8_lossy(&guard).to_string();
        guard.clear();
        out
    }

    fn write_human(&self, line: &str) {
        if let Ok(mut guard) = self.writer.lock() {
            write_line(&mut *guard, line);
        }
        if self.mirror_stdout {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            write_line(&mut handle, line);
        }
    }

    fn render_tool_call(call: &AgentToolCallEvent) -> String {
        format!("\n[tool] {} ({})\n", call.tool_name, call.call_id)
    }

    fn render_tool_result(result: &AgentToolResultEvent) -> String {
        format!("\n[result] {}\n", result.preview)
    }

    fn render_error(error: &AgentErrorEvent) -> String {
        format!("\n[error:{}] {}\n", error.code, error.message)
    }
}

impl EventSink for HumanEventSink {
    fn emit_event(&self, envelope: &AgentEventEnvelope) {
        let line = match &envelope.payload {
            AgentEventPayload::Status(status) => format!("\n[{}] {}\n", status.stage, status.message),
            AgentEventPayload::MessageDelta(delta) => delta.delta.clone(),
            AgentEventPayload::ToolCall(call) => Self::render_tool_call(call),
            AgentEventPayload::ToolResult(result) => Self::render_tool_result(result),
            AgentEventPayload::Error(error) => Self::render_error(error),
            _ => String::new(),
        };
        if !line.is_empty() {
            self.write_human(&line);
        }
    }

    fn emit_complete(&self, payload: &AgentCompletePayload) {
        self.write_human(&format!("\n[turn:{}]\n", payload.outcome));
    }
}
```

- [ ] **Step 4: Wire sink selection in `main.rs`**

```rust
mod output;

use output::{HumanEventSink, JsonlEventSink};

let sink: std::sync::Arc<dyn EventSink> = match args::parse_output_mode(&args.output)? {
    args::OutputMode::Human => std::sync::Arc::new(HumanEventSink::stdout()),
    args::OutputMode::Jsonl => std::sync::Arc::new(JsonlEventSink::stdout()),
};
```

- [ ] **Step 5: Run output tests**

Run: `cargo test -p agent-cli output::tests -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/output.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add human and jsonl event sink implementations"
```

---

### Task 3: Add Turn Runner for Chat-Completions Multi-Turn Context

**Files:**
- Create: `crates/agent-cli/src/turn_runner.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/turn_runner.rs`

- [ ] **Step 1: Write failing tests for provider validation and request policy**

```rust
#[cfg(test)]
mod tests {
    use super::{is_chat_completions_provider, request_requires_tools};
    use agent_core::AgentTurnDescriptor;

    #[test]
    fn accepts_minimax_and_deepseek_only() {
        assert!(is_chat_completions_provider("minimax"));
        assert!(is_chat_completions_provider("deepseek"));
        assert!(!is_chat_completions_provider("openai"));
    }

    #[test]
    fn detects_tool_required_requests() {
        let req = AgentTurnDescriptor {
            project_path: ".".to_string(),
            prompt: "[Selection: @src/main.rs:1:1-1:2]\nedit this".to_string(),
            tab_id: "t1".to_string(),
            model: None,
            local_session_id: None,
            previous_response_id: None,
            turn_profile: None,
        };
        assert!(request_requires_tools(&req));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p agent-cli turn_runner::tests::accepts_minimax_and_deepseek_only -v`
Expected: FAIL because `turn_runner` module does not exist.

- [ ] **Step 3: Implement turn runner and history threading helpers**

```rust
use std::sync::Arc;

use agent_core::{
    providers, AgentRuntimeState, AgentTurnDescriptor, EventSink, StaticConfigProvider,
    ToolExecutorFn,
};

pub fn is_chat_completions_provider(provider: &str) -> bool {
    matches!(provider, "minimax" | "deepseek")
}

pub fn request_requires_tools(request: &AgentTurnDescriptor) -> bool {
    let profile = agent_core::resolve_turn_profile(request);
    agent_core::tool_choice_for_task(request, &profile) == "required"
}

pub async fn run_turn(
    sink: &dyn EventSink,
    config_provider: &StaticConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    tool_executor: ToolExecutorFn,
) -> Result<providers::AgentTurnOutcome, String> {
    let provider = config_provider.config.provider.trim().to_ascii_lowercase();
    if !is_chat_completions_provider(&provider) {
        return Err(format!(
            "Provider '{}' is not enabled for MVP-1 REPL. Use minimax or deepseek.",
            provider
        ));
    }

    let history = if let Some(local_session_id) = request.local_session_id.as_deref() {
        runtime_state
            .history_for_session(local_session_id)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let outcome = providers::chat_completions::run_turn_loop(
        sink,
        config_provider,
        runtime_state,
        request,
        &history,
        Arc::clone(&tool_executor),
        None,
    )
    .await?;

    if let Some(local_session_id) = request.local_session_id.as_deref() {
        runtime_state
            .append_history(local_session_id, outcome.messages.clone())
            .await;
    }

    Ok(outcome)
}
```

- [ ] **Step 4: Replace direct provider call in `main.rs` with `turn_runner::run_turn`**

```rust
mod turn_runner;

let outcome = turn_runner::run_turn(
    sink.as_ref(),
    &config_provider,
    &runtime_state,
    &request,
    tool_executor.clone(),
).await;
```

- [ ] **Step 5: Run turn-runner tests and build**

Run: `cargo test -p agent-cli turn_runner::tests -v`
Expected: PASS.

Run: `cargo build -p agent-cli`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/turn_runner.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add chat-completions turn runner with in-memory history threading"
```

---

### Task 4: Add Single-Line REPL Loop Module

**Files:**
- Create: `crates/agent-cli/src/repl.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/repl.rs`

- [ ] **Step 1: Write failing tests for input classification**

```rust
#[cfg(test)]
mod tests {
    use super::{classify_input, ReplAction};

    #[test]
    fn classifies_exit_and_quit() {
        assert_eq!(classify_input("exit"), ReplAction::Exit);
        assert_eq!(classify_input("quit"), ReplAction::Exit);
    }

    #[test]
    fn classifies_empty_as_ignore() {
        assert_eq!(classify_input("   "), ReplAction::Ignore);
    }

    #[test]
    fn classifies_normal_text_as_submit() {
        assert_eq!(
            classify_input("hello"),
            ReplAction::Submit("hello".to_string())
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p agent-cli repl::tests::classifies_exit_and_quit -v`
Expected: FAIL because `repl` module does not exist.

- [ ] **Step 3: Implement REPL action parser and async loop driver**

```rust
use std::future::Future;
use std::io::{self, BufRead, Write};
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplAction {
    Submit(String),
    Exit,
    Ignore,
}

pub fn classify_input(line: &str) -> ReplAction {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ReplAction::Ignore;
    }
    if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
        return ReplAction::Exit;
    }
    ReplAction::Submit(trimmed.to_string())
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub async fn run_repl<R, W, H>(
    mut reader: R,
    writer: &mut W,
    mut on_submit: H,
) -> Result<(), String>
where
    R: BufRead,
    W: Write,
    H: for<'a> FnMut(String) -> BoxFuture<'a, Result<(), String>>,
{
    let mut line = String::new();
    loop {
        line.clear();
        write!(writer, "> ").map_err(|e| format!("failed to write prompt: {}", e))?;
        writer
            .flush()
            .map_err(|e| format!("failed to flush prompt: {}", e))?;

        let bytes = reader
            .read_line(&mut line)
            .map_err(|e| format!("failed to read input: {}", e))?;
        if bytes == 0 {
            break;
        }

        match classify_input(&line) {
            ReplAction::Ignore => continue,
            ReplAction::Exit => break,
            ReplAction::Submit(prompt) => on_submit(prompt).await?,
        }
    }
    Ok(())
}

pub fn stdin_reader() -> io::StdinLock<'static> {
    Box::leak(Box::new(io::stdin())).lock()
}
```

- [ ] **Step 4: Add small async loop behavior test using in-memory I/O**

```rust
#[tokio::test]
async fn repl_submits_non_empty_lines_until_exit() {
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    let input = Cursor::new("hello\n\nworld\nexit\n");
    let mut output = Vec::new();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_ref = Arc::clone(&seen);

    super::run_repl(input, &mut output, move |prompt| {
        let seen_ref = Arc::clone(&seen_ref);
        Box::pin(async move {
            seen_ref.lock().unwrap().push(prompt);
            Ok(())
        })
    })
    .await
    .unwrap();

    assert_eq!(seen.lock().unwrap().as_slice(), ["hello", "world"]);
}
```

- [ ] **Step 5: Run repl tests**

Run: `cargo test -p agent-cli repl::tests -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/repl.rs
git commit -m "feat(agent-cli): add single-line repl loop and input classification"
```

---

### Task 5: Compose MVP-1 Runtime in `main.rs`

**Files:**
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: Write failing tests for provider defaults and mode behavior helpers**

```rust
#[cfg(test)]
mod mvp1_tests {
    use super::default_base_url;

    #[test]
    fn default_base_url_prefers_chat_completions_endpoints() {
        assert_eq!(
            default_base_url("minimax"),
            Some("https://api.minimax.chat/v1")
        );
        assert_eq!(
            default_base_url("deepseek"),
            Some("https://api.deepseek.com/v1")
        );
    }
}
```

- [ ] **Step 2: Run test to verify baseline before refactor**

Run: `cargo test -p agent-cli mvp1_tests::default_base_url_prefers_chat_completions_endpoints -v`
Expected: PASS before larger wiring changes.

- [ ] **Step 3: Refactor `main.rs` into composition root with single-turn/repl dispatch**

```rust
mod args;
mod output;
mod repl;
mod tool_executor;
mod turn_runner;

use std::process::ExitCode;
use std::sync::Arc;

use agent_core::{
    emit_agent_complete, emit_error, AgentRuntimeConfig, AgentRuntimeState, AgentTaskKind,
    AgentTurnDescriptor, AgentTurnProfile, EventSink, StaticConfigProvider, ToolExecutorFn,
};
use clap::Parser;

use args::{Args, RunMode};

fn emit_cli_failure(sink: &dyn EventSink, tab_id: &str, code: &str, message: &str) {
    emit_error(sink, tab_id, code, message.to_string());
    emit_agent_complete(sink, tab_id, "error");
}

fn default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "minimax" => Some("https://api.minimax.chat/v1"),
        _ => None,
    }
}

fn completion_outcome(suspended: bool) -> &'static str {
    if suspended {
        "suspended"
    } else {
        "completed"
    }
}

fn build_request(args: &Args, prompt: String, local_session_id: &str) -> AgentTurnDescriptor {
    let mut request = AgentTurnDescriptor {
        project_path: args.project_path.clone(),
        prompt,
        tab_id: args.tab_id.clone(),
        model: Some(args.model.clone()),
        local_session_id: Some(local_session_id.to_string()),
        previous_response_id: None,
        turn_profile: None,
    };

    // MVP-1 keeps fallback tool executor; force suggestion-only to avoid required tool calls.
    request.turn_profile = Some(AgentTurnProfile {
        task_kind: AgentTaskKind::SuggestionOnly,
        response_mode: agent_core::AgentResponseMode::SuggestionOnly,
        ..AgentTurnProfile::default()
    });
    request
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let provider = args.provider.trim().to_ascii_lowercase();

    let Some(default_url) = default_base_url(&provider) else {
        eprintln!("agent-runtime error: unsupported provider '{}'.", provider);
        return ExitCode::FAILURE;
    };

    let output_mode = match args::parse_output_mode(&args.output) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("agent-runtime error: {}", err);
            return ExitCode::FAILURE;
        }
    };

    let sink: Arc<dyn EventSink> = match output_mode {
        args::OutputMode::Human => Arc::new(output::HumanEventSink::stdout()),
        args::OutputMode::Jsonl => Arc::new(output::JsonlEventSink::stdout()),
    };

    let mut config = AgentRuntimeConfig::default_local_agent();
    config.provider = provider;
    config.model = args.model.clone();
    config.api_key = Some(args.api_key.clone());
    config.base_url = args.base_url.clone().unwrap_or_else(|| default_url.to_string());

    let config_provider = Arc::new(StaticConfigProvider {
        config,
        config_dir: std::env::temp_dir().join("agent-runtime"),
    });
    let runtime_state = Arc::new(AgentRuntimeState::default());

    let tool_executor: ToolExecutorFn = Arc::new(|call, _cancel_rx| {
        Box::pin(async move { tool_executor::execute_cli_tool(call) })
    });

    let local_session_id = format!("{}-session", args.tab_id);

    match args.run_mode() {
        RunMode::SingleTurn => {
            let prompt = args.prompt.clone().unwrap_or_default();
            let request = build_request(&args, prompt, &local_session_id);
            match turn_runner::run_turn(
                sink.as_ref(),
                config_provider.as_ref(),
                runtime_state.as_ref(),
                &request,
                Arc::clone(&tool_executor),
            )
            .await
            {
                Ok(outcome) => {
                    emit_agent_complete(sink.as_ref(), &request.tab_id, completion_outcome(outcome.suspended));
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    emit_cli_failure(sink.as_ref(), &args.tab_id, "turn_loop_failed", &error);
                    ExitCode::FAILURE
                }
            }
        }
        RunMode::Repl => {
            let mut stdout = std::io::stdout();
            let reader = repl::stdin_reader();
            let res = repl::run_repl(reader, &mut stdout, |prompt| {
                let request = build_request(&args, prompt, &local_session_id);
                let sink = Arc::clone(&sink);
                let config_provider = Arc::clone(&config_provider);
                let runtime_state = Arc::clone(&runtime_state);
                let tool_executor = Arc::clone(&tool_executor);
                Box::pin(async move {
                    match turn_runner::run_turn(
                        sink.as_ref(),
                        config_provider.as_ref(),
                        runtime_state.as_ref(),
                        &request,
                        tool_executor,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            emit_agent_complete(
                                sink.as_ref(),
                                &request.tab_id,
                                completion_outcome(outcome.suspended),
                            );
                            Ok(())
                        }
                        Err(error) => {
                            emit_cli_failure(
                                sink.as_ref(),
                                &request.tab_id,
                                "turn_loop_failed",
                                &error,
                            );
                            Ok(())
                        }
                    }
                })
            })
            .await;

            if let Err(error) = res {
                emit_cli_failure(sink.as_ref(), &args.tab_id, "repl_failed", &error);
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}
```

- [ ] **Step 4: Run focused tests and full crate tests**

Run: `cargo test -p agent-cli mvp1_tests::default_base_url_prefers_chat_completions_endpoints -v`
Expected: PASS.

Run: `cargo test -p agent-cli -v`
Expected: PASS.

- [ ] **Step 5: Manual smoke test**

Run (REPL):

```bash
AGENT_API_KEY="${AGENT_API_KEY}" cargo run -p agent-cli --bin agent-runtime -- \
  --provider minimax \
  --model MiniMax-M1 \
  --project-path .
```

Expected:
- process enters REPL prompt `> `;
- input one line gets streamed human-readable output;
- `exit` terminates process.

Run (single-turn jsonl):

```bash
AGENT_API_KEY="${AGENT_API_KEY}" cargo run -p agent-cli --bin agent-runtime -- \
  --provider deepseek \
  --model deepseek-chat \
  --project-path . \
  --prompt "Say hello" \
  --output jsonl
```

Expected:
- JSONL events printed;
- process exits after one turn.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): compose mvp1 runtime with repl default and jsonl-compatible single-turn"
```

---

### Task 6: Update Handoff Docs for MVP-1 Reality

**Files:**
- Modify: `docs/superpowers/crates-agent-handoff.md`
- Test: n/a (docs)

- [ ] **Step 1: Add MVP-1 CLI status section**

```md
## agent-cli MVP-1 status

- Supports single-turn (`--prompt`) and REPL (default when prompt absent)
- Supports output modes: `human` (default) and `jsonl`
- Interactive provider path currently targets chat-completions providers (`minimax`, `deepseek`)
- Tool execution remains fallback/unsupported in MVP-1
```

- [ ] **Step 2: Commit docs update**

```bash
git add docs/superpowers/crates-agent-handoff.md
git commit -m "docs(agent-cli): document mvp1 repl and output-mode behavior"
```

---

## Final Verification Gate

Run:

```bash
cargo build -p agent-cli
cargo test -p agent-cli -v
cargo clippy -p agent-cli -- -D warnings
```

Expected:
- all commands PASS.

---

## Self-Review

### 1. Spec coverage

- REPL 多轮对话：Task 4 + Task 5 覆盖。
- 单行回车发送：Task 4 `classify_input` + `run_repl` 覆盖。
- 默认 human 输出 + jsonl 保留：Task 2 + Task 5 覆盖。
- `--prompt` 单轮语义保留：Task 1 + Task 5 覆盖。
- chat-completions（minimax/deepseek）聚焦：Task 3 + Task 5 覆盖。

### 2. Placeholder scan

- 无 `TODO/TBD/implement later` 文本。
- 每个代码步骤都提供了具体代码块。
- 每个测试步骤都提供了可执行命令和预期结果。

### 3. Type consistency

- `Args`/`RunMode`/`OutputMode` 在 `args.rs` 定义并在 `main.rs` 一致引用。
- `turn_runner::run_turn` 签名在任务内一致使用。
- `repl::run_repl` 回调签名在测试和 `main.rs` 调用保持一致。
