# Claude Prism Crates 工作说明（给执行型 Agent）

更新时间：2026-04-23

## 1. 目标与范围

这份文档面向“接手实现任务的 agent”，帮助你快速理解 `crates/*` 现状、边界和改动入口。

本仓库当前 workspace 成员：
- `crates/agent-core`
- `crates/agent-cli`
- `apps/desktop/src-tauri`

其中 `crates` 层是运行时核心：
- `agent-core`：与 Tauri/CLI 解耦的通用 agent 运行时
- `agent-cli`：基于 `agent-core` 的独立命令行入口

## 2. 总体架构（一句话版）

`agent-core` 提供“策略 + 会话状态 + 工具执行编排 + provider run loop + 事件模型”；`agent-cli` 只负责参数输入、事件输出和 provider 分发。

## 3. Crates 快速地图

### 3.1 `agent-core`（核心）

代码规模（`src`）：约 10k+ 行，关键模块如下。

- `config.rs`
  - 统一运行时配置结构 `AgentRuntimeConfig`
  - 抽象配置加载接口 `ConfigProvider`
  - `StaticConfigProvider`（CLI/测试常用）

- `event_sink.rs`
  - 统一事件输出接口 `EventSink`
  - 运行时与 UI/CLI 的唯一事件桥

- `events.rs`
  - 标准事件协议：`AgentEventPayload` + `AgentCompletePayload`
  - 事件名常量：`AGENT_EVENT_NAME`, `AGENT_COMPLETE_EVENT_NAME`

- `provider.rs`
  - 通用 provider 领域模型：`AgentTurnDescriptor`, `AgentTurnProfile`, `AgentTaskKind` 等

- `instructions.rs`
  - turn 分类与指令拼装核心
  - 关键函数：`resolve_turn_profile`, `tool_choice_for_task`, `max_rounds_for_task`, `build_agent_instructions_with_work_state`

- `tools.rs`
  - 工具契约模型与 schema 生成
  - 关键函数：`default_tool_specs`, `to_openai_tool_schema`, `to_chat_completions_tool_schema`, `parse_tool_arguments`
  - 工具执行策略/审批策略都在这里建模

- `turn_engine.rs`
  - provider 无关的工具执行流水线
  - 关键函数：`execute_tool_calls`
  - 统一事件发射函数族：`emit_status` / `emit_tool_call` / `emit_tool_result` / `emit_error` 等
  - `ToolExecutorFn` 是平台注入点（桌面端/CLI 各自注入执行器）

- `providers/openai.rs`
  - OpenAI Responses 流式回合循环 `run_turn_loop`
  - SSE 消费 + tool 调用回合 + retry/cancel/budget

- `providers/chat_completions.rs`
  - 兼容 `minimax/deepseek` 的 Chat Completions 回合循环 `run_turn_loop`
  - transcript 重建、reasoning/tool call 汇总、provider 兼容降级

- `session.rs`
  - 运行时状态中心 `AgentRuntimeState`
  - 会话、历史、审批、pending turn、workflow state、memory index 持久化

- `streaming.rs`
  - SSE 帧解析与流片段合并工具

- `message_builder.rs`
  - 各 provider 消息格式拼装/提取函数

- `workflows/*`
  - 学术工作流状态机（`paper_drafting` / `literature_review` / `peer_review`）

- `document_artifacts.rs`, `review_runtime.rs`, `telemetry.rs`
  - 文档资源检索、可审查工件载荷、遥测落盘辅助

### 3.2 `agent-cli`（薄壳入口）

- `main.rs`
  - 解析 CLI 参数并区分运行模式：`--prompt` 单轮 / 无 `--prompt` 默认 REPL
  - 构造 `StaticConfigProvider` + 运行时状态 + `ToolExecutorFn` fallback
  - 分发到 `turn_runner::run_turn`（当前仅 chat-completions provider）
  - 统一错误收敛与完成事件发射

- `tool_executor.rs`
  - 当前是 fallback：返回 “CLI runtime 不支持该工具”
  - 也因此 CLI 对工具型请求默认 fail-fast（不是 bug，是当前边界）

- `args.rs`
  - `RunMode`（`SingleTurn` / `Repl`）与 `OutputMode`（`human` / `jsonl`）解析

- `output.rs`
  - `HumanEventSink`（默认）输出可读日志流
  - `JsonlEventSink` 保持机器可消费事件流

- `turn_runner.rs`
  - provider 入口收敛与校验（仅 `minimax` / `deepseek`）
  - 会话历史线程（基于 `AgentRuntimeState::history_for_session` / `append_history`）

- `repl.rs`
  - 单行输入 REPL：空行忽略，`exit/quit` 退出
  - `run_repl` 回调驱动多轮 turn 执行

## 4. 核心调用链（你改代码前先看）

### 4.1 OpenAI Responses

1. CLI/桌面入口构造 `request` + `runtime_state` + `ConfigProvider` + `EventSink`
2. 调用 `providers::openai::run_turn_loop(...)`
3. `stream_response_once` 拉取 SSE，解析 assistant 文本 + function calls
4. `turn_engine::execute_tool_calls(...)` 执行/并发执行工具
5. tool 结果回灌模型继续下一轮，直到：
   - 无 tool call（完成）
   - 命中暂停审批（suspended）
   - 取消/预算/错误（error）

### 4.2 Chat Completions（MiniMax/DeepSeek）

1. `transcript_to_chat_messages(...)` 生成 messages
2. `stream_chat_completions_response_once(...)` 消费流式增量
3. 与 OpenAI 同样进入 `execute_tool_calls(...)`
4. 累积 `transcript_messages`，最终返回 `AgentTurnOutcome`

## 5. 三个“平台解耦”接口（最重要）

- `ConfigProvider`
  - 负责“从哪里拿配置”
  - 桌面端来自 settings；CLI 来自静态构造

- `EventSink`
  - 负责“把事件发到哪里”
  - 桌面端发 Tauri event；CLI 写 stdout JSONL

- `ToolExecutorFn`
  - 负责“具体工具怎么执行”
  - `agent-core` 只编排，不关心具体平台实现

理解这三个接口后，你就知道什么逻辑该放 core，什么该留在 adapter。

## 6. 任务分类与策略关键点

`instructions.rs` 里会根据 prompt/selection/attachment 自动归类任务：
- `SelectionEdit`, `FileEdit`, `SuggestionOnly`, `Analysis`, `LiteratureReview`, `PaperDrafting`, `PeerReview`

策略结果直接影响：
- `tool_choice_for_task`（`required` / `auto` / `none`）
- `max_rounds_for_task`（不同任务不同轮数上限）
- `sampling_profile`（edit_stable / analysis_balanced / analysis_deep / chat_flexible）

这部分是行为变化的高风险区，改动后必须回归测试。

## 7. 工具体系（tools.rs）

`tools.rs` 不仅是 schema，还定义了执行语义：
- capability class / resource scope
- approval policy / review policy / suspend behavior
- result shape / parallel safety

额外注意：
- `PRISM_AGENT_WRITING_TOOLS` 可控制写作工具组是否启用
- provider schema 适配由 `to_openai_tool_schema` 与 `to_chat_completions_tool_schema` 统一处理

## 8. 状态与持久化（session.rs）

`AgentRuntimeState` 维护并持久化：
- sessions/histories
- tool approvals + pending turns
- tab/session work state
- workflows
- memory index
- telemetry log path

默认目录语义：
- app 级：`<app_config_dir>/agent-runtime/*`
- project 级：`<project_root>/.chat-prism`

## 9. Workflow 状态机（学术场景）

内置 3 条 workflow：
- `literature_review`: `pico_scoping -> search_and_screen -> paper_analysis -> evidence_synthesis -> completed`
- `paper_drafting`: `outline_confirmation -> section_drafting -> consistency_check -> revision_pass -> final_packaging -> completed`
- `peer_review`: `scope_and_criteria -> section_review -> statistics_review -> report_and_revision_plan -> completed`

状态推进与 checkpoint 决策在 `workflows/mod.rs`。

## 10. CLI 当前边界（非常重要）

`agent-cli` 当前是“MVP-1 可交互 CLI”，但仍不是“全功能桌面替代”：
- 支持两种运行方式：
  - `--prompt`：单轮执行后退出
  - 无 `--prompt`：进入 REPL，多轮对话
- 支持配置初始化与编辑：
  - 首次运行配置缺失时自动进入全屏向导（provider/model/api_key/base_url/output）
  - `agent-runtime config init`：强制重新初始化
  - `agent-runtime config edit`：编辑现有配置
  - `agent-runtime config show`：展示当前配置（api_key 掩码）
  - `agent-runtime config path`：展示配置文件路径
  - REPL 输入 `/config`：就地打开配置编辑向导
- 配置优先级固定为：`CLI 参数 > 环境变量 > 本地配置文件 > 交互向导`
- 支持两种输出：
  - `human`（默认，可读流式输出）
  - `jsonl`（兼容机器消费）
- 交互 provider 路径当前仅支持 chat-completions（`minimax` / `deepseek`）
- 支持受限本地工具执行（MVP）：
  - `read_file`
  - `list_files`
  - `search_project`
  - `run_shell_command`（审批门禁 + 安全拦截）
- 新增 REPL 审批命令：
  - `/approve shell once`
  - `/approve shell session`
  - `/approve shell deny`
- 工具能力仍有边界：
  - 仍不支持 `write_file` / `apply_text_patch` / `replace_selected_text` 等写操作工具
  - 不支持 pending-turn 恢复命令面（CLI 侧尚未实现 resume 流）

所以“CLI 跑不动编辑任务”是预期行为，不是回归。

## 10.1 agent-cli TUI Runtime（S3）

- 默认 REPL 在 `--output human` 时进入全屏 TUI。
- 可用 `--ui-mode classic` 强制回退到经典行式 REPL。
- `--output jsonl` 始终绕过 TUI，保持机器可消费输出。
- TUI 语义时间线默认使用 `›/●/└`，支持展开 detail。
- turn 结果为 `suspended` 时保持同一 session，不重建会话上下文。

## 11. 与桌面端的接口关系

`apps/desktop/src-tauri/src/agent/adapter.rs` 提供：
- `TauriEventSink`（实现 `EventSink`）
- `TauriConfigProvider`（实现 `ConfigProvider`）

原则：
- 平台无关逻辑进 `agent-core`
- 平台耦合逻辑留在 adapter 或具体 tool executor

## 12. 给新 agent 的改动落点索引

如果你要做下面任务，优先改这些文件：

- 调整任务分类/轮数/tool choice
  - `crates/agent-core/src/instructions.rs`

- 增删工具、改工具契约或 schema
  - `crates/agent-core/src/tools.rs`

- 调整工具并发、审批中断、事件发射
  - `crates/agent-core/src/turn_engine.rs`

- 调整 provider 行为（重试、流解析、message 拼接）
  - `crates/agent-core/src/providers/openai.rs`
  - `crates/agent-core/src/providers/chat_completions.rs`

- 改会话持久化、memory、workflow 快照
  - `crates/agent-core/src/session.rs`

- 扩展 CLI 实际工具能力
  - `crates/agent-cli/src/tool_executor.rs`
  - `crates/agent-cli/src/main.rs`

## 13. 推荐验证命令

```bash
cargo build -p agent-core
cargo test -p agent-core --lib
cargo clippy -p agent-core -- -D warnings

cargo build -p agent-cli
cargo test -p agent-cli

# 保证 core 保持平台无关
rg -n "use tauri|tauri::" crates/agent-core/src
```

## 14. 当前仓库快照（供交接时快速判断）

- `agent-core` 与 `agent-cli` 均已成为 workspace 一等成员。
- `agent-core` 中已有较完整测试（`#[test]` 数量明显高于 CLI 层）。
- `crates/*` 下未发现显式 `TODO/FIXME/HACK` 标记（以 `rg` 快检为准）。

---

如果你是新接手的 agent，建议先按顺序阅读：
1. `crates/agent-core/src/lib.rs`（看 re-export 面）
2. `crates/agent-core/src/instructions.rs`
3. `crates/agent-core/src/turn_engine.rs`
4. `crates/agent-core/src/providers/openai.rs` 或 `chat_completions.rs`
5. `crates/agent-core/src/session.rs`
6. `crates/agent-cli/src/main.rs`
