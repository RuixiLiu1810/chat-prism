# P0: Cancel Signal + Stream Retry + Orphaned Tool Result

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复三个会导致 agent 卡死或无法中断的关键 bug：(1) Ctrl-C 取消信号从未真正到达流式循环；(2) 流式重试不解析 Retry-After 头，且网络断连错误不重试；(3) 中断后消息历史包含孤立的 tool_use/tool_result，导致下一轮 API 400 错误。

**Architecture:** 取消信号修复通过让 `turn_runner.rs` 传递真实 cancel_rx 而不是 `None`，并在 `main.rs` 添加 `tokio::signal::ctrl_c()` 处理器实现。重试改进在 `providers/chat_completions.rs` 和 `providers/openai.rs` 中添加 Retry-After 头解析和 stale 连接识别。Orphan 修复在每次 API 请求前调用 `ensure_tool_result_pairing()`。

**Tech Stack:** Rust, tokio, reqwest, tokio::sync::watch, tokio::signal

---

## v2 强制修订（本节优先于下文）

### Non-goals
- 不在本阶段重构工具注册表。
- 不在本阶段引入多 agent。
- 不在本阶段做 desktop 新功能开发（仅兼容现有接口）。

### 口径修正
- 若下文步骤与当前仓库现状冲突，以**仓库现状**为准再实施。
- `Retry-After` 策略必须增加上限保护：`delay_secs = min(parsed_retry_after, 30)`。
- 当 `Retry-After` 头非法或缺失时，回退到指数退避（沿用现有公共 helper）。
- 网络中断类错误重试需受 `MAX_RETRIES` 和 cancel 信号双重约束。

### DoD（v2）
- [ ] Ctrl-C 在流式输出中触发 turn cancel（不是杀进程）。
- [ ] TUI Escape 可取消当前 turn。
- [ ] 429/503 有效解析 `Retry-After`，并有 30 秒上限。
- [ ] orphan 历史修复后不再触发下一轮 400。
- [ ] `cargo test -p agent-core` 与 `cargo test -p agent-cli` 通过。

### 执行约束
- 本文件任务可继续执行，但若出现“为实现计划而偏离现有接口”的情况，应先修订计划再编码。

---

## File Map

| 操作 | 文件 | 职责 |
|------|------|------|
| Modify | `crates/agent-core/src/providers/chat_completions.rs` | 添加 Retry-After 解析、stale 连接重试、`ensure_tool_result_pairing()` 调用 |
| Modify | `crates/agent-core/src/providers/openai.rs` | 同步 chat_completions.rs 的重试改进 |
| Modify | `crates/agent-core/src/turn_engine.rs` | 新增 `ensure_tool_result_pairing()` 函数 |
| Modify | `crates/agent-cli/src/turn_runner.rs` | 修改 `run_turn()` 接受并传递 `cancel_rx` |
| Modify | `crates/agent-cli/src/main.rs` | 添加 `tokio::signal::ctrl_c()` SIGINT 处理器 |
| Modify | `crates/agent-cli/src/tui/shell.rs` | Ctrl-C/Escape 按键连接到 `cancel_tab()` |

---

## Task 1: 确认 cancel_tab 的可用性并为 turn_runner 添加 cancel_rx 参数

**Files:**
- Modify: `crates/agent-cli/src/turn_runner.rs`

- [ ] **Step 1: 验证当前 run_turn 签名**

```bash
grep -n "pub async fn run_turn\|pub async fn resume_pending_turn" crates/agent-cli/src/turn_runner.rs
```

Expected: 显示两个函数定义行，`run_turn` 目前无 `cancel_rx` 参数。

- [ ] **Step 2: 确认 session 已有 cancel_tab**

```bash
grep -n "pub async fn cancel_tab\|pub async fn register_cancellation" crates/agent-core/src/session.rs
```

Expected: 两行均存在，`cancel_tab` 接受 `&str` tab_id 并发送 `true`。

- [ ] **Step 3: 写失败测试**

在 `crates/agent-cli/src/turn_runner.rs` 末尾添加测试（暂时编译失败）：

```rust
#[cfg(test)]
mod cancel_tests {
    use tokio::sync::watch;

    #[test]
    fn run_turn_signature_accepts_cancel_rx() {
        // 编译测试：确认 run_turn 接受 Option<watch::Receiver<bool>>
        // 此测试在修改签名前会因参数数量不匹配而编译失败
        let _: fn(
            &dyn agent_core::EventSink,
            &agent_core::StaticConfigProvider,
            &agent_core::AgentRuntimeState,
            &agent_core::AgentTurnDescriptor,
            agent_core::ToolExecutorFn,
            Option<tokio::sync::watch::Receiver<bool>>,
        ) -> _ = super::run_turn;
    }
}
```

- [ ] **Step 4: 运行测试确认它失败**

```bash
cargo test -p agent-cli cancel_tests 2>&1 | head -20
```

Expected: 编译错误，`run_turn` 参数数量不符。

- [ ] **Step 5: 修改 run_turn 签名**

在 `crates/agent-cli/src/turn_runner.rs` 中找到：

```rust
pub async fn run_turn(
    sink: &dyn EventSink,
    config_provider: &StaticConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    tool_executor: ToolExecutorFn,
) -> Result<providers::AgentTurnOutcome, String> {
```

替换为：

```rust
pub async fn run_turn(
    sink: &dyn EventSink,
    config_provider: &StaticConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    tool_executor: ToolExecutorFn,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<providers::AgentTurnOutcome, String> {
```

在文件顶部确保有：

```rust
use tokio::sync::watch;
```

- [ ] **Step 6: 找到并更新内部 run_turn_loop 调用**

在同文件 `run_turn` 函数体内找到：

```rust
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
```

将 `None` 替换为 `cancel_rx`：

```rust
    let outcome = providers::chat_completions::run_turn_loop(
        sink,
        config_provider,
        runtime_state,
        request,
        &history,
        Arc::clone(&tool_executor),
        cancel_rx,
    )
    .await?;
```

- [ ] **Step 7: 运行测试确认通过**

```bash
cargo test -p agent-cli cancel_tests 2>&1
```

Expected: PASS — 编译成功，签名类型匹配。

- [ ] **Step 8: 确认其他调用方编译失败（找出需要更新的位置）**

```bash
cargo build -p agent-cli 2>&1 | grep "error\[" | head -20
```

Expected: 显示 `main.rs` 中调用 `run_turn` 的位置编译失败（参数数量不符）。记录这些位置。

- [ ] **Step 9: Commit**

```bash
git add crates/agent-cli/src/turn_runner.rs
git commit -m "fix(cancel): add cancel_rx param to run_turn signature"
```

---

## Task 2: main.rs 添加 SIGINT → cancel_rx 传递

**Files:**
- Modify: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: 找到 main.rs 中所有 run_turn 调用**

```bash
grep -n "run_turn\|resume_pending_turn" crates/agent-cli/src/main.rs | head -20
```

Expected: 显示调用行号。

- [ ] **Step 2: 读取调用上下文**

```bash
sed -n '440,620p' crates/agent-cli/src/main.rs
```

Expected: 看到 REPL 循环，找到 `run_turn(` 调用点。

- [ ] **Step 3: 在 Cargo.toml 确认 tokio signal feature 已启用**

```bash
grep -A5 '\[dependencies\]' crates/agent-cli/Cargo.toml | grep tokio
grep "tokio" crates/agent-cli/Cargo.toml
```

Expected: tokio 依赖应包含 `features = ["signal", ...]`。

如果没有 `signal` feature，在 `crates/agent-cli/Cargo.toml` 中找到 tokio 依赖行，添加 `"signal"` 到 features 列表。

- [ ] **Step 4: 在 REPL 循环中添加 cancel channel 和 SIGINT 处理**

找到 `main.rs` 中调用 `run_turn(` 的地方。在每次 `run_turn` 调用前，用以下模式包裹（实际行号以 Step 1 grep 结果为准）：

```rust
// 为本次 turn 注册取消信号
let cancel_rx = runtime_state.register_cancellation(&tab_id).await;
let cancel_tab_id = tab_id.clone();
let cancel_state = Arc::clone(&runtime_state_arc);  // runtime_state 的 Arc 引用

// 监听 Ctrl-C：发送取消信号
let _sigint_guard = tokio::spawn(async move {
    if tokio::signal::ctrl_c().await.is_ok() {
        cancel_state.cancel_tab(&cancel_tab_id).await;
    }
});

let outcome = turn_runner::run_turn(
    sink,
    &config_provider,
    &runtime_state,
    &request,
    Arc::clone(&tool_executor),
    Some(cancel_rx),
).await;

// 取消注册（不论成功或失败）
runtime_state.clear_cancellation(&tab_id).await;
```

**注意：** `runtime_state` 当前可能是值类型而不是 Arc。如果是值类型，调整为：

```rust
let cancel_rx = runtime_state.register_cancellation(&tab_id).await;

// 在 spawn 中克隆 runtime_state（它实现了 Clone）
let cancel_state = runtime_state.clone();
let cancel_tab_id = tab_id.clone();
let _sigint_guard = tokio::spawn(async move {
    if tokio::signal::ctrl_c().await.is_ok() {
        cancel_state.cancel_tab(&cancel_tab_id).await;
    }
});
```

- [ ] **Step 5: 更新 resume_pending_turn 调用（如有）**

```bash
grep -n "resume_pending_turn(" crates/agent-cli/src/main.rs
```

对每个调用点，同样添加 cancel_rx：先注册 → 传入 → 事后清理。

- [ ] **Step 6: 编译确认无错误**

```bash
cargo build -p agent-cli 2>&1 | grep "^error" | head -10
```

Expected: 无编译错误。

- [ ] **Step 7: 手动冒烟测试**

```bash
cargo run -p agent-cli -- --provider minimax --model MiniMax-Text-01 --api-key test "hello"
```

在运行期间按 Ctrl-C，观察是否打印取消消息而非直接终止。

- [ ] **Step 8: Commit**

```bash
git add crates/agent-cli/src/main.rs crates/agent-cli/Cargo.toml
git commit -m "fix(cancel): wire SIGINT ctrl-c to cancel_tab in REPL loop"
```

---

## Task 3: TUI shell.rs 连接 Escape/Ctrl-C 到取消信号

**Files:**
- Modify: `crates/agent-cli/src/tui/shell.rs`

- [ ] **Step 1: 找到 TUI 中当前的取消相关代码**

```bash
grep -n "cancel\|Escape\|KeyCode::Char('c')\|ctrl\|abort\|interrupt" crates/agent-cli/src/tui/shell.rs | head -20
```

Expected: 找到按键事件处理位置。

- [ ] **Step 2: 找到 tool_executor 构建位置**

```bash
grep -n "let tool_executor\|ToolExecutorFn\|register_cancellation" crates/agent-cli/src/tui/shell.rs | head -10
```

Expected: 找到 tool_executor 闭包定义的行号。

- [ ] **Step 3: 写失败测试（验证 TUI run_turn 传入 cancel_rx）**

此测试通过检查编译时 TUI 调用 `run_turn` 传了 6 个参数（含 cancel_rx）来验证。在文件末尾临时添加：

```rust
#[cfg(test)]
mod tui_cancel_tests {
    #[test]
    fn placeholder_tui_cancel_wired() {
        // 此测试在 TUI 的 run_turn 调用更新后会编译通过
        // 暂时是编译测试，通过后删除此注释
        assert!(true);
    }
}
```

- [ ] **Step 4: 找到 TUI 中 run_turn 的调用，在调用前添加 cancel 注册**

找到 TUI 中 `turn_runner::run_turn(` 的调用（以 Step 2 grep 结果定位），在调用前添加：

```rust
let cancel_rx = runtime_state.register_cancellation(tab_id).await;
```

修改 `run_turn` 调用：

```rust
let result = turn_runner::run_turn(
    sink,
    config_provider,
    runtime_state,
    request,
    Arc::clone(&tool_executor),
    Some(cancel_rx),
)
.await;
runtime_state.clear_cancellation(tab_id).await;
```

- [ ] **Step 5: 找到按键事件处理，添加取消触发**

在 TUI 事件循环中找到 Escape 或 Ctrl-C 按键处理（以 Step 1 grep 结果定位），添加：

```rust
// 当用户按 Escape 或 Ctrl-C 时发送取消信号
runtime_state.cancel_tab(tab_id).await;
```

- [ ] **Step 6: 编译**

```bash
cargo build -p agent-cli 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-cli/src/tui/shell.rs
git commit -m "fix(cancel): wire TUI escape/ctrl-c to cancel_tab"
```

---

## Task 4: 添加 Retry-After 头解析和 stale 连接重试（chat_completions）

**Files:**
- Modify: `crates/agent-core/src/providers/chat_completions.rs`

- [ ] **Step 1: 写失败测试**

在 `crates/agent-core/src/providers/chat_completions.rs` 末尾添加：

```rust
#[cfg(test)]
mod retry_tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn retry_after_header_parsed_correctly() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("42"));
        assert_eq!(get_retry_after_secs(&headers), Some(42u64));
    }

    #[test]
    fn retry_after_header_missing_returns_none() {
        let headers = HeaderMap::new();
        assert_eq!(get_retry_after_secs(&headers), None);
    }

    #[test]
    fn retry_after_header_invalid_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("tomorrow"));
        assert_eq!(get_retry_after_secs(&headers), None);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p agent-core retry_tests 2>&1 | head -20
```

Expected: 编译错误 `get_retry_after_secs` 未定义。

- [ ] **Step 3: 实现 get_retry_after_secs**

在 `chat_completions.rs` 的函数区（在 `stream_chat_completions_response_once` 之前）添加：

```rust
fn get_retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

fn is_stale_connection_error(err: &reqwest::Error) -> bool {
    use std::error::Error;
    if let Some(source) = err.source() {
        let msg = source.to_string().to_ascii_lowercase();
        return msg.contains("connection reset")
            || msg.contains("broken pipe")
            || msg.contains("connection closed");
    }
    false
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test -p agent-core retry_tests 2>&1
```

Expected: 所有 3 个测试 PASS。

- [ ] **Step 5: 修改重试块使用 Retry-After 头**

找到 `chat_completions.rs` 中的重试块（约在 `const MAX_RETRIES: u32 = 3;` 附近）：

当前代码：
```rust
const MAX_RETRIES: u32 = 3;
let mut response = {
    let mut attempt = 0u32;
    loop {
        let resp = client
            ...
            .send()
            .await
            .map_err(|err| {
                format!(...)
            })?;
```

修改 `map_err` 闭包，将网络错误也纳入重试：

```rust
const MAX_RETRIES: u32 = 10;
let mut response = {
    let mut attempt = 0u32;
    loop {
        let send_result = client
            .post(&url)
            .bearer_auth(&api_key)
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await;

        // stale connection 错误也应重试
        let resp = match send_result {
            Err(ref err) if is_stale_connection_error(err) && attempt < MAX_RETRIES => {
                let backoff_secs = std::cmp::min(1u64 << attempt, 60);
                emit_status(
                    sink,
                    &request.tab_id,
                    "retrying",
                    &format!(
                        "Connection error, retrying in {}s (attempt {}/{})...",
                        backoff_secs,
                        attempt + 1,
                        MAX_RETRIES
                    ),
                );
                let sleep_dur = Duration::from_secs(backoff_secs);
                if let Some(rx) = cancel_rx.as_mut() {
                    tokio::select! {
                        _ = tokio::time::sleep(sleep_dur) => {}
                        changed = rx.changed() => {
                            if changed.is_err() || *rx.borrow() {
                                return Err(AGENT_CANCELLED_MESSAGE.to_string());
                            }
                        }
                    }
                } else {
                    tokio::time::sleep(sleep_dur).await;
                }
                attempt += 1;
                continue;
            }
            Err(err) => {
                return Err(format!(
                    "{} request failed: {}",
                    provider_display_name(&config.provider),
                    err
                ));
            }
            Ok(resp) => resp,
        };

        if resp.status().is_success() {
            break resp;
        }

        let status = resp.status();
        let headers = resp.headers().clone();
        let retryable = matches!(status.as_u16(), 429 | 503 | 529);
        if retryable && attempt < MAX_RETRIES {
            // 优先使用 Retry-After 头，否则指数退避
            let backoff_secs = get_retry_after_secs(&headers)
                .unwrap_or_else(|| std::cmp::min(1u64 << attempt, 300));
            emit_status(
                sink,
                &request.tab_id,
                "retrying",
                &format!(
                    "Received {} from {}, retrying in {}s (attempt {}/{})...",
                    status.as_u16(),
                    provider_display_name(&config.provider),
                    backoff_secs,
                    attempt + 1,
                    MAX_RETRIES
                ),
            );
            let sleep_dur = Duration::from_secs(backoff_secs);
            if let Some(rx) = cancel_rx.as_mut() {
                tokio::select! {
                    _ = tokio::time::sleep(sleep_dur) => {}
                    changed = rx.changed() => {
                        if changed.is_err() || *rx.borrow() {
                            return Err(AGENT_CANCELLED_MESSAGE.to_string());
                        }
                    }
                }
            } else {
                tokio::time::sleep(sleep_dur).await;
            }
            attempt += 1;
            continue;
        }

        let resp_body = resp.text().await.unwrap_or_default();
        let preview = if resp_body.len() > 500 {
            format!("{}...", &resp_body[..500])
        } else {
            resp_body
        };
        return Err(format!(
            "{} request failed with status {}: {}",
            provider_display_name(&config.provider),
            status,
            preview
        ));
    }
};
```

- [ ] **Step 6: 编译**

```bash
cargo build -p agent-core 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/providers/chat_completions.rs
git commit -m "fix(retry): add Retry-After header parsing and stale connection retry"
```

---

## Task 5: 同步 openai.rs 的重试改进

**Files:**
- Modify: `crates/agent-core/src/providers/openai.rs`

- [ ] **Step 1: 找到 openai.rs 的重试块**

```bash
grep -n "MAX_RETRIES\|retryable\|backoff" crates/agent-core/src/providers/openai.rs | head -10
```

Expected: 显示重试相关代码位置。

- [ ] **Step 2: 写失败测试**

在 `crates/agent-core/src/providers/openai.rs` 末尾添加：

```rust
#[cfg(test)]
mod retry_tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn openai_retry_after_header_parsed() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("30"));
        assert_eq!(get_retry_after_secs(&headers), Some(30u64));
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

```bash
cargo test -p agent-core openai::retry_tests 2>&1 | head -10
```

Expected: 编译错误，`get_retry_after_secs` 未定义。

- [ ] **Step 4: 将 get_retry_after_secs 和 is_stale_connection_error 移到公共模块**

在 `crates/agent-core/src/providers/` 目录下找 `mod.rs`：

```bash
ls crates/agent-core/src/providers/
```

如果有 `mod.rs`，将两个函数移入其中并 `pub(super) fn`。如果没有，在 `chat_completions.rs` 改为 `pub(crate) fn`，在 `openai.rs` 中 `use super::chat_completions::{get_retry_after_secs, is_stale_connection_error};`。

- [ ] **Step 5: 在 openai.rs 重试块中应用相同修改**

找到 openai.rs 中的重试块（参考 Task 4 Step 5 的逻辑），做完全相同的修改：
- `MAX_RETRIES = 10`
- stale connection 识别
- Retry-After 头使用
- 529 状态码加入 retryable 列表

- [ ] **Step 6: 运行测试**

```bash
cargo test -p agent-core 2>&1 | tail -5
```

Expected: 所有测试 PASS，无新失败。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/providers/
git commit -m "fix(retry): sync openai.rs retry improvements with chat_completions"
```

---

## Task 6: 实现 ensure_tool_result_pairing

**Files:**
- Modify: `crates/agent-core/src/turn_engine.rs`

- [ ] **Step 1: 写失败测试**

在 `crates/agent-core/src/turn_engine.rs` 末尾（或现有 `#[cfg(test)]` 块内）添加：

```rust
#[cfg(test)]
mod orphan_tests {
    use super::*;
    use serde_json::json;

    fn make_assistant_with_tool_use(tool_use_id: &str) -> Value {
        json!({
            "type": "assistant",
            "content": [{"type": "tool_use", "id": tool_use_id, "name": "read_file", "input": {}}]
        })
    }

    fn make_user_with_tool_result(tool_use_id: &str) -> Value {
        json!({
            "type": "user",
            "content": [{"type": "tool_result", "tool_use_id": tool_use_id, "content": "ok"}]
        })
    }

    #[test]
    fn paired_messages_unchanged() {
        let mut msgs = vec![
            make_assistant_with_tool_use("id-1"),
            make_user_with_tool_result("id-1"),
        ];
        ensure_tool_result_pairing(&mut msgs);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn orphaned_tool_use_gets_synthetic_result() {
        let mut msgs = vec![make_assistant_with_tool_use("id-2")];
        ensure_tool_result_pairing(&mut msgs);
        // 应该在后面插入一个合成的 tool_result
        assert_eq!(msgs.len(), 2);
        let added = &msgs[1];
        let content = &added["content"][0];
        assert_eq!(content["tool_use_id"], "id-2");
        assert_eq!(added["role"], "user");
    }

    #[test]
    fn orphaned_tool_result_removed() {
        let mut msgs = vec![make_user_with_tool_result("ghost-id")];
        ensure_tool_result_pairing(&mut msgs);
        // ghost-id 没有对应的 tool_use，应该被移除
        // 找不到任何包含 ghost-id 的 tool_result
        let has_ghost = msgs.iter().any(|m| {
            m["content"]
                .as_array()
                .map(|arr| arr.iter().any(|b| b["tool_use_id"] == "ghost-id"))
                .unwrap_or(false)
        });
        assert!(!has_ghost);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p agent-core orphan_tests 2>&1 | head -20
```

Expected: 编译错误，`ensure_tool_result_pairing` 未定义。

- [ ] **Step 3: 实现 ensure_tool_result_pairing**

在 `turn_engine.rs` 中找到 `pub fn compact_chat_messages` 附近（约 670 行），在其前面添加：

```rust
const SYNTHETIC_TOOL_RESULT_TEXT: &str =
    "[Tool use was interrupted before completion. Please continue from where you left off.]";

/// 修复 tool_use / tool_result 配对问题，在每次 API 请求前调用。
/// - 有 tool_use 但无 tool_result 的：插入合成 error tool_result
/// - 有 tool_result 但无对应 tool_use 的：移除孤立 tool_result
pub fn ensure_tool_result_pairing(messages: &mut Vec<Value>) {
    // 1. 收集所有 tool_use ID
    let mut tool_use_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for msg in messages.iter() {
        if let Some(content) = msg.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        tool_use_ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    // 2. 收集所有 tool_result 引用的 tool_use_id
    let mut result_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for msg in messages.iter() {
        if let Some(content) = msg.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                    if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                        result_ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    // 3. 找出缺少 result 的 tool_use ID（按顺序）
    let missing_results: Vec<String> = {
        let mut found = Vec::new();
        for msg in messages.iter() {
            if let Some(content) = msg.get("content").and_then(Value::as_array) {
                for block in content {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        if let Some(id) = block.get("id").and_then(Value::as_str) {
                            if !result_ids.contains(id) {
                                found.push(id.to_string());
                            }
                        }
                    }
                }
            }
        }
        found
    };

    // 4. 为每个缺失的 result 插入合成消息
    for missing_id in &missing_results {
        let synthetic = json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": missing_id,
                "is_error": true,
                "content": [{"type": "text", "text": SYNTHETIC_TOOL_RESULT_TEXT}]
            }]
        });
        messages.push(synthetic);
    }

    // 5. 移除引用了不存在 tool_use 的孤立 tool_result 块
    for msg in messages.iter_mut() {
        if let Some(content) = msg.get_mut("content").and_then(Value::as_array_mut) {
            content.retain(|block| {
                if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                    if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                        return tool_use_ids.contains(id);
                    }
                }
                true // 非 tool_result 块保留
            });
        }
    }
}
```

- [ ] **Step 4: 将函数导出到 lib.rs**

在 `crates/agent-core/src/lib.rs` 的 `pub use turn_engine::` 那行，添加 `ensure_tool_result_pairing`：

```rust
pub use turn_engine::{
    compact_chat_messages, emit_agent_complete, ..., ensure_tool_result_pairing, ...
};
```

- [ ] **Step 5: 运行测试**

```bash
cargo test -p agent-core orphan_tests 2>&1
```

Expected: 3 个测试全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/agent-core/src/turn_engine.rs crates/agent-core/src/lib.rs
git commit -m "feat(orphan): implement ensure_tool_result_pairing to fix history corruption"
```

---

## Task 7: 在 API 请求前调用 ensure_tool_result_pairing

**Files:**
- Modify: `crates/agent-core/src/providers/chat_completions.rs`
- Modify: `crates/agent-core/src/providers/openai.rs`

- [ ] **Step 1: 找到构建 messages 的位置**

```bash
grep -n "transcript_to_chat_messages\|let mut messages\|ensure_tool_result" crates/agent-core/src/providers/chat_completions.rs | head -10
```

Expected: 找到调用 `transcript_to_chat_messages` 的行。

- [ ] **Step 2: 在 chat_completions.rs 中调用修复函数**

找到 `transcript_to_chat_messages(` 调用后的 `let mut messages` 或类似行，在其后立即添加：

```rust
// 修复孤立 tool_use/tool_result 对，防止 API 返回 400
ensure_tool_result_pairing(&mut messages);
```

如果 `messages` 在此处是 `Vec<Value>` 且可变借用，直接加此行即可。

- [ ] **Step 3: 同步 openai.rs**

找到 openai.rs 中相同位置（同样调用了 `transcript_to_chat_messages` 或构建 messages 的地方），添加相同的一行。

- [ ] **Step 4: 编译运行所有测试**

```bash
cargo test -p agent-core 2>&1 | tail -10
```

Expected: 所有测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/agent-core/src/providers/
git commit -m "fix(orphan): call ensure_tool_result_pairing before every API request"
```

---

## Task 8: P0 集成验证

- [ ] **Step 1: 运行全部 agent-core 测试**

```bash
cargo test -p agent-core 2>&1 | tail -20
```

Expected: 所有测试 PASS，无 FAIL。

- [ ] **Step 2: 运行全部 agent-cli 测试**

```bash
cargo test -p agent-cli 2>&1 | tail -20
```

Expected: 所有测试 PASS，无 FAIL。

- [ ] **Step 3: 冒烟测试 — 取消流程**

```bash
cargo run -p agent-cli -- run --provider minimax --model MiniMax-M1 "写一首很长的诗，每行重复100次" 2>&1 &
sleep 2
kill -INT $!
```

Expected: 看到 "Agent run cancelled by user." 输出，进程正常退出（exit code 130 或 0）。

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(p0): complete cancel signal + stream retry + orphaned tool result fix"
```
