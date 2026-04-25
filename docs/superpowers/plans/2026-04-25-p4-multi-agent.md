# P4: Multi-Agent Coordinator — spawn_subagent Tool

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 agent-core 中实现 `spawn_subagent` 工具，使主 agent 能够将子任务委托给具有独立历史、独立 EventSink 和独立取消信号的子 agent 执行。子 agent 的输出作为 tool_result 返回给主 agent。限制：仅支持单级嵌套（子 agent 不能再 spawn 子 agent）。

**Architecture:**
- 新文件 `crates/agent-core/src/workflows/coordinator.rs` — 包含 `run_subagent_turn()` 函数，接受一个 prompt + tool_subset，以 `NullEventSink`（或 `BufferEventSink`）运行完整的 turn loop，返回 agent 最终响应文本
- `spawn_subagent` 工具实现在 `crates/agent-cli/src/local_tools/` 中，或作为通用工具注册到 `agent-core`（因为 coordinator 逻辑在 agent-core 中）
- 深度防护：通过传入标志位 `allow_spawn: bool` 到 run_turn_loop，子 agent 的工具集中不包含 `spawn_subagent`

**Tech Stack:** Rust, tokio, `crates/agent-core/src/workflows/`（现有模块）

---

## v2 强制修订（本节优先于下文）

### Non-goals
- 不支持递归 subagent。
- 不支持并行多子代理编排。
- 不扩展到 desktop 新 UI 行为。

### 口径修正
- `NullEventSink` 已在仓库存在并导出，本阶段不重复实现。
- `spawn_subagent` 必须为单层委托：子工具集显式不含 `spawn_subagent`。
- 若与 P3 registry 接口有差异，先以 P3 最终兼容桥接口为准再落地。
- 下文中涉及 `AgentTurnDescriptor` 字段名或 `outcome` 访问器的方法名，仅作实现草案；编码前必须以当前 `provider/session` 真实结构校准。

### DoD（v2）
- [ ] `run_subagent_turn` 可执行单次委托并返回文本结果。
- [ ] 子代理工具集不含 `spawn_subagent`，递归调用被硬性禁止。
- [ ] 父取消信号可传播到子代理并中断执行。
- [ ] `cargo test -p agent-core` 与 `cargo test -p agent-cli` 通过。

### 执行约束
- 本文件原 Task 1（NullEventSink）标记为“已完成基线能力”，可跳过。
- 从 coordinator 与 recursion guard 相关任务开始执行。

---

## File Map

| 操作 | 文件 | 职责 |
|------|------|------|
| Create | `crates/agent-core/src/workflows/coordinator.rs` | `run_subagent_turn()` 核心实现 |
| Modify | `crates/agent-core/src/event_sink.rs` | 复用已存在 `NullEventSink`（无需重复实现） |
| Modify | `crates/agent-core/src/workflows/mod.rs` | `pub mod coordinator;` |
| Modify | `crates/agent-core/src/lib.rs` | 导出 `run_subagent_turn` |
| Create | `crates/agent-cli/src/local_tools/subagent.rs` | `spawn_subagent` 工具实现 |
| Modify | `crates/agent-cli/src/local_tools/mod.rs` | 注册 `SpawnSubagentTool` |
| Modify | `crates/agent-core/src/tools.rs` | 添加 `spawn_subagent` 的 tool_contract 和 schema |

---

## Task 1: 实现 NullEventSink

**Files:**
- Modify: `crates/agent-core/src/event_sink.rs` （或 create null.rs）

> v2 说明：`NullEventSink` 已是现有能力。本 Task 改为“基线确认任务”，不再重复实现。

- [ ] **Step 1: 确认 EventSink 与 NullEventSink 已存在**

```bash
rg -n "pub trait EventSink|pub struct NullEventSink|impl EventSink for NullEventSink" crates/agent-core/src/event_sink.rs
```

Expected: 三者均存在。

- [ ] **Step 2: 确认 lib.rs 已导出 NullEventSink**

```bash
rg -n "pub use event_sink::\\{.*NullEventSink.*\\}|pub use event_sink::NullEventSink" crates/agent-core/src/lib.rs
```

Expected: 至少有一处导出匹配。

- [ ] **Step 3: 运行现有测试验证基线**

```bash
cargo test -p agent-core null_sink_does_not_panic -- --nocapture
```

Expected: PASS。

- [ ] **Step 4: Commit（可选，仅在本任务有实际改动时）**

```bash
git add crates/agent-core/src/event_sink.rs crates/agent-core/src/lib.rs
git commit -m "chore(coordinator): verify existing NullEventSink baseline"
```

---

## Task 2: 实现 run_subagent_turn

**Files:**
- Create: `crates/agent-core/src/workflows/coordinator.rs`
- Modify: `crates/agent-core/src/workflows/mod.rs`

- [ ] **Step 1: 查看 workflows 模块现状**

```bash
ls crates/agent-core/src/workflows/
cat crates/agent-core/src/workflows/mod.rs
```

Expected: 找到现有的 workflow 模块文件列表。

- [ ] **Step 2: 查看 run_turn_loop 函数签名**

```bash
grep -n "pub async fn run_turn_loop\|pub fn run_turn_loop" crates/agent-core/src/providers/chat_completions.rs | head -3
grep -n "pub async fn run_turn_loop\|pub fn run_turn_loop" crates/agent-core/src/providers/openai.rs | head -3
```

Expected: 找到完整的 run_turn_loop 签名（参数列表）。

- [ ] **Step 3: 写失败测试**

在 `crates/agent-core/src/workflows/coordinator.rs`（先创建空文件），或在 workflows/mod.rs 末尾添加：

```rust
#[cfg(test)]
mod coordinator_tests {
    use super::coordinator::*;
    use crate::config::StaticConfigProvider;
    use crate::session::AgentRuntimeState;
    use crate::ToolRegistry;

    #[tokio::test]
    async fn subagent_returns_text_result() {
        // 此为集成测试框架，实际运行需要真实 API key
        // 单元测试：验证函数签名存在并可调用
        let _: fn(
            &StaticConfigProvider,
            &AgentRuntimeState,
            String,
            std::sync::Arc<ToolRegistry>,
            Option<tokio::sync::watch::Receiver<bool>>,
        ) -> _ = run_subagent_turn;
    }
}
```

- [ ] **Step 4: 运行测试确认失败**

```bash
cargo test -p agent-core coordinator_tests 2>&1 | head -10
```

Expected: 编译错误，`coordinator` 模块不存在。

- [ ] **Step 5: 在 workflows/mod.rs 中添加 pub mod coordinator**

```rust
pub mod coordinator;
```

- [ ] **Step 6: 创建 coordinator.rs**

```rust
// crates/agent-core/src/workflows/coordinator.rs
//
// 子 agent 协调器 — 允许主 agent 以 tool_result 形式获取子任务结果

use std::sync::Arc;

use crate::{
    AgentRuntimeState, AgentTurnDescriptor, NullEventSink, ToolExecutorFn, ToolRegistry,
    IntoExecutor,
};
use crate::config::StaticConfigProvider;
use crate::providers;

const SUBAGENT_MAX_TURNS: u32 = 10;
const SUBAGENT_RESULT_MAX_CHARS: usize = 8_000;

/// 运行一个单级子 agent，返回其最终文本响应（截断至 SUBAGENT_RESULT_MAX_CHARS）
///
/// 子 agent：
/// - 使用 NullEventSink（不输出到父 agent 的事件流）
/// - 使用独立的空历史（不继承父 agent 的对话）
/// - 使用父 agent 提供的工具集（但不含 spawn_subagent，防止递归）
/// - 受父 agent 的 cancel_rx 控制（父取消 → 子取消）
pub async fn run_subagent_turn(
    config_provider: &StaticConfigProvider,
    runtime_state: &AgentRuntimeState,
    prompt: String,
    subagent_tools: Arc<ToolRegistry>,
    cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<String, String> {
    let sink = NullEventSink;
    let tab_id = format!("subagent-{}", uuid_v4_simple());

    // 子 agent 的工具执行器（将 registry 转为 ToolExecutorFn）
    let executor: ToolExecutorFn = Arc::clone(&subagent_tools).into_executor();

    // 子 agent 的请求描述：空历史，仅用 prompt 作为 user message
    let request = AgentTurnDescriptor {
        tab_id: tab_id.clone(),
        user_message: prompt.clone(),
        // 历史为空
        conversation_history: vec![],
        // 获取父 agent 的系统 prompt 配置（但不含 spawn_subagent 能力）
        domain_config: config_provider.domain_config(),
        model: None,
        max_tokens: None,
        sampling_config: None,
    };

    // 运行子 agent turn loop（最多 SUBAGENT_MAX_TURNS 轮工具调用）
    let outcome = providers::chat_completions::run_turn_loop(
        &sink,
        config_provider,
        runtime_state,
        &request,
        &[],
        executor,
        cancel_rx,
    )
    .await?;

    // 提取子 agent 的最终文本响应
    let response_text = outcome
        .final_assistant_text()
        .unwrap_or_else(|| "[Subagent produced no text response]".to_string());

    // 截断防止 token 爆炸
    if response_text.len() > SUBAGENT_RESULT_MAX_CHARS {
        Ok(format!(
            "{}...[truncated at {} chars]",
            &response_text[..SUBAGENT_RESULT_MAX_CHARS],
            SUBAGENT_RESULT_MAX_CHARS
        ))
    } else {
        Ok(response_text)
    }
}

/// 简单的 UUID v4（不依赖 uuid crate，仅用于 tab_id）
fn uuid_v4_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:x}-subagent", nanos)
}
```

**注意：** `AgentTurnDescriptor` 的实际字段名和 `outcome.final_assistant_text()` 方法名需以 grep 结果为准。执行时需先查证：

```bash
grep -n "pub struct AgentTurnDescriptor\|pub fn final_assistant_text\|pub fn assistant_text" crates/agent-core/src/session.rs crates/agent-core/src/turn_engine.rs | head -10
```

以实际字段名修正 coordinator.rs 的代码。

- [ ] **Step 7: 在 lib.rs 导出**

```rust
pub use workflows::coordinator::run_subagent_turn;
```

- [ ] **Step 8: 编译**

```bash
cargo build -p agent-core 2>&1 | grep "^error" | head -10
```

Expected: 无错误（根据实际签名调整代码）。

- [ ] **Step 9: 运行测试**

```bash
cargo test -p agent-core coordinator_tests 2>&1
```

Expected: 编译测试 PASS（签名匹配）。

- [ ] **Step 10: Commit**

```bash
git add crates/agent-core/src/workflows/
git commit -m "feat(coordinator): implement run_subagent_turn for single-level delegation"
```

---

## Task 3: 实现 spawn_subagent 工具

**Files:**
- Create: `crates/agent-cli/src/local_tools/subagent.rs`

- [ ] **Step 1: 写失败测试**

先创建空文件，在 mod.rs 引用：

```bash
touch crates/agent-cli/src/local_tools/subagent.rs
```

在 `crates/agent-cli/src/local_tools/mod.rs` 中添加 `mod subagent;`。

```bash
cargo build -p agent-cli 2>&1 | head -5
```

Expected: 编译成功（空模块）。

- [ ] **Step 2: 实现 spawn_subagent 工具**

```rust
// crates/agent-cli/src/local_tools/subagent.rs

use agent_core::{
    run_subagent_turn, AgentRuntimeState, AgentToolResult, IntoExecutor,
    ToolRegistry,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::watch;

use super::common::{error_result, ok_result, tool_arg_string};

const MAX_PROMPT_LEN: usize = 4_000;

/// 执行 spawn_subagent 工具调用
pub async fn execute_spawn_subagent(
    config_provider: &agent_core::StaticConfigProvider,
    runtime_state: &AgentRuntimeState,
    subagent_tools: Arc<ToolRegistry>,  // 不含 spawn_subagent 的工具集
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let prompt = match tool_arg_string(&args, "prompt") {
        Ok(p) => p,
        Err(e) => return error_result("spawn_subagent", call_id, e),
    };

    if prompt.trim().is_empty() {
        return error_result(
            "spawn_subagent",
            call_id,
            "prompt cannot be empty".to_string(),
        );
    }

    if prompt.len() > MAX_PROMPT_LEN {
        return error_result(
            "spawn_subagent",
            call_id,
            format!(
                "prompt too long ({} chars, max {})",
                prompt.len(),
                MAX_PROMPT_LEN
            ),
        );
    }

    match run_subagent_turn(
        config_provider,
        runtime_state,
        prompt,
        subagent_tools,
        cancel_rx,
    )
    .await
    {
        Ok(response) => ok_result("spawn_subagent", call_id, response),
        Err(e) => error_result("spawn_subagent", call_id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_subagent_rejects_empty_prompt() {
        // 验证空 prompt 会被拒绝（不需要真实 API）
        let args = serde_json::json!({"prompt": ""});
        let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        assert!(prompt.trim().is_empty(), "empty prompt should be detected");
    }

    #[test]
    fn spawn_subagent_rejects_oversized_prompt() {
        let big_prompt = "x".repeat(MAX_PROMPT_LEN + 1);
        assert!(big_prompt.len() > MAX_PROMPT_LEN);
    }
}
```

- [ ] **Step 3: 注册 SpawnSubagentTool 到 ToolHandler**

在 `mod.rs` 中添加：

```rust
/// SpawnSubagentTool 需要 config_provider 和工具集（不含自身，防止递归）
pub struct SpawnSubagentTool {
    pub config_provider: std::sync::Arc<agent_core::StaticConfigProvider>,
    pub runtime_state: std::sync::Arc<agent_core::AgentRuntimeState>,
    pub subagent_tools: std::sync::Arc<ToolRegistry>, // 不含 spawn_subagent
}

impl ToolHandler for SpawnSubagentTool {
    fn name(&self) -> &'static str { "spawn_subagent" }

    fn contract(&self) -> agent_core::tools::AgentToolContract {
        agent_core::tools::tool_contract("spawn_subagent")
    }

    fn execute(
        &self,
        call: AgentToolCall,
        cancel_rx: Option<watch::Receiver<bool>>,
    ) -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>> {
        let config = std::sync::Arc::clone(&self.config_provider);
        let state = std::sync::Arc::clone(&self.runtime_state);
        let tools = std::sync::Arc::clone(&self.subagent_tools);
        Box::pin(async move {
            let parsed_args = match agent_core::parse_tool_arguments(&call.arguments) {
                Ok(v) => v,
                Err(e) => return agent_core::tools::error_result("spawn_subagent", &call.call_id, format!("Invalid tool arguments JSON: {}", e)),
            };
            subagent::execute_spawn_subagent(
                &config,
                &state,
                tools,
                &call.call_id,
                parsed_args,
                cancel_rx,
            )
            .await
        })
    }
}
```

- [ ] **Step 4: 更新 build_default_registry 分两步构建（先构建不含 spawn_subagent 的 subagent_tools，再构建含 spawn_subagent 的主 registry）**

```rust
/// 构建含 spawn_subagent 的完整工具集
/// subagent_tools 是去掉 spawn_subagent 的子集（防止递归）
pub fn build_full_registry(
    config_provider: std::sync::Arc<agent_core::StaticConfigProvider>,
    runtime_state: std::sync::Arc<agent_core::AgentRuntimeState>,
) -> ToolRegistry {
    // 子 agent 可用的工具（不含 spawn_subagent）
    let subagent_registry = std::sync::Arc::new(build_default_registry(std::sync::Arc::clone(&runtime_state)));

    ToolRegistry::builder()
        .register(ReadFileTool)
        .register(ListFilesTool)
        .register(SearchProjectTool)
        .register(RunShellCommandTool)
        .register_arc(std::sync::Arc::new(MemoryWriteTool {
            runtime_state: std::sync::Arc::clone(&runtime_state),
        }))
        .register_arc(std::sync::Arc::new(SpawnSubagentTool {
            config_provider,
            runtime_state,
            subagent_tools: subagent_registry,
        }))
        .build()
}
```

- [ ] **Step 5: 在 tools.rs 添加 spawn_subagent contract 和 schema**

在 `tool_contract()` 中添加：

```rust
"spawn_subagent" => AgentToolContract {
    capability_class: ToolCapabilityClass::Network, // 会调用 API
    resource_scope: ToolResourceScope::Workspace,
    approval_policy: ToolApprovalPolicy::Never,
    review_policy: ToolReviewPolicy::None,
    suspend_behavior: ToolSuspendBehavior::None,
    result_shape: ToolResultShape::CommandOutput,
    parallel_safe: false,
    approval_bucket: "spawn_subagent",
},
```

在 `default_tool_specs()` 中添加：

```rust
AgentToolSpec {
    name: "spawn_subagent".to_string(),
    description: "Delegate a self-contained subtask to a subagent. The subagent runs independently with its own conversation history and returns its final text response. Use for parallel research, code generation in isolation, or complex subtasks that don't need shared context. Subagents cannot spawn further subagents. Prompt must clearly describe a complete, standalone task.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "prompt": {
                "type": "string",
                "description": "Complete, standalone task description for the subagent. Max 4000 characters. Must be self-contained — the subagent has no access to the current conversation history."
            }
        },
        "required": ["prompt"]
    }),
},
```

- [ ] **Step 6: 在 main.rs 使用 build_full_registry**

将 `main.rs` 中的 `build_default_executor(...)` 替换为：

```rust
let tool_executor = std::sync::Arc::new(
    local_tools::build_full_registry(Arc::clone(&config_provider_arc), Arc::clone(&runtime_state_arc))
).into_executor();
```

- [ ] **Step 7: 编译所有**

```bash
cargo build --workspace 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 8: 运行测试**

```bash
cargo test -p agent-core -p agent-cli 2>&1 | grep -E "test result|FAILED"
```

Expected: 全部 PASS。

- [ ] **Step 9: Commit**

```bash
git add crates/agent-cli/src/local_tools/ crates/agent-core/src/tools.rs crates/agent-cli/src/main.rs
git commit -m "feat(coordinator): implement spawn_subagent tool with single-level guard"
```

---

## Task 4: P4 集成验证

- [ ] **Step 1: 整体编译**

```bash
cargo build --workspace 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 2: 运行所有测试**

```bash
cargo test --workspace 2>&1 | grep -E "test result|FAILED"
```

Expected: 全部 PASS。

- [ ] **Step 3: 递归防护验证（重要）**

确认 `subagent_registry`（传给 `SpawnSubagentTool` 的工具集）不包含 `spawn_subagent`：

```bash
cargo test -p agent-cli -- --nocapture 2>&1 | grep "spawn_subagent"
```

或手动检查 `build_default_registry` 不含 `SpawnSubagentTool`：

```bash
grep -n "SpawnSubagentTool\|spawn_subagent" crates/agent-cli/src/local_tools/mod.rs
```

Expected: `SpawnSubagentTool` 只出现在 `build_full_registry` 中，`build_default_registry` 中没有。

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(p4): complete multi-agent coordinator — spawn_subagent with recursion guard"
```
