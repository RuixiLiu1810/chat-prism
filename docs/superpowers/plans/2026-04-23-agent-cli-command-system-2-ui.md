# Agent CLI Command System 2.0 + Lightweight Header 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: 使用 `executing-plans`（或 `subagent-driven-development`）按任务推进，按检查点回传。

**Goal:** 在 `agent-cli` 中落地命令体系 2.0（`/help /commands /config /model /status /clear`）与默认常驻轻量头部，提升接近 Claude Code 的交互手感，同时保持 `jsonl` 输出模式兼容。

**Architecture:** 采用分层 CLI Shell 方案：`command_router`（输入与命令分发）+ `status_snapshot`（上下文采集）+ `header_renderer`（轻量 ASCII 头部渲染）；`main.rs` 仅做编排与运行时接线。

**Tech Stack:** Rust 2021, clap, tokio, std::process::Command(git status probing), existing `agent-core` runtime integration

---

## Scope Check

本计划仅覆盖 `crates/agent-cli` 命令交互与头部体验，不改 `agent-core` 协议与 provider 行为，不引入重型 TUI 依赖。

## File Structure

| File | Responsibility |
|---|---|
| `crates/agent-cli/src/command_router.rs` | 命令解析、未知命令建议、`/model` 参数化解析 |
| `crates/agent-cli/src/status_snapshot.rs` | provider/model/project/git/session/output 状态采集 |
| `crates/agent-cli/src/header_renderer.rs` | 2 行轻量头部渲染与清屏重绘 |
| `crates/agent-cli/src/main.rs` | REPL 主循环接线、命令执行、状态刷新策略 |
| `docs/superpowers/specs/2026-04-23-agent-cli-command-system-2-ui-design.md` | 设计规范（已存在） |
| `docs/superpowers/plans/2026-04-23-agent-cli-command-system-2-ui.md` | 本实施计划 |

---

### Task 1: 命令路由层（Command Router）

**Files:**
- Create: `crates/agent-cli/src/command_router.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/command_router.rs`

- [ ] 支持命令：`/help /commands /config /model /status /clear`
- [ ] `/model` 支持两种形态：`/model`（show）和 `/model <name>`（set）
- [ ] 未知命令返回统一格式，并给出候选建议（如 `/commnads -> /commands`）

验证：
- `cargo test -p agent-cli command_router::tests -v`

---

### Task 2: 状态快照层（Status Snapshot）

**Files:**
- Create: `crates/agent-cli/src/status_snapshot.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/status_snapshot.rs`

- [ ] 汇总字段：`provider/model/project_path/git_branch(+dirty)/session_id/output_mode`
- [ ] git 检测逻辑：非 repo 兜底 `<no-git>`
- [ ] 字段为空兜底 `<unset>`，不 panic

验证：
- `cargo test -p agent-cli status_snapshot::tests -v`

---

### Task 3: 轻量头部渲染层（Header Renderer）

**Files:**
- Create: `crates/agent-cli/src/header_renderer.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/header_renderer.rs`

- [ ] 固定 2 行信息区 + 轻量分隔线
- [ ] 支持 `git dirty` 标记
- [ ] 支持 `/clear` 清屏并重绘头部

验证：
- `cargo test -p agent-cli header_renderer::tests -v`

---

### Task 4: REPL 编排接线（Main Integration）

**Files:**
- Modify: `crates/agent-cli/src/main.rs`

- [ ] 启动时渲染头部（仅 `human` 输出模式）
- [ ] 每轮对话结束后重绘头部
- [ ] 状态改变命令（`/config`, `/model <x>`, `/clear`）后重绘头部
- [ ] `/status` 输出当前快照摘要
- [ ] `/help` 与 `/commands` 输出统一可读面板

验证：
- `cargo test -p agent-cli -v`
- `cargo build -p agent-cli`

---

### Task 5: 质量收口

**Files:**
- Modify: `crates/agent-cli/src/main.rs`
- Modify: related new modules

- [ ] `clippy -D warnings` 清零
- [ ] 错误消息风格统一（不崩溃，不中断 REPL）
- [ ] 保持 `jsonl` 输出模式兼容（不破坏事件流）

验证：
- `cargo clippy -p agent-cli -- -D warnings`

---

## Definition of Done

1. 6 个命令全部可用，未知命令有建议。
2. 默认 REPL 可见轻量头部，且每轮后刷新。
3. `/clear` 可重绘界面；`/status` 可输出上下文快照。
4. `cargo test/build/clippy` 全部通过。
