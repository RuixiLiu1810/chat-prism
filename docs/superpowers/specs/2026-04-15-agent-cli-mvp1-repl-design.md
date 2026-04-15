# Agent CLI MVP-1 设计说明（Claude Code 风格交互层）

日期：2026-04-15
状态：Draft -> Approved for MVP-1 scope

## 1. 背景与目标

当前 `agent-cli` 仅支持单轮 `--prompt` 调用，且输出为 JSONL 事件流；不具备“类似 Claude Code”的命令行交互体验。

本阶段目标（MVP-1）是：

1. 实现 REPL 多轮对话（单行输入，回车发送）。
2. 提供默认 human-readable 流式输出。
3. 保留 `--output jsonl` 兼容模式。
4. 保持 `--prompt` 单轮模式。
5. provider 先聚焦 chat-completions 路径（`minimax` / `deepseek`）。

## 2. 范围与非目标

### 2.1 In Scope

1. `agent-cli` 结构模块化：参数、输出、turn 运行器、REPL 循环。
2. 默认无 `--prompt` 时进入 REPL。
3. 人类可读输出与 JSONL 输出双模式。
4. chat-completions provider path（minimax/deepseek）稳定运行。

### 2.2 Out of Scope

1. 真实工具执行（read/edit/shell 等）。
2. `/approve` `/resume` `/cancel` 指令系统。
3. 会话持久化与恢复。
4. 大范围 `agent-core` 改造。

## 3. 方案选型

### 方案 A：直接在 `main.rs` 叠加 REPL

- 优点：实现快。
- 缺点：后续扩展 slash 命令、审批、session 时复杂度激增。

### 方案 B（推荐）：模块化 CLI 外壳

- 优点：结构清晰，MVP-1 可交付且便于 MVP-2/3/4 增量演进。
- 缺点：初期改动略大于 A。

### 方案 C：先建完整状态机/事件总线

- 优点：理论上最强扩展性。
- 缺点：对 MVP-1 过度设计，交付慢。

结论：采用方案 B。

## 4. 架构设计

## 4.1 模块划分

新增/调整模块：

1. `crates/agent-cli/src/args.rs`
- 负责参数定义、运行模式判定（single-turn vs repl）。

2. `crates/agent-cli/src/output.rs`
- 定义 `OutputMode`。
- 提供 `HumanEventSink` 与 `JsonlEventSink`。

3. `crates/agent-cli/src/turn_runner.rs`
- 统一封装“执行一轮”的逻辑。
- 屏蔽 provider 分支细节（MVP-1 仅 chat-completions）。

4. `crates/agent-cli/src/repl.rs`
- 单行读取循环。
- 调用 `turn_runner` 执行每轮。

5. `crates/agent-cli/src/main.rs`
- 只保留启动装配和分发（单轮/REPL）。

## 4.2 运行语义

1. 当存在 `--prompt`：
- 执行单轮。
- 输出后退出。

2. 当不存在 `--prompt`：
- 进入 REPL 循环。
- 每次输入一行，回车发送一轮。
- 输入 `exit` 或 `quit` 退出（MVP-1 先使用文本关键字，不引入 slash 命令系统）。

3. 输出模式：
- `human`（默认）：状态、增量文本、turn 结束标记。
- `jsonl`：保留现有结构化事件输出。

## 4.3 Provider 策略（MVP-1）

1. 保留 chat-completions 作为主运行链路。
2. `minimax` / `deepseek` 正常可用。
3. `openai` 在 MVP-1 明确提示“当前 REPL 模式未纳入此 provider”，避免语义歧义。

## 4.4 与 agent-core 的边界

1. `agent-core` 维持现有行为，不做重构。
2. 允许“最多 1-2 个纯扩展点”，仅在显著降低 `agent-cli` 复杂度时引入。
3. MVP-1 目标是 CLI 交互层落地，不把核心层改造作为前提条件。

## 5. 数据流与控制流

单轮模式：

1. parse args。
2. 构造 config/runtime state。
3. 组装 request。
4. `turn_runner.run_turn()`。
5. sink 输出结果，进程退出。

REPL 模式：

1. parse args。
2. 初始化 shared runtime/config/sink。
3. loop: 读取一行输入 -> 组装 request -> `turn_runner.run_turn()` -> 输出。
4. 直到收到退出关键字。

## 6. 错误处理策略

1. provider 不支持：给出明确错误并继续（REPL）或退出（single-turn）。
2. 空输入：REPL 忽略并继续下一轮。
3. turn 失败：
- human 模式打印简洁错误；
- jsonl 模式保持结构化错误事件。
4. 中断处理：MVP-1 先不引入复杂 cancel 状态机；保持可恢复运行（下一行继续发起新 turn）。

## 7. 测试与验收

## 7.1 单元测试

1. `args.rs`：模式判定（`--prompt` 与 REPL）。
2. `output.rs`：human/jsonl 行为差异。
3. `turn_runner.rs`：provider 分支与错误映射。
4. `repl.rs`：输入循环、退出关键字、空输入跳过。

## 7.2 集成验收

1. `--prompt` 仍可单轮运行。
2. 默认进入 REPL 并可连续 10+ 轮。
3. human 输出可读且流式稳定。
4. `--output jsonl` 与现有消费方兼容。
5. `minimax` / `deepseek` 跑通 chat-completions。

## 8. 实施顺序

1. Task A：参数与模式判定（`args.rs` + `main.rs` 分发）。
2. Task B：输出层（`output.rs`）。
3. Task C：turn 运行器（`turn_runner.rs`）。
4. Task D：REPL 循环（`repl.rs`）。
5. Task E：测试与 smoke 验证。

## 9. 风险与缓解

1. 风险：human 输出与 event 结构耦合，后续改事件会破显示。
- 缓解：`output.rs` 做 payload 映射集中层。

2. 风险：`main.rs` 历史逻辑迁移出错。
- 缓解：先保持单轮路径行为等价，再接入 REPL。

3. 风险：provider 分支导致行为不一致。
- 缓解：MVP-1 明确 provider 范围，优先稳定 chat-completions。

## 10. 成功定义（Definition of Done）

满足以下条件即 MVP-1 完成：

1. 无 `--prompt` 默认进入 REPL，多轮可用。
2. 默认 human 输出可读，`--output jsonl` 兼容保留。
3. `--prompt` 单轮行为不回退。
4. chat-completions provider（minimax/deepseek）可稳定运行。
5. `cargo build -p agent-cli` 与 `cargo test -p agent-cli` 通过。
