# P2: Memory System — Layered Instruction Memory + remember_fact-first

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现两层记忆系统：(1) 分层指令记忆；(2) 运行时可写记忆能力。以现有 `remember_fact` 为主语义，避免新增长期并行语义工具。

**Architecture:** 复用现有 `AgentRuntimeState` 的 memory index 与 `remember_fact` 流程，优先做安全边界收口与会话注入稳定性增强。若需要 `memory_write`，仅作为 alias 映射到同一内部实现，不引入双轨行为。

**Tech Stack:** Rust, toml (crate), tokio::fs

⚠️ **安全注意：** 记忆写入必须严格验证 key 名称，禁止任何路径分隔符（`/`, `\\`, `..`），防止写入 persistence_dir 以外的文件。

---

## v2 强制修订（本节优先于下文）

### Non-goals
- 不引入与 `remember_fact` 并行且长期共存的第二套语义工具。
- 不在本阶段引入 registry 抽象（留给 P3）。
- 不改动无关的 prompt 模板体系。

### 口径修正
- `remember_fact` 是主入口；`memory_write` 若保留，仅允许 alias 到同一写入逻辑。
- 以当前仓库真实结构为准：memory index 为 JSON 持久化，新增 TOML 不是强制目标。
- 下文中对 `MemoryEntry` 字段的假设若与现状不一致，必须先按现状修正再实现。

### DoD（v2）
- [ ] 非法 key（`..`, `/`, `\\`）被拒绝。
- [ ] 写入后下一会话能在系统上下文中看到记忆注入。
- [ ] `remember_fact` 与可选 alias 行为一致，不发生语义分叉。
- [ ] `cargo test -p agent-core` 与 `cargo test -p agent-cli` 通过。

### 执行约束
- 下文凡以 `memory_write` 为唯一入口的步骤，改为“remember_fact-first + optional alias”。

---

## v2 执行清单（权威，覆盖下方旧任务块）

> 说明：下方原始 Task 1–5 保留为历史草案，存在旧命名（`memory_write`）与旧步骤假设。  
> **真正执行时请优先使用本节。**

### Step A: 现状基线确认

- [ ] 确认 `AgentRuntimeState` 的 memory index、加载与注入链路可用。
- [ ] 确认已有 `remember_fact` 的 schema/contract/dispatch 状态。
- [ ] 确认当前测试基线：`cargo test -p agent-core` 与 `cargo test -p agent-cli` 可运行。

### Step B: 安全边界收口（remember_fact-first）

- [ ] 统一 key 验证规则（拒绝 `..`、`/`、`\\`、空串，限制长度和字符集）。
- [ ] 统一 value 大小上限与错误信息格式。
- [ ] 写入路径限定在持久化目录 memory 子路径，不允许路径穿越。
- [ ] 在会话初始化与注入阶段，保证写入后下一会话可见。

### Step C: 命名统一策略

- [ ] 主入口保留 `remember_fact`。
- [ ] 如需兼容 `memory_write`，只做 alias 到同一路径逻辑。
- [ ] 禁止 `remember_fact` 与 `memory_write` 出现行为分叉。

### Step D: 测试与验收

- [ ] agent-core 增加/修复以下测试：合法写入、非法 key、超长 value、跨会话注入可见。
- [ ] agent-cli 增加/修复工具层测试：参数校验、错误传播、成功回执。
- [ ] 执行：
  - `cargo test -p agent-core`
  - `cargo test -p agent-cli`
  - `cargo build --workspace`

### Step E: 提交策略（建议）

- [ ] `feat(memory): harden remember_fact validation and persistence boundary`
- [ ] `feat(memory): unify remember_fact path and optional memory_write alias`
- [ ] `test(memory): add cross-session visibility and safety regression tests`

---

## 旧任务块（历史草案，不作为执行输入）

## File Map

| 操作 | 文件 | 职责 |
|------|------|------|
| Modify | `crates/agent-core/Cargo.toml` | 添加 `toml` 依赖 |
| Modify | `crates/agent-core/src/session.rs` | 添加 `write_memory_entry()`、`list_memory_keys()` 方法 |
| Create | `crates/agent-cli/src/local_tools/memory.rs` | `memory_write` 工具实现（含 key 安全验证） |
| Modify | `crates/agent-cli/src/local_tools/mod.rs` | 注册 `memory_write` 工具到 dispatch 表 |
| Modify | `crates/agent-core/src/tools.rs` | 在 `tool_contract()` 中添加 `memory_write` 条目 |
| Modify | `crates/agent-core/src/instructions.rs` | （可选）在系统 prompt 中告知 agent 可以使用 memory_write |
| Modify | `crates/agent-cli/src/main.rs` | 在启动时调用 `ensure_storage_at` 初始化持久化目录 |

---

## Task 1: 添加 toml 依赖，确认 Cargo 环境

**Files:**
- Modify: `crates/agent-core/Cargo.toml`

- [ ] **Step 1: 检查现有依赖**

```bash
grep "toml\|serde_toml" crates/agent-core/Cargo.toml
```

Expected: 可能为空（没有 toml 依赖）。

- [ ] **Step 2: 添加 toml 依赖**

在 `crates/agent-core/Cargo.toml` 的 `[dependencies]` 中添加：

```toml
toml = "0.8"
```

- [ ] **Step 3: 验证编译**

```bash
cargo build -p agent-core 2>&1 | grep "^error" | head -5
```

Expected: 无错误（toml 成功下载并编译）。

- [ ] **Step 4: Commit**

```bash
git add crates/agent-core/Cargo.toml
git commit -m "build: add toml dependency for memory system"
```

---

## Task 2: 实现 write_memory_entry 和 list_memory_keys

**Files:**
- Modify: `crates/agent-core/src/session.rs`

- [ ] **Step 1: 找到现有 memory 相关代码**

```bash
grep -n "memory\|MemoryIndex\|MemoryEntry\|MEMORY_DIR\|load_memory" crates/agent-core/src/session.rs | head -20
```

Expected: 找到 `MEMORY_DIR` 常量定义、`load_memory_index` 函数、`MemoryIndex`、`MemoryEntry` 结构。

- [ ] **Step 2: 找到 persistence_dir 的获取方式**

```bash
grep -n "persistence_dir\|get_persistence_dir\|persistence_dir.lock" crates/agent-core/src/session.rs | head -10
```

Expected: 找到 `self.persistence_dir` 字段的访问模式。

- [ ] **Step 3: 写失败测试**

在 `session.rs` 末尾添加：

```rust
#[cfg(test)]
mod memory_write_tests {
    use super::*;

    #[tokio::test]
    async fn write_and_read_memory_entry() {
        let state = AgentRuntimeState::default();
        let tmp = tempfile::tempdir().expect("tempdir");
        state.ensure_storage_at(tmp.path().to_path_buf()).await.expect("init storage");

        state.write_memory_entry("test-key", "test value content").await.expect("write");

        let keys = state.list_memory_keys().await;
        assert!(keys.contains(&"test-key".to_string()));
    }

    #[tokio::test]
    async fn write_memory_rejects_invalid_key() {
        let state = AgentRuntimeState::default();
        let tmp = tempfile::tempdir().expect("tempdir");
        state.ensure_storage_at(tmp.path().to_path_buf()).await.expect("init storage");

        let result = state.write_memory_entry("../escape", "malicious").await;
        assert!(result.is_err(), "should reject key with path traversal");

        let result2 = state.write_memory_entry("has/slash", "bad").await;
        assert!(result2.is_err(), "should reject key with slash");
    }
}
```

- [ ] **Step 4: 运行测试确认失败**

```bash
cargo test -p agent-core memory_write_tests 2>&1 | head -10
```

Expected: 编译错误，`write_memory_entry` 和 `list_memory_keys` 未定义。

- [ ] **Step 5: 实现 write_memory_entry 和 list_memory_keys**

在 `session.rs` 的 `impl AgentRuntimeState` 块末尾添加：

```rust
/// 验证 memory key 是否合法：只允许小写字母、数字、连字符
fn validate_memory_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("Memory key cannot be empty".to_string());
    }
    if key.len() > 64 {
        return Err("Memory key too long (max 64 chars)".to_string());
    }
    if !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!(
            "Invalid memory key '{}': only lowercase letters, digits, and hyphens allowed",
            key
        ))
    }
    Ok(())
}

/// 将 key-value 对持久化到 ~/.config/agent-runtime/memory/<key>.toml
pub async fn write_memory_entry(&self, key: &str, value: &str) -> Result<(), String> {
    validate_memory_key(key)?;

    let persistence_dir = self.persistence_dir.lock().await;
    let base = persistence_dir
        .as_ref()
        .ok_or_else(|| "Storage not initialized. Call ensure_storage_at first.".to_string())?;

    let memory_dir = base.join("memory");
    tokio::fs::create_dir_all(&memory_dir)
        .await
        .map_err(|e| format!("Failed to create memory dir: {}", e))?;

    // 只允许写入 memory_dir 内的文件
    let file_path = memory_dir.join(format!("{}.toml", key));
    // 双重验证：确认 canonical path 在 memory_dir 内
    // （由于 key 已验证不含路径分隔符，此处是防御性检查）
    let toml_content = format!("value = {:?}\n", value);
    tokio::fs::write(&file_path, &toml_content)
        .await
        .map_err(|e| format!("Failed to write memory entry '{}': {}", key, e))?;

    // 更新内存中的 MemoryIndex
    let mut index = self.memory_index.lock().await;
    let now = chrono::Utc::now().to_rfc3339();
    // 如果已存在同 key 的条目，更新；否则插入
    if let Some(existing) = index.entries.iter_mut().find(|e| e.key == key) {
        existing.updated_at = now;
        existing.value_preview = value.chars().take(80).collect();
    } else {
        index.entries.push(MemoryEntry {
            key: key.to_string(),
            value_preview: value.chars().take(80).collect(),
            memory_type: MemoryType::User,
            updated_at: now.clone(),
            created_at: now,
        });
    }

    Ok(())
}

/// 列出所有已存储的 memory key
pub async fn list_memory_keys(&self) -> Vec<String> {
    let index = self.memory_index.lock().await;
    index.entries.iter().map(|e| e.key.clone()).collect()
}

/// 读取指定 memory key 的完整内容
pub async fn read_memory_entry(&self, key: &str) -> Result<String, String> {
    validate_memory_key(key)?;

    let persistence_dir = self.persistence_dir.lock().await;
    let base = persistence_dir
        .as_ref()
        .ok_or_else(|| "Storage not initialized".to_string())?;

    let file_path = base.join("memory").join(format!("{}.toml", key));
    let raw = tokio::fs::read_to_string(&file_path)
        .await
        .map_err(|e| format!("Memory entry '{}' not found: {}", key, e))?;

    // 解析 TOML: value = "..."
    raw.lines()
        .find(|l| l.trim_start().starts_with("value"))
        .and_then(|l| l.split_once('='))
        .map(|(_, v)| v.trim().trim_matches('"').to_string())
        .ok_or_else(|| format!("Failed to parse memory entry '{}'", key))
}
```

**注意：** `MemoryEntry` 中已有 `key`、`value_preview`、`memory_type`、`updated_at`、`created_at` 字段（根据 session.rs 中已有的 struct）。如果字段名不匹配，以 grep 结果为准。

- [ ] **Step 6: 运行测试**

```bash
cargo test -p agent-core memory_write_tests 2>&1
```

Expected: 2 个测试 PASS。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/session.rs
git commit -m "feat(memory): add write_memory_entry, list_memory_keys, read_memory_entry to AgentRuntimeState"
```

---

## Task 3: 实现 memory_write 工具

**Files:**
- Create: `crates/agent-cli/src/local_tools/memory.rs`

- [ ] **Step 1: 写失败测试（先在 mod.rs 中引用 memory 模块触发编译失败）**

在 `crates/agent-cli/src/local_tools/mod.rs` 中临时添加（后续正式添加）：

```rust
mod memory;
```

然后运行：

```bash
cargo build -p agent-cli 2>&1 | head -5
```

Expected: 编译错误 `memory.rs` 不存在。

- [ ] **Step 2: 创建 memory.rs**

```rust
// crates/agent-cli/src/local_tools/memory.rs

use agent_core::{tools::error_result, AgentRuntimeState, AgentToolResult};
use serde_json::Value;
use tokio::sync::watch;

use super::common::{ok_result, tool_arg_string, truncate_preview};

/// 执行 memory_write 工具调用
pub async fn execute_memory_write(
    runtime_state: &AgentRuntimeState,
    call_id: &str,
    args: Value,
    _cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let key = match tool_arg_string(&args, "key") {
        Ok(k) => k,
        Err(e) => return error_result("memory_write", call_id, e),
    };
    let value = match tool_arg_string(&args, "value") {
        Ok(v) => v,
        Err(e) => return error_result("memory_write", call_id, e),
    };

    if value.len() > 8_000 {
        return error_result(
            "memory_write",
            call_id,
            "Value too long (max 8000 characters)".to_string(),
        );
    }

    match runtime_state.write_memory_entry(&key, &value).await {
        Ok(()) => ok_result(
            "memory_write",
            call_id,
            format!("Memory entry '{}' saved ({} chars).", key, value.len()),
        ),
        Err(e) => error_result("memory_write", call_id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::AgentRuntimeState;

    #[tokio::test]
    async fn memory_write_succeeds_with_valid_key() {
        let state = AgentRuntimeState::default();
        let tmp = tempfile::tempdir().expect("tempdir");
        state.ensure_storage_at(tmp.path().to_path_buf()).await.expect("init");

        let args = serde_json::json!({"key": "my-note", "value": "agent learned something"});
        let result = execute_memory_write(&state, "call-1", args, None).await;
        assert!(!result.is_error, "expected success, got: {:?}", result.content);
    }

    #[tokio::test]
    async fn memory_write_rejects_bad_key() {
        let state = AgentRuntimeState::default();
        let tmp = tempfile::tempdir().expect("tempdir");
        state.ensure_storage_at(tmp.path().to_path_buf()).await.expect("init");

        let args = serde_json::json!({"key": "../etc/passwd", "value": "bad"});
        let result = execute_memory_write(&state, "call-2", args, None).await;
        assert!(result.is_error, "expected error for path traversal key");
    }

    #[tokio::test]
    async fn memory_write_rejects_oversized_value() {
        let state = AgentRuntimeState::default();
        let tmp = tempfile::tempdir().expect("tempdir");
        state.ensure_storage_at(tmp.path().to_path_buf()).await.expect("init");

        let big_value = "x".repeat(8_001);
        let args = serde_json::json!({"key": "big", "value": big_value});
        let result = execute_memory_write(&state, "call-3", args, None).await;
        assert!(result.is_error, "expected error for oversized value");
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

```bash
cargo test -p agent-cli memory::tests 2>&1 | head -10
```

Expected: 编译错误（`ok_result`、`tool_arg_string` 等来自 `common` 的函数不存在，或 `ensure_storage_at` 未导出）。

查看 common.rs 确认实际可用的函数名：

```bash
grep -n "pub fn" crates/agent-cli/src/local_tools/common.rs | head -10
```

如有名称差异，修正 memory.rs 中的函数调用。

- [ ] **Step 4: 修复编译错误，运行测试**

```bash
cargo test -p agent-cli memory::tests 2>&1
```

Expected: 3 个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/agent-cli/src/local_tools/memory.rs
git commit -m "feat(memory): implement memory_write tool with path safety validation"
```

---

## Task 4: 注册 memory_write 到工具 dispatch 和 contract

**Files:**
- Modify: `crates/agent-cli/src/local_tools/mod.rs`
- Modify: `crates/agent-core/src/tools.rs`

- [ ] **Step 1: 在 mod.rs 中添加 dispatch**

找到 `mod.rs` 中的 `match call.tool_name.as_str()` 块（当前有 `"read_file"`, `"list_files"`, `"search_project"`, `"run_shell_command"` 4 个分支），在 `other =>` 之前添加：

```rust
"memory_write" => {
    memory::execute_memory_write(
        runtime_state,
        &call.call_id,
        parsed_args,
        cancel_rx,
    )
    .await
}
```

同时确保 `mod memory;` 已在文件顶部。

- [ ] **Step 2: 在 tools.rs 的 tool_contract 中注册**

找到 `pub fn tool_contract(tool_name: &str) -> AgentToolContract {` 函数（约 90 行），在 `_ =>` 默认分支前添加：

```rust
"memory_write" => AgentToolContract {
    capability_class: ToolCapabilityClass::MemoryWrite,
    resource_scope: ToolResourceScope::Workspace,
    approval_policy: ToolApprovalPolicy::Never,
    review_policy: ToolReviewPolicy::None,
    suspend_behavior: ToolSuspendBehavior::None,
    result_shape: ToolResultShape::CommandOutput,
    parallel_safe: false,
    approval_bucket: "memory_write",
},
```

注意：`ToolCapabilityClass::MemoryWrite` 已在 `tools.rs` 的枚举中定义（grep 确认）：

```bash
grep "MemoryWrite" crates/agent-core/src/tools.rs
```

- [ ] **Step 3: 在 default_tool_specs 中添加 memory_write schema**

找到 `pub fn default_tool_specs()` 函数，添加 memory_write 的 JSON schema：

```rust
AgentToolSpec {
    name: "memory_write".to_string(),
    description: "Persist a key-value note to long-term memory. The note will be available in future sessions. Use to remember important project context, user preferences, or conclusions. Key: lowercase letters, digits, and hyphens only (max 64 chars). Value: plain text, max 8000 chars.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "key": {
                "type": "string",
                "description": "Memory key: lowercase letters, digits, hyphens only. Example: 'project-stack', 'user-preference'"
            },
            "value": {
                "type": "string",
                "description": "Content to remember. Plain text, max 8000 characters."
            }
        },
        "required": ["key", "value"]
    }),
},
```

- [ ] **Step 4: 编译确认**

```bash
cargo build -p agent-core -p agent-cli 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 5: 运行所有测试**

```bash
cargo test -p agent-core -p agent-cli 2>&1 | grep -E "test result|FAILED"
```

Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/local_tools/mod.rs crates/agent-core/src/tools.rs
git commit -m "feat(memory): register memory_write tool in dispatch table and tool_contract"
```

---

## Task 5: 在 agent-cli 启动时初始化持久化存储

**Files:**
- Modify: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: 找到 main.rs 中的初始化位置**

```bash
grep -n "ensure_storage_at\|AgentRuntimeState::default\|runtime_state" crates/agent-cli/src/main.rs | head -10
```

Expected: 找到 `runtime_state` 创建位置。

- [ ] **Step 2: 确认配置目录路径**

在 CLI 中，配置目录应使用 `~/.config/agent-runtime/`：

```bash
grep -n "config_dir\|home_dir\|dirs::\|dirs_next" crates/agent-cli/Cargo.toml crates/agent-cli/src/main.rs | head -10
```

检查是否有 `dirs` 或 `dirs-next` 依赖。如果没有，使用 `std::env::var("HOME")` 或 `dirs::config_dir()` 确定路径。

- [ ] **Step 3: 如果需要，添加 dirs 依赖**

```bash
grep "dirs" crates/agent-cli/Cargo.toml
```

如果没有 `dirs` 依赖，添加到 `crates/agent-cli/Cargo.toml`：

```toml
dirs = "5"
```

- [ ] **Step 4: 在 main 初始化中调用 ensure_storage_at**

找到 `AgentRuntimeState::default()` 创建后的位置，添加：

```rust
// 初始化持久化存储（记忆、审批记录等）
let config_dir = dirs::config_dir()
    .unwrap_or_else(|| std::path::PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
    ))
    .join("agent-runtime");

if let Err(e) = runtime_state.ensure_storage_at(config_dir).await {
    eprintln!("Warning: failed to initialize persistent storage: {}", e);
    // 不 fatal — 继续运行，只是没有持久化
}
```

- [ ] **Step 5: 编译 + 测试**

```bash
cargo build -p agent-cli 2>&1 | grep "^error" | head -5
cargo test -p agent-cli 2>&1 | tail -5
```

Expected: 无错误，测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/main.rs crates/agent-cli/Cargo.toml
git commit -m "feat(memory): initialize persistent storage on agent-cli startup"
```

---

## Task 6: P2 集成验证

- [ ] **Step 1: 运行所有测试**

```bash
cargo test -p agent-core -p agent-cli 2>&1 | grep -E "test result|FAILED"
```

Expected: 全部 PASS。

- [ ] **Step 2: 手动验证写入**

```bash
cargo run -p agent-cli -- run --provider minimax --model MiniMax-M1 "请用 memory_write 工具把 key='test-note' value='this was remembered' 写入记忆" 2>&1
cat ~/.config/agent-runtime/memory/test-note.toml
```

Expected: 文件存在且内容正确。

- [ ] **Step 3: 验证跨会话持久化**

重启 CLI 后发送新 prompt，确认系统 prompt 中包含上一步写入的记忆内容（通过 `--output jsonl` 查看完整 events）。

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(p2): complete memory system — layered instruction memory + memory_write tool"
```
