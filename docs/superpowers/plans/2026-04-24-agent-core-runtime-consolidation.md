# Agent-Core Runtime Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `agent-core` the single source of truth for runtime policy/tool surface/compaction behavior across desktop and CLI adapters.

**Architecture:** This plan keeps adapters thin and moves policy/selection/compaction decisions into `agent-core` with explicit typed contracts. We introduce a profile-based tool surface, resource-aware policy checks, and a dedicated compact module so providers call shared primitives instead of embedding behavior. Desktop/CLI only map config into core contracts.

**Tech Stack:** Rust 2021, tokio, serde/serde_json, reqwest SSE, existing `agent-core` provider/tool contracts, cargo test.

---

## Scope Check

This plan intentionally targets one subsystem: **agent-core runtime behavior and contracts**. It excludes MCP/LSP/Subagent orchestration and advanced TUI UI work; those should be separate plans.

---

## File Structure

### Create
- `crates/agent-core/src/policy.rs` - Resource/path policy matcher and deny reasons for tool calls.
- `crates/agent-core/src/compact.rs` - Message compaction strategy and pure helpers.
- `crates/agent-core/src/providers/common.rs` - Shared provider retry/backoff and streaming utility helpers.

### Modify
- `crates/agent-core/src/config.rs` - Add runtime `tool_profile` + `resource_policy` configuration.
- `crates/agent-core/src/provider.rs` - Extend turn profile contract for resource policy propagation.
- `crates/agent-core/src/tools.rs` - Add profile-aware tool selection and policy enforcement integration.
- `crates/agent-core/src/turn_engine.rs` - Use compact module + richer tool policy context.
- `crates/agent-core/src/providers/chat_completions.rs` - Use profile-aware tool schemas + shared provider helpers.
- `crates/agent-core/src/providers/openai.rs` - Same as above.
- `crates/agent-core/src/providers/mod.rs` - Export `common` module.
- `crates/agent-core/src/lib.rs` - Export `compact` and `policy` modules.
- `apps/desktop/src-tauri/src/settings/mod.rs` - Map desktop settings to core `tool_profile` + `resource_policy` defaults.
- `crates/agent-cli/src/main.rs` - Set CLI static runtime config to `coding_cli` profile.

### Test
- `crates/agent-core/src/config.rs` (new tests)
- `crates/agent-core/src/tools.rs` (new tests)
- `crates/agent-core/src/policy.rs` (new tests)
- `crates/agent-core/src/compact.rs` (new tests)
- `crates/agent-core/src/providers/chat_completions.rs` (updated tests)
- `crates/agent-core/src/providers/openai.rs` (updated tests)

---

### Task 1: Add Profile-Based Tool Surface in `agent-core`

**Files:**
- Modify: `crates/agent-core/src/config.rs`
- Modify: `crates/agent-core/src/tools.rs`
- Modify: `crates/agent-core/src/providers/chat_completions.rs`
- Modify: `crates/agent-core/src/providers/openai.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Test: `crates/agent-core/src/config.rs`
- Test: `crates/agent-core/src/tools.rs`

- [ ] **Step 1: Write failing tests for profile-specific tool lists**

```rust
#[test]
fn coding_cli_profile_excludes_academic_tools() {
    let names = default_tool_specs_for_profile(AgentToolProfile::CodingCli)
        .into_iter()
        .map(|s| s.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"read_file".to_string()));
    assert!(names.contains(&"write_file".to_string()));
    assert!(names.contains(&"run_shell_command".to_string()));
    assert!(!names.contains(&"search_literature".to_string()));
    assert!(!names.contains(&"review_manuscript".to_string()));
}

#[test]
fn full_academic_profile_keeps_existing_surface() {
    let names = default_tool_specs_for_profile(AgentToolProfile::FullAcademic)
        .into_iter()
        .map(|s| s.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"search_literature".to_string()));
    assert!(names.contains(&"draft_section".to_string()));
    assert!(names.contains(&"write_file".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-core coding_cli_profile_excludes_academic_tools -- --nocapture`
Expected: FAIL with unresolved `AgentToolProfile` / `default_tool_specs_for_profile`.

- [ ] **Step 3: Add runtime profile types in config**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentToolProfile {
    FullAcademic,
    CodingCli,
}

#[derive(Debug, Clone)]
pub struct AgentRuntimeConfig {
    pub runtime: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub domain_config: AgentDomainConfig,
    pub sampling_profiles: AgentSamplingProfilesConfig,
    pub tool_profile: AgentToolProfile,
    pub resource_policy: AgentResourcePolicy,
}
```

```rust
impl AgentRuntimeConfig {
    pub fn default_local_agent() -> Self {
        Self {
            runtime: "local_agent".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: None,
            domain_config: AgentDomainConfig {
                domain: "general".to_string(),
                custom_instructions: None,
                terminology_strictness: "moderate".to_string(),
            },
            sampling_profiles: AgentSamplingProfilesConfig {
                edit_stable: AgentSamplingConfig { temperature: 0.2, top_p: 0.9, max_tokens: 8192 },
                analysis_balanced: AgentSamplingConfig { temperature: 0.4, top_p: 0.9, max_tokens: 6144 },
                analysis_deep: AgentSamplingConfig { temperature: 0.3, top_p: 0.92, max_tokens: 12288 },
                chat_flexible: AgentSamplingConfig { temperature: 0.7, top_p: 0.95, max_tokens: 4096 },
            },
            tool_profile: AgentToolProfile::FullAcademic,
            resource_policy: AgentResourcePolicy::default(),
        }
    }
}
```

- [ ] **Step 4: Implement profile-aware tool-spec selection and provider wiring**

```rust
pub fn default_tool_specs_for_profile(profile: AgentToolProfile) -> Vec<AgentToolSpec> {
    match profile {
        AgentToolProfile::FullAcademic => build_default_tool_specs(true),
        AgentToolProfile::CodingCli => build_coding_cli_tool_specs(),
    }
}

fn build_coding_cli_tool_specs() -> Vec<AgentToolSpec> {
    vec![
        make_tool_spec("read_file", "Read a text file from the current project.", /* schema */ json!({...})),
        make_tool_spec("list_files", "List files inside the current project.", json!({...})),
        make_tool_spec("search_project", "Search for text in the current project using ripgrep.", json!({...})),
        make_tool_spec("replace_selected_text", "Replace currently selected text.", json!({...})),
        make_tool_spec("apply_text_patch", "Apply a precise text patch to a file.", json!({...})),
        make_tool_spec("write_file", "Write full replacement content to a file.", json!({...})),
        make_tool_spec("run_shell_command", "Run a shell command in the current project.", json!({...})),
        make_tool_spec("remember_fact", "Save an important fact to persistent memory.", json!({...})),
    ]
}
```

```rust
let specs = default_tool_specs_for_profile(config.tool_profile)
    .into_iter()
    .map(|spec| to_chat_completions_tool_schema(&spec, &config.provider))
    .collect::<Vec<_>>();
```

```rust
let tools = default_tool_specs_for_profile(config.tool_profile)
    .into_iter()
    .map(|spec| to_openai_tool_schema(&spec))
    .collect::<Vec<_>>();
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p agent-core default_tool_specs -- --nocapture`
Expected: PASS with profile-specific tests green.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/config.rs crates/agent-core/src/tools.rs crates/agent-core/src/providers/chat_completions.rs crates/agent-core/src/providers/openai.rs crates/agent-core/src/lib.rs
git commit -m "feat(agent-core): add profile-based tool surface selection"
```

---

### Task 2: Add Core Resource Policy Engine and Enforce in Tool Policy Checks

**Files:**
- Create: `crates/agent-core/src/policy.rs`
- Modify: `crates/agent-core/src/config.rs`
- Modify: `crates/agent-core/src/provider.rs`
- Modify: `crates/agent-core/src/tools.rs`
- Modify: `crates/agent-core/src/turn_engine.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Test: `crates/agent-core/src/policy.rs`
- Test: `crates/agent-core/src/tools.rs`

- [ ] **Step 1: Write failing tests for read/write path policy**

```rust
#[test]
fn denies_write_outside_allowed_prefix() {
    let policy = AgentResourcePolicy {
        deny_prefixes: vec!["secrets/".to_string()],
        allow_read_prefixes: vec![],
        allow_write_prefixes: vec!["src/".to_string()],
    };

    let check = evaluate_resource_access(&policy, ToolResourceScope::TextFile, PolicyAccessMode::Write, Some("docs/guide.md"));
    assert!(check.is_err());
}

#[test]
fn allows_read_when_allow_list_empty() {
    let policy = AgentResourcePolicy::default();
    let check = evaluate_resource_access(&policy, ToolResourceScope::TextFile, PolicyAccessMode::Read, Some("README.md"));
    assert!(check.is_ok());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-core denies_write_outside_allowed_prefix -- --nocapture`
Expected: FAIL with unresolved `evaluate_resource_access`.

- [ ] **Step 3: Implement `policy.rs` and resource policy config types**

```rust
#[derive(Debug, Clone, Default)]
pub struct AgentResourcePolicy {
    pub deny_prefixes: Vec<String>,
    pub allow_read_prefixes: Vec<String>,
    pub allow_write_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAccessMode {
    Read,
    Write,
}

pub fn evaluate_resource_access(
    policy: &AgentResourcePolicy,
    scope: ToolResourceScope,
    mode: PolicyAccessMode,
    target: Option<&str>,
) -> Result<(), String> {
    if !matches!(scope, ToolResourceScope::TextFile | ToolResourceScope::Workspace | ToolResourceScope::Document) {
        return Ok(());
    }

    let Some(target) = target else {
        return Ok(());
    };

    let normalized = target.replace('\\', "/").trim_start_matches('/').to_string();

    if policy.deny_prefixes.iter().any(|p| normalized.starts_with(p)) {
        return Err(format!("target '{}' is denied by resource policy", normalized));
    }

    let allow_list = match mode {
        PolicyAccessMode::Read => &policy.allow_read_prefixes,
        PolicyAccessMode::Write => &policy.allow_write_prefixes,
    };

    if allow_list.is_empty() || allow_list.iter().any(|p| normalized.starts_with(p)) {
        Ok(())
    } else {
        Err(format!("target '{}' is outside allowed {:?} prefixes", normalized, mode))
    }
}
```

- [ ] **Step 4: Enforce policy in `check_tool_call_policy`**

```rust
#[derive(Debug, Clone)]
pub struct ToolExecutionPolicyContext {
    pub task_kind: AgentTaskKind,
    pub has_binary_attachment_context: bool,
    pub resource_policy: AgentResourcePolicy,
}
```

```rust
let access_mode = match call.tool_name.as_str() {
    "write_file" | "apply_text_patch" | "replace_selected_text" => Some(PolicyAccessMode::Write),
    "read_file" | "search_project" | "list_files" | "read_document" | "read_document_excerpt" | "search_document_text" => Some(PolicyAccessMode::Read),
    _ => None,
};

if let Some(mode) = access_mode {
    if let Err(reason) = evaluate_resource_access(
        &context.resource_policy,
        tool_contract(&call.tool_name).resource_scope,
        mode,
        target,
    ) {
        return Some(AgentToolResult {
            tool_name: call.tool_name.clone(),
            call_id: call.call_id.clone(),
            is_error: true,
            preview: "Tool call blocked by resource policy.".to_string(),
            content: json!({
                "error": reason,
                "disallowedByPolicy": true,
                "attemptedTool": call.tool_name,
                "policyKind": "resource_path",
            }),
        });
    }
}
```

```rust
let policy_context = ToolExecutionPolicyContext {
    task_kind: resolved_profile.task_kind.clone(),
    has_binary_attachment_context: request_has_binary_attachment_context(request),
    resource_policy: resolved_profile
        .resource_policy
        .clone()
        .unwrap_or_default(),
};
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p agent-core policy -- --nocapture`
Expected: PASS for resource policy matcher and tool policy block tests.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/policy.rs crates/agent-core/src/config.rs crates/agent-core/src/provider.rs crates/agent-core/src/tools.rs crates/agent-core/src/turn_engine.rs crates/agent-core/src/lib.rs
git commit -m "feat(agent-core): add resource-aware tool path policy enforcement"
```

---

### Task 3: Extract Compaction into Dedicated Core Module (Compact v2)

**Files:**
- Create: `crates/agent-core/src/compact.rs`
- Modify: `crates/agent-core/src/turn_engine.rs`
- Modify: `crates/agent-core/src/providers/chat_completions.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Test: `crates/agent-core/src/compact.rs`

- [ ] **Step 1: Write failing tests for compaction behavior in new module**

```rust
#[test]
fn compaction_keeps_system_and_recent_tail() {
    let mut messages = vec![
        json!({"role":"system","content":"system"}),
        json!({"role":"user","content":"old-1"}),
        json!({"role":"assistant","content":"old-2"}),
        json!({"role":"user","content":"recent-1"}),
        json!({"role":"assistant","content":"recent-2"}),
    ];

    let changed = maybe_compact_messages(
        &mut messages,
        CompactConfig { token_limit: 8, summary_reserve: 2 },
    );

    assert!(changed);
    assert_eq!(messages[0]["role"], "system");
    assert!(messages.iter().any(|m| m["content"].as_str().unwrap_or("").contains("Context compacted")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-core compaction_keeps_system_and_recent_tail -- --nocapture`
Expected: FAIL with unresolved `maybe_compact_messages`.

- [ ] **Step 3: Implement `compact.rs` and move compact logic from turn_engine**

```rust
#[derive(Debug, Clone, Copy)]
pub struct CompactConfig {
    pub token_limit: u32,
    pub summary_reserve: u32,
}

pub fn maybe_compact_messages(messages: &mut Vec<Value>, config: CompactConfig) -> bool {
    let total_tokens = estimate_messages_tokens(messages);
    if total_tokens <= config.token_limit || messages.len() <= 3 {
        return false;
    }

    let cut_point = find_compaction_cut_point(messages, config);
    if cut_point <= 1 {
        return false;
    }

    let dropped_count = cut_point.saturating_sub(1);
    messages.splice(
        1..cut_point,
        std::iter::once(json!({
            "role": "system",
            "content": format!(
                "[Context compacted: {} earlier messages removed. Recent context preserved below.]",
                dropped_count
            ),
        })),
    );

    true
}
```

- [ ] **Step 4: Wire providers and turn_engine to use new module API**

```rust
use crate::compact::{maybe_compact_messages, CompactConfig};

let _compacted = maybe_compact_messages(
    &mut next_messages,
    CompactConfig {
        token_limit: 60_000,
        summary_reserve: 200,
    },
);
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p agent-core compact -- --nocapture`
Expected: PASS for compact module tests and provider call sites.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/compact.rs crates/agent-core/src/turn_engine.rs crates/agent-core/src/providers/chat_completions.rs crates/agent-core/src/lib.rs
git commit -m "refactor(agent-core): extract compact v2 strategy into dedicated module"
```

---

### Task 4: Introduce Provider Shared Helpers to Remove Duplicated Runtime Logic

**Files:**
- Create: `crates/agent-core/src/providers/common.rs`
- Modify: `crates/agent-core/src/providers/chat_completions.rs`
- Modify: `crates/agent-core/src/providers/openai.rs`
- Modify: `crates/agent-core/src/providers/mod.rs`
- Test: `crates/agent-core/src/providers/chat_completions.rs`
- Test: `crates/agent-core/src/providers/openai.rs`

- [ ] **Step 1: Write failing tests for shared retry/backoff utility**

```rust
#[test]
fn backoff_seconds_scales_with_attempt() {
    assert_eq!(retry_delay_seconds(1), 1);
    assert_eq!(retry_delay_seconds(2), 2);
    assert_eq!(retry_delay_seconds(3), 4);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-core backoff_seconds_scales_with_attempt -- --nocapture`
Expected: FAIL with unresolved `retry_delay_seconds`.

- [ ] **Step 3: Implement provider shared helpers**

```rust
pub fn retry_delay_seconds(attempt: u32) -> u64 {
    match attempt {
        0 | 1 => 1,
        2 => 2,
        _ => 4,
    }
}

pub fn should_retry_status(status: u16) -> bool {
    matches!(status, 429 | 503)
}

pub fn redact_api_key(value: &str) -> String {
    if value.len() <= 8 {
        "********".to_string()
    } else {
        format!("{}****{}", &value[..4], &value[value.len() - 4..])
    }
}
```

- [ ] **Step 4: Replace duplicated retry code paths in both providers**

```rust
if should_retry_status(status.as_u16()) && attempt < max_retries {
    let delay = retry_delay_seconds(attempt + 1);
    emit_status(
        sink,
        &request.tab_id,
        "retrying",
        &format!(
            "Received {} from provider, retrying in {}s (attempt {}/{})...",
            status,
            delay,
            attempt + 1,
            max_retries
        ),
    );
    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
    continue;
}
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p agent-core providers -- --nocapture`
Expected: PASS with unchanged provider behavior and shared utility coverage.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/providers/common.rs crates/agent-core/src/providers/chat_completions.rs crates/agent-core/src/providers/openai.rs crates/agent-core/src/providers/mod.rs
git commit -m "refactor(agent-core): share provider retry/backoff helpers"
```

---

### Task 5: Wire Desktop and CLI Adapters to Core Contracts

**Files:**
- Modify: `apps/desktop/src-tauri/src/settings/mod.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: Write failing test for CLI default tool profile**

```rust
#[test]
fn static_provider_uses_coding_cli_profile_for_agent_cli() {
    let resolved = ResolvedConfig {
        provider: "minimax".to_string(),
        model: "MiniMax-M2.7".to_string(),
        api_key: "test-key".to_string(),
        base_url: "https://api.minimax.chat/v1".to_string(),
        output: "human".to_string(),
    };

    let provider = static_provider_for(&resolved);
    assert_eq!(provider.config.tool_profile, AgentToolProfile::CodingCli);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent-cli static_provider_uses_coding_cli_profile_for_agent_cli -- --nocapture`
Expected: FAIL because `tool_profile` is not assigned.

- [ ] **Step 3: Map adapter defaults into core runtime config**

```rust
// crates/agent-cli/src/main.rs
fn static_provider_for(resolved: &ResolvedConfig) -> StaticConfigProvider {
    let mut config = AgentRuntimeConfig::default_local_agent();
    config.provider = resolved.provider.clone();
    config.model = resolved.model.clone();
    config.api_key = Some(resolved.api_key.clone());
    config.base_url = resolved.base_url.clone();
    config.tool_profile = AgentToolProfile::CodingCli;
    config.resource_policy = AgentResourcePolicy::default();

    StaticConfigProvider {
        config,
        config_dir: std::env::temp_dir().join("agent-runtime"),
    }
}
```

```rust
// apps/desktop/src-tauri/src/settings/mod.rs
Ok(AgentRuntimeConfig {
    runtime,
    provider,
    model,
    base_url,
    api_key,
    domain_config: AgentDomainConfig { domain, custom_instructions, terminology_strictness },
    sampling_profiles: AgentSamplingProfilesConfig { /* existing mapping */ },
    tool_profile: AgentToolProfile::FullAcademic,
    resource_policy: AgentResourcePolicy::default(),
})
```

- [ ] **Step 4: Add desktop/CLI smoke tests for profile mapping**

```rust
#[test]
fn default_config_profile_is_full_academic() {
    let cfg = AgentRuntimeConfig::default_local_agent();
    assert_eq!(cfg.tool_profile, AgentToolProfile::FullAcademic);
}
```

```rust
#[test]
fn static_provider_uses_coding_cli_profile_for_agent_cli() {
    // same as step 1
}
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p agent-core default_config_profile_is_full_academic -- --nocapture`
Expected: PASS.

Run: `cargo test -p agent-cli static_provider_uses_coding_cli_profile_for_agent_cli -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/settings/mod.rs crates/agent-cli/src/main.rs crates/agent-core/src/config.rs
git commit -m "feat(agent-core): wire adapter defaults to profile and resource policy contracts"
```

---

### Task 6: End-to-End Validation and Documentation Update

**Files:**
- Modify: `docs/superpowers/crates-agent-handoff.md`
- Modify: `docs/superpowers/plans/2026-04-24-agent-core-runtime-consolidation.md` (checklist updates only)

- [ ] **Step 1: Add handoff documentation for new core contracts**

```markdown
## Runtime Contracts (2026-04 core consolidation)

- `AgentRuntimeConfig.tool_profile`
  - `full_academic`: desktop local-agent surface
  - `coding_cli`: agent-cli coding surface
- `AgentRuntimeConfig.resource_policy`
  - deny/allow prefixes for read/write policy enforcement in `check_tool_call_policy`
- Compaction now routes through `agent_core::compact::maybe_compact_messages`.
- Providers share retry/backoff helpers from `agent_core::providers::common`.
```

- [ ] **Step 2: Run full target test matrix**

Run: `cargo test -p agent-core -- --nocapture`
Expected: PASS.

Run: `cargo test -p agent-cli -- --nocapture`
Expected: PASS except pre-existing unrelated failures explicitly noted.

Run: `cargo build -p claude-prism-desktop`
Expected: PASS (or only unrelated warnings).

- [ ] **Step 3: Run lint and compile checks**

Run: `cargo clippy -p agent-core -- -D warnings`
Expected: PASS.

Run: `cargo check -p agent-cli`
Expected: PASS.

- [ ] **Step 4: Create release-note style summary commit**

```bash
git add docs/superpowers/crates-agent-handoff.md
git commit -m "docs(agent-core): document runtime profile, policy, and compact contracts"
```

- [ ] **Step 5: Tag migration completeness in PR description template**

```markdown
### Validation checklist
- [x] Profile-based tool schemas validated
- [x] Resource path policy blocks enforced
- [x] Compact v2 extraction validated
- [x] Provider shared retry helper validated
- [x] Desktop + CLI adapter mappings validated
```

- [ ] **Step 6: Final merge readiness command block**

```bash
git status
cargo test -p agent-core
cargo test -p agent-cli
cargo build -p claude-prism-desktop
```

Expected: clean working tree (except intentional doc/version bumps) and green target builds.

---

## Self-Review

### 1. Spec coverage
- Agent-core收口主线：覆盖（Task 1, 4, 5）
- 核心策略能力（工具面 + 资源权限）：覆盖（Task 1, 2）
- 长会话稳定性（compact）：覆盖（Task 3）
- 适配层薄化与一致映射：覆盖（Task 5）
- 验证与文档：覆盖（Task 6）

No uncovered requirement in this scoped plan.

### 2. Placeholder scan
- No `TODO/TBD/implement later` markers.
- Every code-changing step includes concrete Rust/Markdown snippets.
- Every test step has exact command + expected output.

### 3. Type consistency
- `AgentToolProfile`, `AgentResourcePolicy`, `ToolExecutionPolicyContext` names are consistent across tasks.
- `default_tool_specs_for_profile(...)` is used consistently in provider wiring.
- `maybe_compact_messages(...)` is consistently referenced as compact entrypoint.

