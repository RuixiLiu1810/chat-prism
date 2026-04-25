# P1: Context Engineering — Pre-Send Compaction + Model Window Awareness + Circuit Breaker

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让上下文管理从"溢出后响应式截断"升级为"发送前主动检查"：在每次 API 请求前，根据目标模型的实际 context window 大小检查 token 估算，必要时压缩；同时添加连续工具失败熔断器，防止 agent 卡死在工具错误循环中。

**Architecture:** 在 `config.rs` 中新增模型 context window 映射；修改 `compact_chat_messages()` 返回 `bool` 表示是否实际压缩；在 `providers/chat_completions.rs` 和 `providers/openai.rs` 的 `run_turn_loop` 中，在构建请求体之前添加 pre-send 压缩循环，含熔断（最多 3 次失败）。工具失败熔断器通过扩展 `ToolCallTracker` 实现，连续 5 次同工具错误触发 turn 终止。

**Tech Stack:** Rust, serde_json (token 估算)

---

## v2 强制修订（本节优先于下文）

### Non-goals
- 不修改工具执行器接口形态（留给 P3）。
- 不引入 memory 或 subagent 功能。
- 不做 provider 之外的大规模重构。

### 口径修正
- 模型窗口映射应采用**可扩展配置表**（集中于 `config.rs`），避免在多个模块硬编码分支。
- 优先复用当前 compact 模块：`compact::maybe_compact_messages(...)` 返回值可作为 pre-send 判定。
- 若当前 `turn_engine::compact_chat_messages` 已是 wrapper，则无需强制改签名为 `bool`；以最小改动接入 pre-send 循环即可。
- 熔断器要区分“可恢复失败”与“不可恢复失败”，错误文案需可解释（避免静默终止）。

### DoD（v2）
- [ ] pre-send 阶段根据模型窗口阈值决定是否压缩。
- [ ] 连续失败达到阈值触发熔断并返回可解释错误。
- [ ] 长对话不再在首轮发送时触发 context overflow。
- [ ] `cargo test -p agent-core` 通过。

### 执行约束
- 下文凡涉及“强制改 `compact_chat_messages` 签名”的步骤，改为“按现有 compact API 最小接入”。

---

## File Map

| 操作 | 文件 | 职责 |
|------|------|------|
| Modify | `crates/agent-core/src/config.rs` | 新增 `context_window_for_model()`、`AUTOCOMPACT_BUFFER_TOKENS`、`MAX_CONSECUTIVE_COMPACT_FAILURES` |
| Modify | `crates/agent-core/src/turn_engine.rs` | `compact_chat_messages()` 改返回 `bool`；扩展 `ToolCallTracker` |
| Modify | `crates/agent-core/src/providers/chat_completions.rs` | pre-send 压缩循环 |
| Modify | `crates/agent-core/src/providers/openai.rs` | 同步 pre-send 压缩 |
| Modify | `crates/agent-core/src/lib.rs` | 导出新常量和函数 |

---

## Task 1: 在 config.rs 中添加模型 context window 映射

**Files:**
- Modify: `crates/agent-core/src/config.rs`

- [ ] **Step 1: 写失败测试**

在 `crates/agent-core/src/config.rs` 末尾添加：

```rust
#[cfg(test)]
mod context_window_tests {
    use super::*;

    #[test]
    fn claude_opus_4_has_large_window() {
        assert_eq!(context_window_for_model("claude-opus-4-5"), 200_000);
    }

    #[test]
    fn deepseek_has_64k_window() {
        assert_eq!(context_window_for_model("deepseek-chat"), 64_000);
    }

    #[test]
    fn unknown_model_falls_back_to_32k() {
        assert_eq!(context_window_for_model("some-unknown-model-xyz"), 32_000);
    }

    #[test]
    fn autocompact_threshold_is_window_minus_buffer() {
        let window = context_window_for_model("deepseek-chat");
        let threshold = autocompact_threshold_for_model("deepseek-chat");
        assert_eq!(threshold, window - AUTOCOMPACT_BUFFER_TOKENS);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p agent-core context_window_tests 2>&1 | head -10
```

Expected: 编译错误，`context_window_for_model` 等未定义。

- [ ] **Step 3: 在 config.rs 中实现函数和常量**

在 `config.rs` 末尾（现有代码之后）添加：

```rust
/// token 缓冲区：压缩阈值 = context_window - AUTOCOMPACT_BUFFER_TOKENS
pub const AUTOCOMPACT_BUFFER_TOKENS: u32 = 13_000;

/// 连续压缩失败熔断上限
pub const MAX_CONSECUTIVE_COMPACT_FAILURES: u32 = 3;

/// 根据模型名称返回 context window 大小（token 数）
pub fn context_window_for_model(model: &str) -> u32 {
    let m = model.to_ascii_lowercase();
    if m.contains("claude-opus-4")
        || m.contains("claude-sonnet")
        || m.contains("claude-haiku")
        || m.contains("claude-3")
    {
        200_000
    } else if m.contains("gpt-4o") {
        128_000
    } else if m.contains("gpt-4") {
        128_000
    } else if m.contains("deepseek") {
        64_000
    } else if m.contains("minimax") || m.contains("abab") {
        40_000
    } else if m.contains("gpt-3.5") {
        16_000
    } else {
        32_000 // 保守默认
    }
}

/// 压缩触发阈值 = context_window - buffer
pub fn autocompact_threshold_for_model(model: &str) -> u32 {
    context_window_for_model(model).saturating_sub(AUTOCOMPACT_BUFFER_TOKENS)
}
```

- [ ] **Step 4: 在 lib.rs 中导出**

在 `crates/agent-core/src/lib.rs` 的 `pub use config::` 行中添加：

```rust
pub use config::{
    AgentDomainConfig, AgentRuntimeConfig, AgentSamplingConfig, AgentSamplingProfilesConfig,
    ConfigProvider, StaticConfigProvider,
    // 新增:
    autocompact_threshold_for_model, context_window_for_model,
    AUTOCOMPACT_BUFFER_TOKENS, MAX_CONSECUTIVE_COMPACT_FAILURES,
};
```

- [ ] **Step 5: 运行测试**

```bash
cargo test -p agent-core context_window_tests 2>&1
```

Expected: 4 个测试全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/config.rs crates/agent-core/src/lib.rs
git commit -m "feat(context): add model context window mapping and autocompact threshold"
```

---

## Task 2: compact_chat_messages 改返回 bool

**Files:**
- Modify: `crates/agent-core/src/turn_engine.rs`

- [ ] **Step 1: 找到当前 compact_chat_messages 签名**

```bash
grep -n "pub fn compact_chat_messages" crates/agent-core/src/turn_engine.rs
```

Expected: 找到函数定义，当前返回 `()`。

- [ ] **Step 2: 写失败测试**

在 `turn_engine.rs` 的 `#[cfg(test)]` 块中添加：

```rust
#[test]
fn compact_returns_true_when_messages_dropped() {
    let big_content = "a".repeat(2000);
    let mut messages: Vec<Value> = (0..50)
        .map(|i| json!({"role": "user", "content": format!("[{}] {}", i, big_content)}))
        .collect();
    let did_compact = compact_chat_messages(&mut messages);
    assert!(did_compact, "should return true when messages were dropped");
    assert!(messages.len() < 50, "messages should be reduced");
}

#[test]
fn compact_returns_false_when_no_compaction_needed() {
    let mut messages = vec![
        json!({"role": "user", "content": "hello"}),
        json!({"role": "assistant", "content": "hi"}),
    ];
    let did_compact = compact_chat_messages(&mut messages);
    assert!(!did_compact, "should return false when nothing was dropped");
}
```

- [ ] **Step 3: 运行测试确认失败**

```bash
cargo test -p agent-core compact_returns 2>&1 | head -10
```

Expected: 编译错误或类型不匹配（返回值不是 `bool`）。

- [ ] **Step 4: 修改 compact_chat_messages 返回 bool**

找到函数定义（约 670 行），修改签名：

```rust
// 修改前:
pub fn compact_chat_messages(messages: &mut Vec<Value>) {

// 修改后:
pub fn compact_chat_messages(messages: &mut Vec<Value>) -> bool {
```

在函数内部，找到实际删除消息的位置，在函数末尾：
- 如果实际删除了任何消息，`return true;`
- 否则 `return false;`

具体实现：在删除前记录原始长度，函数结束时返回 `messages.len() < original_len`：

```rust
pub fn compact_chat_messages(messages: &mut Vec<Value>) -> bool {
    let original_len = messages.len();
    // ... 现有压缩逻辑保持不变 ...
    messages.len() < original_len
}
```

- [ ] **Step 5: 修复所有调用点（现有调用不捕获返回值会触发警告但不报错）**

```bash
grep -rn "compact_chat_messages(" crates/ | grep -v "test\|#\["
```

找到所有调用点，如果不需要返回值，加 `let _ =` 前缀：

```rust
let _ = compact_chat_messages(&mut messages);
```

- [ ] **Step 6: 运行测试**

```bash
cargo test -p agent-core compact_returns 2>&1
```

Expected: 2 个测试 PASS。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/turn_engine.rs
git commit -m "refactor(context): compact_chat_messages returns bool indicating actual compaction"
```

---

## Task 3: 扩展 ToolCallTracker — 连续工具失败熔断

**Files:**
- Modify: `crates/agent-core/src/turn_engine.rs`

- [ ] **Step 1: 找到 ToolCallTracker 定义**

```bash
grep -n "pub struct ToolCallTracker\|impl ToolCallTracker" crates/agent-core/src/turn_engine.rs | head -5
```

Expected: 找到结构体定义行号（约 490 行）。

- [ ] **Step 2: 写失败测试**

```rust
#[cfg(test)]
mod circuit_breaker_tests {
    use super::*;

    #[test]
    fn no_circuit_break_for_different_tools() {
        let mut tracker = ToolCallTracker::default();
        for _ in 0..10 {
            tracker.record_tool_error("tool_a");
            tracker.record_tool_error("tool_b");
        }
        assert!(!tracker.should_circuit_break());
    }

    #[test]
    fn circuit_breaks_after_5_consecutive_same_tool_errors() {
        let mut tracker = ToolCallTracker::default();
        for i in 0..5 {
            let triggered = tracker.record_tool_error("read_file");
            if i < 4 {
                assert!(!triggered, "should not break at attempt {}", i);
            } else {
                assert!(triggered, "should break at attempt 5");
            }
        }
        assert!(tracker.should_circuit_break());
    }

    #[test]
    fn reset_clears_circuit_breaker() {
        let mut tracker = ToolCallTracker::default();
        for _ in 0..5 {
            tracker.record_tool_error("read_file");
        }
        assert!(tracker.should_circuit_break());
        tracker.reset_tool_errors();
        assert!(!tracker.should_circuit_break());
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

```bash
cargo test -p agent-core circuit_breaker_tests 2>&1 | head -10
```

Expected: 编译错误，相关方法未定义。

- [ ] **Step 4: 扩展 ToolCallTracker**

找到 `pub struct ToolCallTracker` 定义，添加新字段：

```rust
pub struct ToolCallTracker {
    // 已有字段（保持不变）...
    
    // 新增字段：
    consecutive_tool_errors: u32,
    last_error_tool_name: Option<String>,
}
```

在 `Default` 或 `new()` 实现中初始化新字段：

```rust
consecutive_tool_errors: 0,
last_error_tool_name: None,
```

在 `impl ToolCallTracker` 中添加新方法：

```rust
const CONSECUTIVE_TOOL_ERROR_LIMIT: u32 = 5;

/// 记录工具错误，返回 true 表示已触发熔断。
pub fn record_tool_error(&mut self, tool_name: &str) -> bool {
    if self.last_error_tool_name.as_deref() == Some(tool_name) {
        self.consecutive_tool_errors += 1;
    } else {
        self.consecutive_tool_errors = 1;
        self.last_error_tool_name = Some(tool_name.to_string());
    }
    self.consecutive_tool_errors >= Self::CONSECUTIVE_TOOL_ERROR_LIMIT
}

/// 成功执行工具后调用，重置连续错误计数。
pub fn reset_tool_errors(&mut self) {
    self.consecutive_tool_errors = 0;
    self.last_error_tool_name = None;
}

/// 当前是否处于熔断状态。
pub fn should_circuit_break(&self) -> bool {
    self.consecutive_tool_errors >= Self::CONSECUTIVE_TOOL_ERROR_LIMIT
}
```

- [ ] **Step 5: 在工具执行结果处理中集成熔断**

找到 `execute_tool_calls` 函数（约 391 行）内部处理 tool_result 的地方，在处理每个 result 后添加：

```rust
// 在处理 tool result 后：
if result.is_error {
    if tracker.record_tool_error(&result.tool_name) {
        emit_status(
            sink,
            tab_id,
            "warning",
            &format!(
                "Tool '{}' failed {} consecutive times. Stopping turn to prevent loop.",
                result.tool_name,
                ToolCallTracker::CONSECUTIVE_TOOL_ERROR_LIMIT,
            ),
        );
        return Err(format!(
            "Circuit breaker: tool '{}' failed too many consecutive times.",
            result.tool_name
        ));
    }
} else {
    tracker.reset_tool_errors();
}
```

- [ ] **Step 6: 运行测试**

```bash
cargo test -p agent-core circuit_breaker_tests 2>&1
```

Expected: 3 个测试全部 PASS。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/turn_engine.rs
git commit -m "feat(stability): add consecutive tool failure circuit breaker to ToolCallTracker"
```

---

## Task 4: Pre-Send 压缩循环（chat_completions.rs）

**Files:**
- Modify: `crates/agent-core/src/providers/chat_completions.rs`

- [ ] **Step 1: 找到构建 messages 的位置**

```bash
grep -n "transcript_to_chat_messages\|let mut messages" crates/agent-core/src/providers/chat_completions.rs | head -5
```

Expected: 找到 `messages` 被构建的行。

- [ ] **Step 2: 写测试**

在 `chat_completions.rs` 末尾添加（测试 pre-send 压缩逻辑本身可通过 turn_engine 测试覆盖，此处写集成编译测试）：

```rust
#[cfg(test)]
mod presend_tests {
    #[test]
    fn presend_compaction_imports_available() {
        // 验证所需函数可从 agent_core 访问
        let _ = agent_core::context_window_for_model("deepseek-chat");
        let _ = agent_core::autocompact_threshold_for_model("deepseek-chat");
    }
}
```

- [ ] **Step 3: 运行测试确认通过（确认依赖可用）**

```bash
cargo test -p agent-core presend_tests 2>&1
```

Expected: PASS（依赖已在 Task 1 中实现）。

- [ ] **Step 4: 在 chat_completions.rs 中添加 pre-send 压缩逻辑**

找到 `transcript_to_chat_messages(...)` 调用及随后构建 `body` 的位置，在 `let body = json!({...})` 之前插入：

```rust
// Pre-send: 检查 token 估算，必要时压缩历史，防止 API 400
{
    let model_name = request
        .model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| config.model.as_str());
    let threshold = autocompact_threshold_for_model(model_name);
    let mut compact_failures = 0u32;
    while estimate_messages_tokens(&messages) > threshold as usize {
        if compact_failures >= MAX_CONSECUTIVE_COMPACT_FAILURES {
            // 熔断：已无法继续压缩，继续发送
            emit_status(
                sink,
                &request.tab_id,
                "warning",
                "Context still over limit after max compact attempts. Proceeding anyway.",
            );
            break;
        }
        let did_compact = compact_chat_messages(&mut messages);
        if !did_compact {
            // 已无可压缩内容
            break;
        }
        compact_failures += 1;
    }
}
```

需要在文件顶部的 `use` 中添加：

```rust
use crate::config::{autocompact_threshold_for_model, MAX_CONSECUTIVE_COMPACT_FAILURES};
use crate::turn_engine::{compact_chat_messages, estimate_messages_tokens};
```

（如果已有其中一些 import，按需合并）

- [ ] **Step 5: 编译**

```bash
cargo build -p agent-core 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/providers/chat_completions.rs
git commit -m "feat(context): add pre-send compaction loop with model window awareness"
```

---

## Task 5: 同步 openai.rs

**Files:**
- Modify: `crates/agent-core/src/providers/openai.rs`

- [ ] **Step 1: 找到 openai.rs 中构建 messages 的位置**

```bash
grep -n "transcript_to_chat_messages\|let mut messages" crates/agent-core/src/providers/openai.rs | head -5
```

- [ ] **Step 2: 在同位置添加相同的 pre-send 压缩块**

与 Task 4 Step 4 完全相同的代码块，放置在 openai.rs 的相同位置。

```rust
// Pre-send: 检查 token 估算，必要时压缩历史
{
    let model_name = request
        .model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| config.model.as_str());
    let threshold = autocompact_threshold_for_model(model_name);
    let mut compact_failures = 0u32;
    while estimate_messages_tokens(&messages) > threshold as usize {
        if compact_failures >= MAX_CONSECUTIVE_COMPACT_FAILURES {
            emit_status(
                sink,
                &request.tab_id,
                "warning",
                "Context still over limit after max compact attempts. Proceeding anyway.",
            );
            break;
        }
        let did_compact = compact_chat_messages(&mut messages);
        if !did_compact {
            break;
        }
        compact_failures += 1;
    }
}
```

- [ ] **Step 3: 编译 + 运行所有测试**

```bash
cargo test -p agent-core 2>&1 | tail -10
cargo test -p agent-cli 2>&1 | tail -10
```

Expected: 全部 PASS。

- [ ] **Step 4: Final commit**

```bash
git add crates/agent-core/src/providers/openai.rs
git commit -m "feat(context): sync pre-send compaction to openai.rs provider"
```

---

## Task 6: P1 集成验证

- [ ] **Step 1: 运行全套测试**

```bash
cargo test -p agent-core -p agent-cli 2>&1 | grep -E "test result|FAILED"
```

Expected: 全部 PASS，无 FAILED。

- [ ] **Step 2: 验证 context window 映射覆盖常用模型**

```bash
cargo test -p agent-core context_window_tests -p agent-core circuit_breaker_tests -p agent-core compact_returns 2>&1
```

Expected: 所有测试 PASS。

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "feat(p1): complete context engineering — pre-send compaction + circuit breaker"
```
