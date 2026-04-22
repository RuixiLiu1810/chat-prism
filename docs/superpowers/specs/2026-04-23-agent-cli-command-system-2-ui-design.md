# Agent CLI 命令体系 2.0 + 常驻轻量头部设计

日期：2026-04-23
状态：Draft -> Proposed

## 1. 背景

`agent-cli` 已具备 REPL、多轮对话、配置向导与 `/config` 基础能力，但在“命令行交互手感”上仍与 Claude Code 存在差距，主要表现为：

1. 命令体系可发现性不足。
2. 缺少稳定的“当前上下文感”（模型、目录、会话、输出模式、git 状态）。
3. 缺少统一命令路由与错误引导（未知命令建议、命令输出风格一致性）。

## 2. 目标

本阶段聚焦“命令行交互手感”提升，目标如下：

1. 实现命令体系 2.0：`/help /commands /config /model /status /clear`。
2. 实现默认常驻的轻量头部（2 行 ASCII 信息区，REPL 嵌入其中）。
3. 按“每轮后刷新一次”策略更新头部。
4. 保持现有对话路径兼容，不引入重型 TUI 框架。

## 3. 范围与非目标

### 3.1 In Scope

1. 新增统一命令路由层。
2. 新增状态快照层（provider/model/project/git/session/output）。
3. 新增头部渲染层（轻量 ASCII）。
4. 命令错误引导与未知命令候选建议。
5. 命令输出风格统一（inline/panel）。

### 3.2 Out of Scope

1. 重型全屏 TUI（`ratatui` 等）。
2. 会话持久化系统（`/session save|load`）。
3. 环境诊断（`/doctor`）。
4. `agent-core` 行为改造。

## 4. 方案选型

### 方案 A：在 `main.rs` 直接叠加命令分支

- 优点：最快。
- 缺点：命令增长后会迅速失控，难维护。

### 方案 B（采用）：CLI Shell 分层

- 分为 `command_router` / `status_snapshot` / `header_renderer` 三层。
- 优点：
  - 职责边界清晰；
  - 易测试；
  - 后续扩展 `/session`、`/doctor` 成本低。

### 方案 C：直接上完整 TUI 框架

- 优点：视觉能力强。
- 缺点：过度设计，偏离“轻量头部 + 文本流”目标。

结论：采用方案 B。

## 5. 信息架构与路由规则

输入处理统一进入 `command_router`：

1. 若输入以 `/` 开头，则按命令路由分发。
2. 若输入非命令，则进入普通对话 turn 路径。
3. 未知命令统一返回错误 + 候选建议。

命令集合（本期固定）：

1. `/help`：显示快速帮助。
2. `/commands`：列出所有支持命令与示例。
3. `/config`：进入已有配置编辑流程。
4. `/model`：查看或切换当前模型（作用于 CLI 配置层）。
5. `/status`：显示当前上下文快照。
6. `/clear`：清屏并重绘头部（不清会话内存）。

## 6. 常驻轻量头部设计

## 6.1 布局

固定 2 行，不做重面板：

1. 第 1 行：`provider/model | output_mode | session_id`
2. 第 2 行：`project_path | git_branch(+dirty)`

风格：ASCII 轻框或轻分隔，不依赖颜色可读。

## 6.2 刷新策略

1. 启动时渲染一次。
2. 每个对话回合结束后重绘一次（已确认策略）。
3. 命令导致状态变更（如 `/config`、`/model`）后额外重绘一次。

## 6.3 终端兼容策略

优先“稳态打印”而非复杂光标操控：

1. 使用分隔块打印新头部。
2. 避免依赖终端私有控制码。
3. ANSI 能力可选增强，不作为必需条件。

## 7. 输出与交互规范

命令输出统一分两类：

1. `inline`：1-2 行状态结果（如 `/status` 简版）。
2. `panel`：块状帮助/命令表（如 `/help`、`/commands`）。

未知命令反馈规范：

1. `Unknown command: /xxx`
2. `Did you mean: /commands`（若存在高相似命令）

## 8. 错误处理

1. 命令参数不足：输出最短可执行用法。
2. 命令执行失败：输出单行错误，不退出 REPL。
3. 对话 turn 失败：保持现有错误事件行为，允许下一轮继续。
4. 头部字段缺失：显示 `<unset>`，不 panic。

## 9. 测试面

## 9.1 单元测试

1. `command_router`
- 命令识别、参数解析、未知命令候选建议。

2. `status_snapshot`
- provider/model/session/output 映射。
- git 分支与 dirty 状态探测。

3. `header_renderer`
- 固定 2 行布局与字段兜底。

## 9.2 集成测试

1. 启动后头部显示。
2. `/help`、`/commands`、`/status`、`/clear` 行为正确。
3. 普通输入不被命令路由拦截，进入 turn。
4. 每轮后发生一次头部刷新。

## 10. 成功标准（Definition of Done）

1. 命令体系 2.0 的 6 个命令全部可用。
2. 默认常驻轻量头部稳定显示。
3. 头部刷新遵循“每轮后刷新一次”。
4. 命令错误可引导，未知命令有建议。
5. `cargo test -p agent-cli` 与 `cargo clippy -p agent-cli -- -D warnings` 通过。

## 11. 实施顺序建议

1. 先拆分 `command_router` + `/help` `/commands` `/status` `/clear`。
2. 再落 `status_snapshot` 与 `header_renderer`。
3. 最后接 `/model` 与 `/config` 状态更新联动。
4. 补全单测与集成验证。
