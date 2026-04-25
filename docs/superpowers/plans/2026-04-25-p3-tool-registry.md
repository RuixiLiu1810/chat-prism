# P3: Tool Registry — Replace ToolExecutorFn with ToolHandler Trait

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `ToolExecutorFn`（`Arc<dyn Fn(...)>`）替换为 `ToolHandler` trait + `ToolRegistry` struct，使工具注册、发现、合约获取成为类型安全的操作，为动态工具加载和权限检查提供基础。

**Architecture:** 策略：保持向后兼容性的"包装式替换"— 新增 `ToolHandler` trait 和 `ToolRegistry`，同时保留 `ToolExecutorFn` 类型别名（由 `ToolRegistry::into_executor()` 生成），这样 desktop 的 3 个文件只需最小改动，而不需要完全重写。

**⚠️ 关键警告：P3 修改 `ToolExecutorFn` 相关代码必然影响 3 个 desktop 文件（编译错误）。本计划明确包含这 3 个文件的更新步骤。如果跳过 desktop 步骤，整个 workspace 将无法编译。**

影响的 desktop 文件：
- `apps/desktop/src-tauri/src/agent/turn_engine.rs`（使用 `ToolExecutorFn` 在 6, 34, 49 行）
- `apps/desktop/src-tauri/src/agent/chat_completions.rs`（14, 90, 105 行）
- `apps/desktop/src-tauri/src/agent/openai.rs`（6, 143, 157 行）

**Tech Stack:** Rust, `tokio::sync::watch`, `std::collections::HashMap`, `Arc`, `Pin`, `Future`

---

## v2 强制修订（本节优先于下文）

### Non-goals
- 不在本阶段引入新工具语义（仅重构注册与分发机制）。
- 不做 desktop 侧功能增强，只做编译与接口兼容。
- 不替换掉 `ToolExecutorFn`，必须保留兼容桥。

### 口径修正
- 所有示例代码必须以当前结构为准：
  - `AgentToolCall` 使用 `arguments: String`，非 `input` 字段。
  - `AgentToolResult.content` 为 `serde_json::Value`，非 `ToolResultContentBlock` 向量。
- Unknown tool 的返回需调用现有错误构造函数或等价 JSON 结构，避免引入并不存在的类型。
- 执行顺序采用 compatibility-first：
  - 先 core+cli 完成 registry 和兼容桥；
  - 再 desktop adapter 最小适配。

### DoD（v2）
- [ ] `ToolRegistry` 可注册/查找/分发工具。
- [ ] `ToolExecutorFn` 旧路径可继续工作（兼容桥生效）。
- [ ] `agent-core` 与 `agent-cli` 编译和测试通过。
- [ ] desktop adapter 完成最小接口适配并可编译检查通过。

### 执行约束
- 下文出现 `call.input`、`ToolResultContentBlock` 等过时字段时，必须先改为当前数据结构再编码。

---

## File Map

| 操作 | 文件 | 职责 |
|------|------|------|
| Modify | `crates/agent-core/src/turn_engine.rs` | 新增 `ToolHandler` trait，`ToolRegistry` struct，`ToolRegistryBuilder` |
| Modify | `crates/agent-core/src/lib.rs` | 导出新类型 |
| Modify | `crates/agent-cli/src/local_tools/mod.rs` | 将 4+1 个工具迁移到 `ToolHandler` 实现 |
| Modify | `crates/agent-cli/src/turn_runner.rs` | 接受 `ToolRegistry` 而不是 `ToolExecutorFn` |
| Modify | `crates/agent-cli/src/main.rs` | 构建 `ToolRegistry` |
| Modify | `crates/agent-cli/src/tui/shell.rs` | 同步更新 |
| **Modify** | `apps/desktop/src-tauri/src/agent/turn_engine.rs` | 适配新接口 |
| **Modify** | `apps/desktop/src-tauri/src/agent/chat_completions.rs` | 适配新接口 |
| **Modify** | `apps/desktop/src-tauri/src/agent/openai.rs` | 适配新接口 |

---

## Task 1: 定义 ToolHandler trait 和 ToolRegistry

**Files:**
- Modify: `crates/agent-core/src/turn_engine.rs`

- [ ] **Step 1: 找到 ToolExecutorFn 的定义位置**

```bash
grep -n "pub type ToolExecutorFn\|ToolExecutorFn =" crates/agent-core/src/turn_engine.rs
```

Expected: 找到类型别名定义行（约 25-40 行）。

- [ ] **Step 2: 写失败测试**

在 `turn_engine.rs` 末尾添加：

```rust
#[cfg(test)]
mod registry_tests {
    use super::*;
    use tokio::sync::watch;
    use std::pin::Pin;
    use std::future::Future;

    struct EchoTool;

    impl ToolHandler for EchoTool {
        fn name(&self) -> &'static str { "echo" }

        fn contract(&self) -> AgentToolContract {
            agent_core::tools::tool_contract("search_project")
        }

        fn execute(
            &self,
            call: AgentToolCall,
            _cancel_rx: Option<watch::Receiver<bool>>,
        ) -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>> {
            let content = format!("echo: {}", call.arguments);
            Box::pin(async move {
                AgentToolResult {
                    tool_name: "echo".to_string(),
                    call_id: call.call_id,
                    is_error: false,
                    content: serde_json::json!({ "echo": content }),
                    preview: "Echo tool executed.".to_string(),
                }
            })
        }
    }

    #[tokio::test]
    async fn registry_dispatches_to_correct_handler() {
        let registry = ToolRegistry::builder()
            .register(EchoTool)
            .build();

        assert!(registry.handler("echo").is_some());
        assert!(registry.handler("nonexistent").is_none());
    }

    #[tokio::test]
    async fn registry_into_executor_works() {
        use std::sync::Arc;
        let registry = ToolRegistry::builder()
            .register(EchoTool)
            .build();

        let executor: ToolExecutorFn = Arc::new(registry).into_executor();
        let call = AgentToolCall {
            call_id: "test-1".to_string(),
            tool_name: "echo".to_string(),
            arguments: r#"{"msg":"hello"}"#.to_string(),
        };
        let result = executor(call, None).await;
        assert!(!result.is_error);
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

```bash
cargo test -p agent-core registry_tests 2>&1 | head -10
```

Expected: 编译错误，`ToolHandler`, `ToolRegistry`, `ToolRegistryBuilder` 未定义。

- [ ] **Step 4: 实现 ToolHandler trait 和 ToolRegistry**

在 `turn_engine.rs` 中，在 `pub type ToolExecutorFn = ...` 定义之后添加：

```rust
/// 单个工具的执行单元。实现此 trait 替代直接构造 ToolExecutorFn。
pub trait ToolHandler: Send + Sync + 'static {
    /// 工具名称（与 API spec 中的 name 一致）
    fn name(&self) -> &'static str;

    /// 工具的安全/审批合约
    fn contract(&self) -> crate::tools::AgentToolContract;

    /// 执行工具调用，返回异步 future
    fn execute(
        &self,
        call: AgentToolCall,
        cancel_rx: Option<watch::Receiver<bool>>,
    ) -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>>;
}

/// 持有所有已注册工具，生成 ToolExecutorFn 或直接分发调用
pub struct ToolRegistry {
    handlers: std::collections::HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistry {
    /// 创建构建器
    pub fn builder() -> ToolRegistryBuilder {
        ToolRegistryBuilder {
            handlers: std::collections::HashMap::new(),
        }
    }

    /// 查找指定名称的处理器
    pub fn handler(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.handlers.get(name).cloned()
    }

    /// 列出所有已注册的工具名称
    pub fn tool_names(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }
}

/// 将 `Arc<ToolRegistry>` 转换为 `ToolExecutorFn`（向后兼容接口）
pub trait IntoExecutor {
    fn into_executor(self) -> ToolExecutorFn;
}

impl IntoExecutor for Arc<ToolRegistry> {
    fn into_executor(self) -> ToolExecutorFn {
        Arc::new(move |call: AgentToolCall, cancel_rx: Option<watch::Receiver<bool>>| {
            let registry = Arc::clone(&self);
            Box::pin(async move {
                match registry.handler(&call.tool_name) {
                    Some(handler) => handler.execute(call, cancel_rx).await,
                    None => crate::tools::error_result(
                        &call.tool_name,
                        &call.call_id,
                        format!("Unknown tool: {}", call.tool_name),
                    ),
                }
            }) as Pin<Box<dyn Future<Output = AgentToolResult> + Send>>
        })
    }
}

/// 构建器模式，链式注册工具
pub struct ToolRegistryBuilder {
    handlers: std::collections::HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistryBuilder {
    pub fn register<H: ToolHandler>(mut self, handler: H) -> Self {
        self.handlers.insert(handler.name().to_string(), Arc::new(handler));
        self
    }

    pub fn register_arc(mut self, handler: Arc<dyn ToolHandler>) -> Self {
        self.handlers.insert(handler.name().to_string(), handler);
        self
    }

    pub fn build(self) -> ToolRegistry {
        ToolRegistry { handlers: self.handlers }
    }
}
```

- [ ] **Step 5: 在 lib.rs 导出新类型**

在 `crates/agent-core/src/lib.rs` 的 `pub use turn_engine::` 列表中添加：

```rust
pub use turn_engine::{
    // ... 已有导出 ...
    IntoExecutor, ToolHandler, ToolRegistry, ToolRegistryBuilder,
};
```

- [ ] **Step 6: 运行测试**

```bash
cargo test -p agent-core registry_tests 2>&1
```

Expected: 2 个测试 PASS。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-core/src/turn_engine.rs crates/agent-core/src/lib.rs
git commit -m "feat(registry): add ToolHandler trait, ToolRegistry, and IntoExecutor"
```

---

## Task 2: 将 agent-cli 工具迁移到 ToolHandler

**Files:**
- Modify: `crates/agent-cli/src/local_tools/mod.rs`

- [ ] **Step 1: 检查现有工具结构**

```bash
cat crates/agent-cli/src/local_tools/mod.rs
```

Expected: 看到 `pub async fn execute_tool_call(...)` 函数，内含 match 分支分发到 4 个工具（read_file, list_files, search_project, run_shell_command，以及记忆工具 remember_fact）。

- [ ] **Step 2: 写编译测试（验证最终形态）**

在 `mod.rs` 末尾添加（暂时不运行，用于指导实现）：

```rust
#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn can_build_full_registry() {
        let registry = build_default_registry(todo!());
        assert!(registry.handler("read_file").is_some());
        assert!(registry.handler("list_files").is_some());
        assert!(registry.handler("search_project").is_some());
        assert!(registry.handler("run_shell_command").is_some());
        assert!(registry.handler("remember_fact").is_some());
    }
}
```

- [ ] **Step 3: 为每个工具定义 ToolHandler 实现**

在 `mod.rs` 中，为每个工具定义一个零大小结构体并实现 `ToolHandler`：

```rust
use agent_core::{ToolHandler, AgentToolCall, AgentToolResult, tools::AgentToolContract};
use std::pin::Pin;
use std::future::Future;
use tokio::sync::watch;

pub struct ReadFileTool;
impl ToolHandler for ReadFileTool {
    fn name(&self) -> &'static str { "read_file" }
    fn contract(&self) -> AgentToolContract { agent_core::tools::tool_contract("read_file") }
    fn execute(&self, call: AgentToolCall, cancel_rx: Option<watch::Receiver<bool>>)
        -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>>
    {
        Box::pin(async move {
            let parsed_args = match agent_core::parse_tool_arguments(&call.arguments) {
                Ok(v) => v,
                Err(e) => return agent_core::tools::error_result("read_file", &call.call_id, format!("Invalid tool arguments JSON: {}", e)),
            };
            workspace::execute_read_file(".", &call.call_id, parsed_args, cancel_rx).await
        })
    }
}

pub struct ListFilesTool;
impl ToolHandler for ListFilesTool {
    fn name(&self) -> &'static str { "list_files" }
    fn contract(&self) -> AgentToolContract { agent_core::tools::tool_contract("list_files") }
    fn execute(&self, call: AgentToolCall, cancel_rx: Option<watch::Receiver<bool>>)
        -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>>
    {
        Box::pin(async move {
            let parsed_args = match agent_core::parse_tool_arguments(&call.arguments) {
                Ok(v) => v,
                Err(e) => return agent_core::tools::error_result("list_files", &call.call_id, format!("Invalid tool arguments JSON: {}", e)),
            };
            workspace::execute_list_files(".", &call.call_id, parsed_args, cancel_rx).await
        })
    }
}

pub struct SearchProjectTool;
impl ToolHandler for SearchProjectTool {
    fn name(&self) -> &'static str { "search_project" }
    fn contract(&self) -> AgentToolContract { agent_core::tools::tool_contract("search_project") }
    fn execute(&self, call: AgentToolCall, cancel_rx: Option<watch::Receiver<bool>>)
        -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>>
    {
        Box::pin(async move {
            let parsed_args = match agent_core::parse_tool_arguments(&call.arguments) {
                Ok(v) => v,
                Err(e) => return agent_core::tools::error_result("search_project", &call.call_id, format!("Invalid tool arguments JSON: {}", e)),
            };
            workspace::execute_search_project(".", &call.call_id, parsed_args, cancel_rx).await
        })
    }
}

pub struct RunShellCommandTool;
impl ToolHandler for RunShellCommandTool {
    fn name(&self) -> &'static str { "run_shell_command" }
    fn contract(&self) -> AgentToolContract { agent_core::tools::tool_contract("run_shell_command") }
    fn execute(&self, call: AgentToolCall, cancel_rx: Option<watch::Receiver<bool>>)
        -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>>
    {
        Box::pin(async move {
            let parsed_args = match agent_core::parse_tool_arguments(&call.arguments) {
                Ok(v) => v,
                Err(e) => return agent_core::tools::error_result("run_shell_command", &call.call_id, format!("Invalid tool arguments JSON: {}", e)),
            };
            shell::execute_run_shell_command(
                &agent_core::AgentRuntimeState::default(),
                "tab-1",
                ".",
                &call.call_id,
                parsed_args,
                cancel_rx,
            ).await
        })
    }
}

// MemoryWriteTool 需要 AgentRuntimeState，因此包装成 Arc
pub struct MemoryWriteTool {
    pub runtime_state: std::sync::Arc<agent_core::AgentRuntimeState>,
}
impl ToolHandler for MemoryWriteTool {
    fn name(&self) -> &'static str { "remember_fact" }
    fn contract(&self) -> AgentToolContract { agent_core::tools::tool_contract("remember_fact") }
    fn execute(&self, call: AgentToolCall, cancel_rx: Option<watch::Receiver<bool>>)
        -> Pin<Box<dyn Future<Output = AgentToolResult> + Send>>
    {
        let state = std::sync::Arc::clone(&self.runtime_state);
        Box::pin(async move {
            let parsed_args = match agent_core::parse_tool_arguments(&call.arguments) {
                Ok(v) => v,
                Err(e) => return agent_core::tools::error_result("remember_fact", &call.call_id, format!("Invalid tool arguments JSON: {}", e)),
            };
            memory::execute_remember_fact(&state, &call.call_id, parsed_args, cancel_rx).await
        })
    }
}
```

- [ ] **Step 4: 添加 build_default_registry 函数**

```rust
use agent_core::{IntoExecutor, ToolExecutorFn, ToolRegistry};

pub fn build_default_registry(
    runtime_state: std::sync::Arc<agent_core::AgentRuntimeState>,
) -> ToolRegistry {
    ToolRegistry::builder()
        .register(ReadFileTool)
        .register(ListFilesTool)
        .register(SearchProjectTool)
        .register(RunShellCommandTool)
        .register_arc(std::sync::Arc::new(MemoryWriteTool { runtime_state }))
        .build()
}

/// 向后兼容：生成 ToolExecutorFn（供仍使用旧接口的调用方）
pub fn build_default_executor(
    runtime_state: std::sync::Arc<agent_core::AgentRuntimeState>,
) -> ToolExecutorFn {
    std::sync::Arc::new(build_default_registry(runtime_state)).into_executor()
}
```

- [ ] **Step 5: 编译**

```bash
cargo build -p agent-cli 2>&1 | grep "^error" | head -10
```

Expected: 无错误（现有 `execute_tool_call` 函数仍然保留，新代码与其共存）。

- [ ] **Step 6: 运行测试**

```bash
cargo test -p agent-cli 2>&1 | tail -10
```

Expected: 全部 PASS。

- [ ] **Step 7: Commit**

```bash
git add crates/agent-cli/src/local_tools/
git commit -m "feat(registry): implement ToolHandler for all local tools + build_default_registry"
```

---

## Task 3: 在 main.rs 和 turn_runner.rs 使用新 Registry

**Files:**
- Modify: `crates/agent-cli/src/main.rs`
- Modify: `crates/agent-cli/src/turn_runner.rs`

- [ ] **Step 1: 找到 main.rs 中构建 tool_executor 的位置**

```bash
grep -n "let tool_executor\|ToolExecutorFn\|build_tool_executor\|Arc::new.*Fn" crates/agent-cli/src/main.rs | head -10
```

Expected: 找到构建 tool_executor 的位置。

- [ ] **Step 2: 替换 main.rs 中的 tool_executor 构建**

找到现有的 tool_executor 构建代码（通常是复杂的 `Arc::new(move |call, cancel_rx| { ... })` 闭包），替换为：

```rust
let tool_executor = local_tools::build_default_executor(Arc::clone(&runtime_state_arc));
```

如果 `runtime_state` 不是 `Arc<AgentRuntimeState>`，调整包装：

```rust
let runtime_state_arc = Arc::new(runtime_state.clone());
let tool_executor = local_tools::build_default_executor(Arc::clone(&runtime_state_arc));
```

- [ ] **Step 3: 找到 TUI shell.rs 中的 tool_executor 构建**

```bash
grep -n "let tool_executor\|ToolExecutorFn\|build_tool_executor" crates/agent-cli/src/tui/shell.rs | head -5
```

做相同替换：

```rust
let tool_executor = local_tools::build_default_executor(Arc::clone(&runtime_state_arc));
```

- [ ] **Step 4: 编译验证**

```bash
cargo build -p agent-cli 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 5: 运行测试**

```bash
cargo test -p agent-cli 2>&1 | tail -5
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/main.rs crates/agent-cli/src/tui/shell.rs
git commit -m "refactor(registry): use build_default_executor in main.rs and tui/shell.rs"
```

---

## Task 4: 更新 Desktop 文件（必须）

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/turn_engine.rs`
- Modify: `apps/desktop/src-tauri/src/agent/chat_completions.rs`
- Modify: `apps/desktop/src-tauri/src/agent/openai.rs`

**⚠️ 重要：** 由于采用了"保留 ToolExecutorFn 类型别名"策略，desktop 文件无需大改。`ToolExecutorFn` 仍然是 `Arc<dyn Fn(...)>`，但现在可以通过 `ToolRegistry::into_executor()` 生成。主要变化是 desktop 端的 tool_executor 构建方式。

- [ ] **Step 1: 检查 desktop 文件当前的 tool_executor 构建方式**

```bash
grep -n "ToolExecutorFn\|tool_executor\|Arc::new.*Fn\|execute_tool" apps/desktop/src-tauri/src/agent/turn_engine.rs | head -15
```

Expected: 找到 desktop 侧构建 tool_executor 的位置和使用方式。

- [ ] **Step 2: 确认 desktop 是否有自己的工具集**

```bash
ls apps/desktop/src-tauri/src/agent/
grep -rn "execute_tool\|tool_call\|read_file\|run_command" apps/desktop/src-tauri/src/agent/ | head -10
```

Expected: desktop 可能有自己的工具（如 open_in_browser，写入临时文件等）。

- [ ] **Step 3: 如果 desktop 工具是独立的 match 分支 — 封装为 ToolHandler**

如果 desktop 有自己的工具 match 分支，将它们包装成 `ToolHandler` 实现（模式与 Task 2 相同），然后使用 `ToolRegistry::builder().register(...).build().into_executor()` 生成 `ToolExecutorFn`。

如果 desktop 的 `ToolExecutorFn` 仅作为函数参数传递（而不是在 desktop 端构建），则可能无需修改。

- [ ] **Step 4: 编译 desktop**

```bash
cargo build -p app 2>&1 | grep "^error" | head -10
```

（desktop 的 Tauri 包名可能是 `app` 或 `tauri-app`，以 Cargo.toml 为准）

```bash
grep "^name" apps/desktop/src-tauri/Cargo.toml
cargo build -p <desktop_crate_name> 2>&1 | grep "^error" | head -10
```

Expected: 无错误。

- [ ] **Step 5: 如果有编译错误，逐一修复**

对每个 `ToolExecutorFn` 相关的错误：
- 如果是类型参数不匹配：检查 `into_executor()` 调用是否正确
- 如果是生命周期/Send 问题：确认 handler 实现了 `Send + Sync + 'static`

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/agent/
git commit -m "fix(desktop): adapt to ToolRegistry interface (P3 compatibility)"
```

---

## Task 5: P3 集成验证

- [ ] **Step 1: 编译整个 workspace**

```bash
cargo build --workspace 2>&1 | grep "^error" | head -20
```

Expected: 无错误（整个 workspace 编译通过，包括 desktop）。

- [ ] **Step 2: 运行所有测试**

```bash
cargo test --workspace 2>&1 | grep -E "test result|FAILED"
```

Expected: 全部 PASS。

- [ ] **Step 3: 动态工具注册验证**

```bash
cargo test -p agent-core registry_tests 2>&1
cargo test -p agent-cli migration_tests 2>&1
```

Expected: 全部 PASS。

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(p3): complete ToolRegistry refactor — ToolHandler trait + IntoExecutor"
```
