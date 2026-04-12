# Agent Tool Loop Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent agent tool loops where the model repeatedly calls the same tools (read → shell → edit → read → shell → ...) by adding loop detection and progress checkpoints to both the Chat Completions and OpenAI Responses API tool loops.

**Architecture:** Add a `ToolCallTracker` to `turn_engine.rs` that tracks `(tool_name, normalized_args)` call frequencies across rounds. When repetition is detected, inject system messages to redirect the model. Every N rounds, inject a progress checkpoint summarizing what's been done and remaining budget. Both chat_completions.rs and openai.rs loops consume this shared tracker.

**Tech Stack:** Rust (Tauri backend), serde_json, std::collections::HashMap

---

## File Structure

| File | Role |
|------|------|
| `src/agent/turn_engine.rs` | New `ToolCallTracker` struct + `build_progress_checkpoint` fn |
| `src/agent/chat_completions.rs` | Integrate tracker into both tool loops (line ~891, ~1285) |
| `src/agent/openai.rs` | Integrate tracker into tool loop (line ~605) |
| `src/agent/mod.rs` | Re-export `ToolCallTracker` |

---

### Task 1: Add ToolCallTracker to turn_engine.rs

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/turn_engine.rs`

The tracker maintains a HashMap of `(tool_name, args_signature)` → call count, plus lists of files read and files edited, to generate progress summaries.

- [ ] **Step 1: Add ToolCallTracker struct and methods**

After the `TurnBudget` impl block (around line 112), add:

```rust
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

/// Tracks tool call patterns across rounds to detect repetitive loops.
#[derive(Debug, Clone)]
pub struct ToolCallTracker {
    /// (tool_name, args_hash) → call count
    call_counts: HashMap<(String, u64), u32>,
    /// Files successfully read
    pub files_read: Vec<String>,
    /// Files successfully edited
    pub files_edited: Vec<String>,
    /// Shell commands executed
    pub shells_run: Vec<String>,
    /// Current round index
    pub current_round: u32,
    /// Max rounds for budget display
    pub max_rounds: u32,
}

impl ToolCallTracker {
    pub fn new(max_rounds: u32) -> Self {
        Self {
            call_counts: HashMap::new(),
            files_read: Vec::new(),
            files_edited: Vec::new(),
            shells_run: Vec::new(),
            current_round: 0,
            max_rounds,
        }
    }

    /// Hash the tool arguments into a u64 for dedup.
    fn hash_args(args: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        // Normalize: trim whitespace, lowercase for path-based args
        args.trim().hash(&mut hasher);
        hasher.finish()
    }

    /// Record a tool call. Returns the repetition count (1 = first call).
    pub fn record_call(&mut self, tool_name: &str, args_json: &str) -> u32 {
        let key = (tool_name.to_string(), Self::hash_args(args_json));
        let count = self.call_counts.entry(key).or_insert(0);
        *count += 1;
        *count
    }

    /// Record successful outcomes for progress tracking.
    pub fn record_outcome(&mut self, tool_name: &str, args: &Value) {
        let path = args.get("path").and_then(Value::as_str)
            .or_else(|| args.get("file_path").and_then(Value::as_str))
            .unwrap_or("");
        match tool_name {
            "read_file" => {
                if !path.is_empty() && !self.files_read.contains(&path.to_string()) {
                    self.files_read.push(path.to_string());
                }
            }
            "apply_text_patch" | "replace_selected_text" | "write_file" => {
                if !path.is_empty() && !self.files_edited.contains(&path.to_string()) {
                    self.files_edited.push(path.to_string());
                }
            }
            "run_shell_command" => {
                if let Some(cmd) = args.get("command").and_then(Value::as_str) {
                    let short = if cmd.len() > 60 { &cmd[..60] } else { cmd };
                    self.shells_run.push(short.to_string());
                }
            }
            _ => {}
        }
    }

    /// Generate a repetition warning message for the model, or None if no issue.
    pub fn repetition_warning(&self, tool_name: &str, args_json: &str) -> Option<String> {
        let key = (tool_name.to_string(), Self::hash_args(args_json));
        let count = self.call_counts.get(&key).copied().unwrap_or(0);
        if count >= 3 {
            Some(format!(
                "[Loop detected] You have called {} with the same arguments {} times. \
                 STOP calling this tool repeatedly. Use previous results or take a different approach. \
                 If your edit is complete, summarize the changes to the user without further tool calls.",
                tool_name, count
            ))
        } else if count >= 2 {
            Some(format!(
                "[Repetition notice] You already called {} with similar arguments. \
                 Use the previous result instead of calling it again.",
                tool_name
            ))
        } else {
            None
        }
    }

    /// Build a progress checkpoint message injected every N rounds.
    pub fn progress_checkpoint(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!(
            "Progress checkpoint (round {}/{}):",
            self.current_round + 1,
            self.max_rounds
        ));
        if !self.files_read.is_empty() {
            parts.push(format!("- Files read: {}", self.files_read.join(", ")));
        }
        if !self.files_edited.is_empty() {
            parts.push(format!("- Files edited: {}", self.files_edited.join(", ")));
        }
        if !self.shells_run.is_empty() {
            let display: Vec<&str> = self.shells_run.iter().map(|s| s.as_str()).take(5).collect();
            parts.push(format!("- Shell commands run: {}", display.join("; ")));
        }
        parts.push(format!(
            "- Remaining rounds: {}",
            self.max_rounds.saturating_sub(self.current_round + 1)
        ));
        parts.push(
            "If your task is complete, respond to the user now. \
             Do not run verification commands after successful edits."
                .to_string(),
        );
        parts.join("\n")
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check 2>&1 | tail -5`
Expected: Compiles with only pre-existing warnings.

---

### Task 2: Integrate tracker into chat_completions.rs primary loop

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/chat_completions.rs` (loop at line ~891)

- [ ] **Step 1: Add tracker initialization before the loop**

After `let mut budget = TurnBudget::new(...)` (around line 887), add:

```rust
let mut tracker = ToolCallTracker::new(budget.max_rounds);
```

- [ ] **Step 2: Record calls and inject warnings inside the tool execution loop**

After `execute_tool_calls(...)` returns and inside the `for executed in &executed_calls.executed` loop, after `let feedback = tool_result_feedback_for_model(&result);`, add outcome recording. Then after building all tool_results_messages, check for repetition warnings and inject progress checkpoints.

- [ ] **Step 3: Add progress checkpoint injection after extending next_messages**

After `next_messages.extend(tool_results_messages);` and before `compact_chat_messages`, add:

```rust
// Record tool call patterns and inject loop guards
for executed in &executed_calls.executed {
    let result = &executed.result;
    let args_json = result.content.get("_args_raw")
        .and_then(Value::as_str)
        .unwrap_or("");
    tracker.record_call(&result.tool_name, args_json);
    tracker.record_outcome(&result.tool_name, &result.content);
}
tracker.current_round = round_idx;

// Inject repetition warnings
let mut warnings: Vec<String> = Vec::new();
for executed in &executed_calls.executed {
    let result = &executed.result;
    let args_json = result.content.get("_args_raw")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(warning) = tracker.repetition_warning(&result.tool_name, args_json) {
        warnings.push(warning);
    }
}

// Inject progress checkpoint every 4 rounds, or when warnings detected
let should_checkpoint = (round_idx + 1) % 4 == 0 || !warnings.is_empty();
if should_checkpoint {
    let mut checkpoint = tracker.progress_checkpoint();
    for w in &warnings {
        checkpoint.push('\n');
        checkpoint.push_str(w);
    }
    next_messages.push(json!({
        "role": "system",
        "content": checkpoint,
    }));
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check 2>&1 | tail -5`

---

### Task 3: Integrate tracker into chat_completions.rs secondary loop

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/chat_completions.rs` (loop at line ~1285)

Same pattern as Task 2 but for the second tool loop (used for resumed turns).

---

### Task 4: Integrate tracker into openai.rs loop

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/openai.rs` (loop at line ~605)

Same tracker pattern. The openai.rs loop uses `tool_outputs` instead of `tool_results_messages`, but the tracker integration is identical.

---

### Task 5: Final verification

- [ ] `cargo check` passes
- [ ] `cargo test` passes (if applicable)
- [ ] Manual test: trigger a MiniMax edit task and verify no infinite loop
