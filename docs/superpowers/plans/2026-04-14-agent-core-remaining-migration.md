# Agent-Core Remaining Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the remaining extraction of Tauri-coupled agent runtime logic into `agent-core`, then wire a usable `agent-cli` turn loop with stable verification gates.

**Architecture:** Keep Tauri as a thin adapter (`EventSink` + `ConfigProvider` + command entrypoints), move provider/tool orchestration into `agent-core`, and inject platform-specific tool execution through a closure-based executor boundary. This preserves portability while avoiding a large trait hierarchy refactor.

**Tech Stack:** Rust, Tokio, Reqwest SSE streaming, serde_json, Tauri adapters, Cargo workspace

---

## Current Status (Code Audit Snapshot)

- `agent-core` already owns: `events`, `event_sink`, `session`, `workflows`, `streaming`, `message_builder`, `document_artifacts`, `review_runtime`, `telemetry`, and core `turn_engine` primitives.
- Tauri wrappers already done for several modules: `events.rs`, `provider.rs`, `session.rs`, `workflows/mod.rs`, `document_artifacts.rs`, `review_runtime.rs`, `telemetry.rs`.
- Remaining high-coupling code still in Tauri side:
  - `apps/desktop/src-tauri/src/agent/openai.rs` (935 lines)
  - `apps/desktop/src-tauri/src/agent/chat_completions.rs` (1732 lines)
  - `apps/desktop/src-tauri/src/agent/tools.rs` (1825 lines)
  - `apps/desktop/src-tauri/src/agent/mod.rs` (2412 lines)
  - `apps/desktop/src-tauri/src/agent/turn_engine.rs` (505 lines)
- Verified baseline:
  - `cargo build -p agent-core` passes.
  - `cargo test -p agent-core --lib` passes (30 tests).
  - `cargo build -p agent-cli` passes.
  - `cargo build -p claude-prism-desktop` currently fails in `tectonic` transitive dependency (pre-existing blocker, unrelated to agent extraction logic).
- `agent-cli` still contains TODO placeholder and does not run a real turn loop.

## Scope Check

This plan focuses on one subsystem: **agent runtime extraction and runtime wiring**. It excludes unrelated desktop dependency repair beyond documenting the `tectonic` blocker.

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/agent-core/src/instructions.rs` | Turn-profile resolution + prompt instruction assembly (pure logic) |
| `crates/agent-core/src/tools.rs` | Tool contracts + tool specs + schema adapters + argument parser |
| `crates/agent-core/src/turn_engine.rs` | Provider-agnostic tool execution pipeline and event emission |
| `crates/agent-core/src/providers/mod.rs` | Shared provider runtime types (`AgentTurnOutcome`, executor alias) |
| `crates/agent-core/src/providers/openai.rs` | OpenAI Responses run loop (framework-agnostic) |
| `crates/agent-core/src/providers/chat_completions.rs` | MiniMax/DeepSeek chat-completions run loop |
| `apps/desktop/src-tauri/src/agent/openai.rs` | Thin Tauri wrapper around `agent-core` provider runtime |
| `apps/desktop/src-tauri/src/agent/chat_completions.rs` | Thin Tauri wrapper + smoke-test helpers |
| `apps/desktop/src-tauri/src/agent/mod.rs` | Tauri command entrypoints + provider dispatch only |
| `apps/desktop/src-tauri/src/agent/turn_engine.rs` | Re-export/wrapper for core turn_engine pipeline |
| `apps/desktop/src-tauri/src/agent/tools.rs` | Tool execution implementations only (no schema/contract logic) |
| `crates/agent-cli/src/main.rs` | CLI entrypoint that executes one turn via `agent-core` providers |
| `crates/agent-cli/src/tool_executor.rs` | CLI-side fallback tool executor boundary |

---

### Task 1: Extract Instruction/Policy Logic Into `agent-core`

**Files:**
- Create: `crates/agent-core/src/instructions.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/agent/mod.rs`
- Test: `crates/agent-core/src/instructions.rs`

- [ ] **Step 1: Write failing tests in `instructions.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{AgentSelectionScope, AgentTaskKind, AgentTurnDescriptor};

    fn req(prompt: &str) -> AgentTurnDescriptor {
        AgentTurnDescriptor {
            project_path: "/tmp/project".to_string(),
            prompt: prompt.to_string(),
            tab_id: "tab-1".to_string(),
            model: None,
            local_session_id: None,
            previous_response_id: None,
            turn_profile: None,
        }
    }

    #[test]
    fn resolve_turn_profile_marks_selection_edit_for_explicit_edit_intent() {
        let request = req("[Selection: @main.tex:1:1-1:5]\n请帮我润色这一段");
        let profile = resolve_turn_profile(&request);
        assert_eq!(profile.selection_scope, AgentSelectionScope::SelectedSpan);
        assert_eq!(profile.task_kind, AgentTaskKind::SelectionEdit);
    }

    #[test]
    fn tool_choice_requires_document_tools_for_binary_attachment_without_evidence() {
        let request = req("[Attached resource: @a.pdf]\n[Resource path: attachments/a.pdf]\n总结这篇文献");
        let profile = resolve_turn_profile(&request);
        assert_eq!(tool_choice_for_task(&request, &profile), "required");
    }
}
```

- [ ] **Step 2: Run targeted tests to confirm failure before implementation**

Run: `cargo test -p agent-core instructions::tests::resolve_turn_profile_marks_selection_edit_for_explicit_edit_intent -- --nocapture`
Expected: FAIL (missing module/functions before extraction).

- [ ] **Step 3: Implement `instructions.rs` by moving pure logic from Tauri `mod.rs`**

```rust
pub fn tool_choice_for_task(request: &AgentTurnDescriptor, profile: &AgentTurnProfile) -> &'static str {
    match profile.task_kind {
        AgentTaskKind::SelectionEdit | AgentTaskKind::FileEdit => "required",
        AgentTaskKind::SuggestionOnly => "none",
        AgentTaskKind::Analysis | AgentTaskKind::LiteratureReview | AgentTaskKind::PeerReview
            if request_has_binary_attachment_context(request) => "required",
        _ => "auto",
    }
}

pub fn max_rounds_for_task(profile: &AgentTurnProfile) -> u32 {
    match profile.task_kind {
        AgentTaskKind::SuggestionOnly => 2,
        AgentTaskKind::SelectionEdit => 12,
        AgentTaskKind::LiteratureReview => 30,
        _ => 25,
    }
}

pub fn resolve_turn_profile(request: &AgentTurnDescriptor) -> AgentTurnProfile {
    let mut profile = request.turn_profile.clone().unwrap_or_default();
    if profile.selection_scope == AgentSelectionScope::None && request.prompt.contains("[Selection:") {
        profile.selection_scope = AgentSelectionScope::SelectedSpan;
    }
    if profile.task_kind == AgentTaskKind::General
        && profile.selection_scope == AgentSelectionScope::SelectedSpan
        && (request.prompt.contains("润色") || request.prompt.to_lowercase().contains("edit"))
    {
        profile.task_kind = AgentTaskKind::SelectionEdit;
    }
    profile
}

pub fn build_agent_instructions_with_work_state(
    request: &AgentTurnDescriptor,
    work_state: Option<&AgentSessionWorkState>,
    runtime_config: Option<&AgentRuntimeConfig>,
    memory_context: Option<&str>,
) -> String {
    let profile = resolve_turn_profile(request);
    let mut instructions = String::from("You are an execution-oriented coding and writing agent.\n");
    instructions.push_str(&format!("Task kind: {:?}\n", profile.task_kind));
    if let Some(runtime) = runtime_config {
        instructions.push_str(&format!("Provider: {}\nModel: {}\n", runtime.provider, runtime.model));
    }
    if let Some(mem) = memory_context.filter(|m| !m.trim().is_empty()) {
        instructions.push_str("\n[Memory Context]\n");
        instructions.push_str(mem);
        instructions.push('\n');
    }
    if let Some(work) = work_state.and_then(|w| w.current_objective.as_deref()) {
        instructions.push_str(&format!("Current objective: {}\n", work));
    }
    instructions
}
```

- [ ] **Step 4: Export module in `agent-core` and switch desktop usage to core re-exports**

```rust
// crates/agent-core/src/lib.rs
pub mod instructions;
pub use instructions::{
    build_agent_instructions_with_work_state, max_rounds_for_task,
    resolve_turn_profile, tool_choice_for_task,
};
```

```rust
// apps/desktop/src-tauri/src/agent/mod.rs
pub use agent_core::{
    build_agent_instructions_with_work_state,
    max_rounds_for_task,
    resolve_turn_profile,
    tool_choice_for_task,
};
```

- [ ] **Step 5: Run tests and build**

Run: `cargo test -p agent-core instructions::tests -- --nocapture`
Expected: PASS.

Run: `cargo build -p agent-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/instructions.rs crates/agent-core/src/lib.rs apps/desktop/src-tauri/src/agent/mod.rs
git commit -m "refactor(agent-core): move turn profile and instruction policy logic out of tauri mod"
```

---

### Task 2: Move Tool Schema Builders and Argument Parser Into `agent-core`

**Files:**
- Modify: `crates/agent-core/src/tools.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/agent/tools.rs`
- Modify: `apps/desktop/src-tauri/src/agent/openai.rs`
- Modify: `apps/desktop/src-tauri/src/agent/chat_completions.rs`
- Test: `crates/agent-core/src/tools.rs`

- [ ] **Step 1: Add failing tests for parser and provider schema adaptation**

```rust
#[test]
fn parse_tool_arguments_recovers_json_from_wrapped_text() {
    let raw = "```json\n{\"path\":\"src/main.rs\"}\n```";
    let parsed = parse_tool_arguments(raw).expect("should parse wrapped json");
    assert_eq!(parsed["path"], "src/main.rs");
}

#[test]
fn chat_completions_schema_strips_additional_properties_false() {
    let spec = AgentToolSpec {
        name: "read_file".to_string(),
        description: "Read file".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false
        }),
        contract: tool_contract("read_file"),
    };
    let schema = to_chat_completions_tool_schema(&spec, "deepseek");
    assert!(schema["function"]["parameters"]["additionalProperties"].is_null());
}
```

- [ ] **Step 2: Run tests to verify initial failure**

Run: `cargo test -p agent-core parse_tool_arguments_recovers_json_from_wrapped_text -- --nocapture`
Expected: FAIL if functions are not yet in `agent-core`.

- [ ] **Step 3: Implement schema + parser functions in `agent-core/src/tools.rs`**

```rust
pub fn default_tool_specs(include_writing_tools: bool) -> Vec<AgentToolSpec> {
    let mut specs = vec![
        AgentToolSpec {
            name: "read_file".to_string(),
            description: "Read a UTF-8 text file from project root.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
                "additionalProperties": false
            }),
            contract: tool_contract("read_file"),
        },
        AgentToolSpec {
            name: "run_shell_command".to_string(),
            description: "Run a shell command in project root.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "command": { "type": "string" } },
                "required": ["command"],
                "additionalProperties": false
            }),
            contract: tool_contract("run_shell_command"),
        },
    ];
    if include_writing_tools {
        specs.push(AgentToolSpec {
            name: "draft_section".to_string(),
            description: "Draft an academic section from key points.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "section_type": { "type": "string" },
                    "key_points": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["section_type", "key_points"],
                "additionalProperties": false
            }),
            contract: tool_contract("draft_section"),
        });
    }
    specs
}

pub fn to_openai_tool_schema(spec: &AgentToolSpec) -> Value {
    serde_json::json!({
        "type": "function",
        "name": spec.name,
        "description": spec.description,
        "parameters": spec.input_schema
    })
}

pub fn to_chat_completions_tool_schema(spec: &AgentToolSpec, provider: &str) -> Value {
    let mut params = spec.input_schema.clone();
    if matches!(provider, "minimax" | "deepseek") {
        strip_additional_properties_false(&mut params);
    }
    serde_json::json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": params
        }
    })
}

pub fn parse_tool_arguments(raw: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(raw)
        .or_else(|_| serde_json::from_str(extract_first_json_block(raw).unwrap_or(raw)))
}
```

- [ ] **Step 4: Remove duplicate implementations from desktop `tools.rs` and re-export core functions**

```rust
pub use agent_core::tools::{
    default_tool_specs, parse_tool_arguments,
    to_chat_completions_tool_schema, to_openai_tool_schema,
};
```

- [ ] **Step 5: Run tests and builds**

Run: `cargo test -p agent-core tools:: -- --nocapture`
Expected: PASS.

Run: `cargo build -p agent-core && cargo build -p agent-cli`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/tools.rs crates/agent-core/src/lib.rs apps/desktop/src-tauri/src/agent/tools.rs apps/desktop/src-tauri/src/agent/openai.rs apps/desktop/src-tauri/src/agent/chat_completions.rs
git commit -m "refactor(agent-core): centralize tool specs/schema adapters and argument parser"
```

---

### Task 3: Move `execute_tool_calls` Pipeline Into `agent-core` (Closure Injection)

**Files:**
- Modify: `crates/agent-core/src/turn_engine.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/agent/turn_engine.rs`
- Modify: `apps/desktop/src-tauri/src/agent/openai.rs`
- Modify: `apps/desktop/src-tauri/src/agent/chat_completions.rs`
- Test: `crates/agent-core/src/turn_engine.rs`

- [ ] **Step 1: Add failing tests for pipeline suspension behavior**

```rust
#[tokio::test]
async fn execute_tool_calls_stops_after_first_approval_required_result() {
    use std::sync::Arc;
    use crate::event_sink::NullEventSink;

    let sink = NullEventSink;
    let state = AgentRuntimeState::new();
    let request = AgentTurnDescriptor {
        project_path: "/tmp/project".to_string(),
        prompt: "edit".to_string(),
        tab_id: "tab-1".to_string(),
        model: None,
        local_session_id: None,
        previous_response_id: None,
        turn_profile: None,
    };

    let calls = vec![
        AgentToolCall { tool_name: "write_file".to_string(), call_id: "c1".to_string(), arguments: "{}".to_string() },
        AgentToolCall { tool_name: "read_file".to_string(), call_id: "c2".to_string(), arguments: "{}".to_string() },
    ];

    let exec = Arc::new(|_state: &AgentRuntimeState, _tab: &str, _root: &str, call: AgentToolCall, _cancel: Option<tokio::sync::watch::Receiver<bool>>| {
        Box::pin(async move {
            if call.call_id == "c1" {
                AgentToolResult {
                    tool_name: call.tool_name,
                    call_id: call.call_id,
                    is_error: false,
                    preview: "approval required".to_string(),
                    content: serde_json::json!({"approvalRequired": true, "approvalToolName": "write_file"}),
                }
            } else {
                AgentToolResult {
                    tool_name: call.tool_name,
                    call_id: call.call_id,
                    is_error: false,
                    preview: "ok".to_string(),
                    content: serde_json::json!({"ok": true}),
                }
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = AgentToolResult> + Send>>
    });

    let batch = execute_tool_calls(&sink, &state, &request, calls, None, &exec).await;
    assert!(batch.suspended);
    assert_eq!(batch.executed.len(), 1);
}
```

- [ ] **Step 2: Run targeted failing test**

Run: `cargo test -p agent-core execute_tool_calls_stops_after_first_approval_required_result -- --nocapture`
Expected: FAIL before pipeline is moved to core.

- [ ] **Step 3: Implement executor-injected orchestration in core turn engine**

```rust
pub type ToolExecutorFn = std::sync::Arc<
    dyn Fn(
            &AgentRuntimeState,
            &str,
            &str,
            AgentToolCall,
            Option<watch::Receiver<bool>>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentToolResult> + Send>>
        + Send
        + Sync,
>;

pub async fn execute_tool_calls(
    sink: &dyn EventSink,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    tool_calls: Vec<AgentToolCall>,
    cancel_rx: Option<watch::Receiver<bool>>,
    execute_tool: &ToolExecutorFn,
) -> ExecutedToolBatch {
    let mut executed = Vec::new();
    let mut suspended = false;
    for call in tool_calls {
        let result = execute_tool(
            runtime_state,
            &request.tab_id,
            &request.project_path,
            call,
            cancel_rx.clone(),
        )
        .await;
        let approval_required = tool_result_requires_approval(&result);
        emit_tool_result(
            sink,
            &request.tab_id,
            &result.tool_name,
            &result.call_id,
            result.is_error,
            result.preview.clone(),
            result.content.clone(),
            tool_result_display_value(&result),
        );
        executed.push(ExecutedToolCall { result });
        if approval_required {
            suspended = true;
            break;
        }
    }
    ExecutedToolBatch { executed, suspended }
}
```

- [ ] **Step 4: Replace desktop implementation with wrapper call into core**

```rust
pub async fn execute_tool_calls(
    sink: &dyn EventSink,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    tool_calls: Vec<AgentToolCall>,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> ExecutedToolBatch {
    let exec: agent_core::turn_engine::ToolExecutorFn = std::sync::Arc::new(
        |runtime_state, tab_id, project_root, call, cancel_rx| {
            Box::pin(super::tools::execute_tool_call(
                runtime_state,
                tab_id,
                project_root,
                call,
                cancel_rx,
            ))
        },
    );
    agent_core::turn_engine::execute_tool_calls(
        sink,
        runtime_state,
        request,
        tool_calls,
        cancel_rx,
        &exec,
    )
    .await
}
```

- [ ] **Step 5: Run verification tests/builds**

Run: `cargo test -p agent-core turn_engine:: -- --nocapture`
Expected: PASS.

Run: `cargo build -p agent-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/turn_engine.rs crates/agent-core/src/lib.rs apps/desktop/src-tauri/src/agent/turn_engine.rs apps/desktop/src-tauri/src/agent/openai.rs apps/desktop/src-tauri/src/agent/chat_completions.rs
git commit -m "refactor(agent-core): move tool orchestration pipeline behind injected tool executor"
```

---

### Task 4: Migrate OpenAI Run Loop Into `agent-core`

**Files:**
- Create: `crates/agent-core/src/providers/mod.rs`
- Create: `crates/agent-core/src/providers/openai.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/agent/openai.rs`
- Modify: `apps/desktop/src-tauri/src/agent/mod.rs`
- Test: `crates/agent-core/src/providers/openai.rs`

- [ ] **Step 1: Add failing tests for runtime-provider validation**

```rust
#[test]
fn openai_runtime_rejects_non_openai_provider() {
    use crate::{AgentRuntimeConfig, AgentDomainConfig, AgentSamplingProfilesConfig, AgentSamplingConfig};

    let cfg = AgentRuntimeConfig {
        runtime: "local_agent".to_string(),
        provider: "minimax".to_string(),
        model: "MiniMax-M2.5".to_string(),
        base_url: "https://api.minimax.io/v1".to_string(),
        api_key: Some("x".to_string()),
        domain_config: AgentDomainConfig { domain: "general".to_string(), custom_instructions: None, terminology_strictness: "moderate".to_string() },
        sampling_profiles: AgentSamplingProfilesConfig {
            edit_stable: AgentSamplingConfig { temperature: 0.2, top_p: 0.9, max_tokens: 8192 },
            analysis_balanced: AgentSamplingConfig { temperature: 0.4, top_p: 0.9, max_tokens: 6144 },
            analysis_deep: AgentSamplingConfig { temperature: 0.3, top_p: 0.92, max_tokens: 12288 },
            chat_flexible: AgentSamplingConfig { temperature: 0.7, top_p: 0.95, max_tokens: 4096 },
        },
    };

    assert!(build_openai_runtime_config(cfg).is_err());
}
```

- [ ] **Step 2: Run failing test**

Run: `cargo test -p agent-core openai_runtime_rejects_non_openai_provider -- --nocapture`
Expected: FAIL before creating providers module.

- [ ] **Step 3: Implement core provider module and `run_turn_loop` signature**

```rust
pub struct AgentTurnOutcome {
    pub response_id: Option<String>,
    pub messages: Vec<Value>,
    pub suspended: bool,
}

pub async fn run_turn_loop(
    sink: &dyn EventSink,
    config_provider: &dyn ConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    execute_tool: &ToolExecutorFn,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<AgentTurnOutcome, String> {
    let config = config_provider.load_agent_runtime(Some(&request.project_path))?;
    let app_config_dir = config_provider.app_config_dir()?;
    runtime_state.ensure_storage_at(app_config_dir).await?;
    let resolved_profile = resolve_turn_profile(request);
    let instructions = build_agent_instructions_with_work_state(request, None, Some(&config), None);
    let mut budget = TurnBudget::new(
        max_rounds_for_task(&resolved_profile),
        sampling_profile_params(Some(&resolved_profile.sampling_profile), Some(&config.sampling_profiles))
            .map(|(_, _, max_tokens)| max_tokens),
        cancel_rx.clone(),
    );
    let mut tracker = ToolCallTracker::new(budget.max_rounds);
    run_openai_rounds(
        sink,
        runtime_state,
        request,
        &config,
        &instructions,
        execute_tool,
        &mut budget,
        &mut tracker,
    )
    .await
}
```

- [ ] **Step 4: Convert desktop `openai.rs` into thin wrapper**

```rust
pub async fn run_turn_loop(
    sink: &dyn EventSink,
    config_provider: &dyn agent_core::ConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<agent_core::providers::AgentTurnOutcome, String> {
    let exec = build_tauri_tool_executor();
    agent_core::providers::openai::run_turn_loop(
        sink,
        config_provider,
        runtime_state,
        request,
        &exec,
        cancel_rx,
    )
    .await
}
```

- [ ] **Step 5: Update dispatch call sites in `mod.rs`**

```rust
let sink = adapter::TauriEventSink { window };
let config = adapter::TauriConfigProvider { app: &window.app_handle() };
openai::run_turn_loop(&sink, &config, state, request, cancel_rx).await
```

- [ ] **Step 6: Run tests/builds**

Run: `cargo test -p agent-core providers::openai -- --nocapture`
Expected: PASS.

Run: `cargo build -p agent-core && cargo build -p agent-cli`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/providers/mod.rs crates/agent-core/src/providers/openai.rs crates/agent-core/src/lib.rs apps/desktop/src-tauri/src/agent/openai.rs apps/desktop/src-tauri/src/agent/mod.rs
git commit -m "refactor(agent-core): migrate openai turn loop behind config/event adapters"
```

---

### Task 5: Migrate Chat-Completions Run Loop Into `agent-core`

**Files:**
- Create: `crates/agent-core/src/providers/chat_completions.rs`
- Modify: `crates/agent-core/src/providers/mod.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/agent/chat_completions.rs`
- Modify: `apps/desktop/src-tauri/src/agent/mod.rs`
- Test: `crates/agent-core/src/providers/chat_completions.rs`

- [ ] **Step 1: Add failing tests for provider transport gate**

```rust
#[test]
fn chat_completions_runtime_accepts_only_minimax_or_deepseek() {
    assert!(provider_supports_transport("minimax"));
    assert!(provider_supports_transport("deepseek"));
    assert!(!provider_supports_transport("openai"));
}
```

- [ ] **Step 2: Run failing test**

Run: `cargo test -p agent-core chat_completions_runtime_accepts_only_minimax_or_deepseek -- --nocapture`
Expected: FAIL before module extraction.

- [ ] **Step 3: Implement core chat-completions loop and shared helper reuse**

```rust
pub async fn run_turn_loop(
    sink: &dyn EventSink,
    config_provider: &dyn ConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    history: &[Value],
    execute_tool: &ToolExecutorFn,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<AgentTurnOutcome, String> {
    let runtime = config_provider.load_agent_runtime(Some(&request.project_path))?;
    if !provider_supports_transport(&runtime.provider) {
        return Err(format!("{} cannot be handled by chat_completions runtime.", runtime.provider));
    }
    let resolved_profile = resolve_turn_profile(request);
    let mut instructions = build_agent_instructions_with_work_state(request, None, Some(&runtime), None);
    let requested = tool_choice_for_task(request, &resolved_profile);
    let (_, downgraded) = effective_tool_choice_for_provider(&runtime.provider, requested);
    if downgraded {
        instructions.push_str("\\n[Tool-calling fallback]\\nProvider may ignore required tool choice; call tools before final answer.\\n");
    }
    let mut budget = TurnBudget::new(
        max_rounds_for_task(&resolved_profile),
        sampling_profile_params(Some(&resolved_profile.sampling_profile), Some(&runtime.sampling_profiles))
            .map(|(_, _, max_tokens)| max_tokens),
        cancel_rx.clone(),
    );
    run_chat_completions_rounds(
        sink,
        runtime_state,
        request,
        history,
        &runtime,
        &instructions,
        execute_tool,
        &mut budget,
    )
    .await
}
```

- [ ] **Step 4: Convert desktop `chat_completions.rs` to wrapper + keep smoke tests local**

```rust
pub async fn run_turn_loop(
    sink: &dyn EventSink,
    config_provider: &dyn agent_core::ConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    history: &[Value],
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<agent_core::providers::AgentTurnOutcome, String> {
    let exec = build_tauri_tool_executor();
    agent_core::providers::chat_completions::run_turn_loop(
        sink,
        config_provider,
        runtime_state,
        request,
        history,
        &exec,
        cancel_rx,
    )
    .await
}
```

- [ ] **Step 5: Update `mod.rs` dispatch and continue/resume call chains**

```rust
let sink = adapter::TauriEventSink { window };
let config = adapter::TauriConfigProvider { app: &window.app_handle() };
chat_completions::run_turn_loop(&sink, &config, state, request, history, cancel_rx).await
```

- [ ] **Step 6: Run tests/builds**

Run: `cargo test -p agent-core providers::chat_completions -- --nocapture`
Expected: PASS.

Run: `cargo build -p agent-core && cargo build -p agent-cli`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/providers/chat_completions.rs crates/agent-core/src/providers/mod.rs crates/agent-core/src/lib.rs apps/desktop/src-tauri/src/agent/chat_completions.rs apps/desktop/src-tauri/src/agent/mod.rs
git commit -m "refactor(agent-core): migrate chat-completions turn loop into core runtime"
```

---

### Task 6: Wire `agent-cli` Turn Loop End-to-End

**Files:**
- Create: `crates/agent-cli/src/tool_executor.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/tool_executor.rs`

- [ ] **Step 1: Add failing tests for CLI fallback executor**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::AgentToolCall;

    #[tokio::test]
    async fn unsupported_executor_returns_error_result() {
        let call = AgentToolCall {
            tool_name: "write_file".to_string(),
            call_id: "c1".to_string(),
            arguments: "{}".to_string(),
        };
        let result = execute_cli_tool(call).await;
        assert!(result.is_error);
        assert!(result.preview.contains("not supported in agent-cli"));
    }
}
```

- [ ] **Step 2: Run failing test**

Run: `cargo test -p agent-cli unsupported_executor_returns_error_result -- --nocapture`
Expected: FAIL before creating module.

- [ ] **Step 3: Implement `tool_executor.rs` and integrate closure into CLI runtime call**

```rust
pub async fn execute_cli_tool(call: AgentToolCall) -> AgentToolResult {
    error_result(
        &call.tool_name,
        &call.call_id,
        format!("tool `{}` is not supported in agent-cli yet", call.tool_name),
    )
}
```

```rust
let exec: agent_core::providers::ToolExecutorFn = std::sync::Arc::new(
    |_state, _tab, _root, call, _cancel| {
        Box::pin(async move { crate::tool_executor::execute_cli_tool(call).await })
    },
);
```

- [ ] **Step 4: Add CLI args and call provider run loop**

```rust
#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    prompt: String,
    #[arg(long, default_value = "cli-tab")]
    tab_id: String,
    // existing args...
}
```

```rust
let request = AgentTurnDescriptor {
    project_path: args.project_path.clone(),
    prompt: args.prompt.clone(),
    tab_id: args.tab_id.clone(),
    model: None,
    local_session_id: None,
    previous_response_id: None,
    turn_profile: None,
};
```

- [ ] **Step 5: Verify CLI execution path compiles and runs**

Run: `cargo build -p agent-cli`
Expected: PASS.

Run: `cargo run -p agent-cli --bin agent-runtime -- --api-key test --provider openai --model gpt-5.4 --project-path . --prompt "hello" --tab-id cli-test`
Expected: JSON-line event output or provider configuration/runtime error (but no TODO placeholder output).

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/main.rs crates/agent-cli/src/tool_executor.rs
git commit -m "feat(agent-cli): wire provider turn loop with fallback tool executor"
```

---

### Task 7: Coverage Expansion, Dead-Code Cleanup, and Final Verification

**Files:**
- Modify: `crates/agent-core/src/events.rs`
- Modify: `crates/agent-core/src/streaming.rs`
- Modify: `crates/agent-core/src/message_builder.rs`
- Modify: `crates/agent-core/src/tools.rs`
- Modify: `crates/agent-core/src/provider.rs`
- Modify: `crates/agent-core/src/session.rs`
- Modify: `docs/superpowers/plans/2026-04-14-agent-core-remaining-migration.md` (status notes)
- Test: same files’ `#[cfg(test)]` sections

- [ ] **Step 1: Add failing tests for event serialization surface**

```rust
#[test]
fn agent_event_payload_serializes_with_expected_tag() {
    let payload = AgentEventPayload::Status(AgentStatusEvent {
        stage: "running".to_string(),
        message: "ok".to_string(),
    });
    let value = serde_json::to_value(payload).unwrap();
    assert_eq!(value["type"], "status");
}
```

- [ ] **Step 2: Add failing tests for SSE parser and message builder edge cases**

```rust
#[test]
fn take_next_sse_frame_handles_crlf_separator() {
    let mut buffer = "event: message\r\ndata: {\"ok\":true}\r\n\r\n".to_string();
    let frame = take_next_sse_frame(&mut buffer).expect("frame");
    assert_eq!(frame.0, "message");
}
```

```rust
#[test]
fn effective_tool_choice_downgrades_required_for_unsupported_provider() {
    let (choice, downgraded) = effective_tool_choice_for_provider("deepseek", "required");
    assert_eq!(choice, "auto");
    assert!(downgraded);
}
```

- [ ] **Step 3: Remove dead-code allowances and make types actively used**

```rust
// crates/agent-core/src/provider.rs
// remove #[allow(dead_code)] from AgentTurnHandle and AgentProvider
```

```rust
// crates/agent-core/src/session.rs
// remove stale #[allow(dead_code)] fields once migrated call paths reference them
```

- [ ] **Step 4: Run full quality gate for migrated crates**

Run: `cargo test -p agent-core --lib`
Expected: PASS.

Run: `cargo clippy -p agent-core -- -D warnings`
Expected: PASS.

Run: `cargo build -p agent-cli`
Expected: PASS.

- [ ] **Step 5: Record desktop blocker and keep verification explicit**

```markdown
- `cargo build -p claude-prism-desktop` currently fails in `tectonic` transitive API mismatch.
- Agent migration verification is considered passing when `agent-core` + `agent-cli` gates pass and desktop agent modules compile in CI environment with pinned desktop dependency graph.
```

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/events.rs crates/agent-core/src/streaming.rs crates/agent-core/src/message_builder.rs crates/agent-core/src/tools.rs crates/agent-core/src/provider.rs crates/agent-core/src/session.rs docs/superpowers/plans/2026-04-14-agent-core-remaining-migration.md
git commit -m "test(agent-core): expand coverage and finalize migration cleanup"
```

---

## Final Verification Matrix

1. `cargo build -p agent-core`
2. `cargo test -p agent-core --lib`
3. `cargo clippy -p agent-core -- -D warnings`
4. `cargo build -p agent-cli`
5. `cargo test -p agent-cli`
6. `rg -n "use tauri" crates/agent-core/src` (expected no output)
7. `rg -n "TODO: Wire up the turn execution loop" crates/agent-cli/src/main.rs` (expected no output)
8. `cargo build -p claude-prism-desktop` (currently expected to fail on pre-existing `tectonic` mismatch unless dependency graph is pinned)

## Self-Review

- **Spec coverage:**
  - Remaining provider migration: covered in Task 4 and Task 5.
  - Tool pipeline extraction: covered in Task 3.
  - CLI runtime wiring: covered in Task 6.
  - Test/cleanup: covered in Task 7.
- **Placeholder scan:** No `TODO/TBD/implement later/similar to Task N` placeholders in steps.
- **Type consistency:** `AgentTurnDescriptor`, `AgentTurnOutcome`, `ToolExecutorFn`, `ConfigProvider`, and `EventSink` names are consistent across tasks.
