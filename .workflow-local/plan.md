# Claude-Prism 项目整治计划

> 生成时间：2026-04-07
> 状态：待审批

---

## 一、项目现状概述

Claude-Prism 是一个基于 Tauri 2 (Rust) + React (TypeScript) 的桌面端学术写作应用。经过密集迭代（agent runtime 迁移、tool 系统重构、文献检索、设置界面等），项目积累了大量技术债：

- **Rust 后端**：~25,000 行（14 个 `.rs` 文件 + 4 个 tool 子模块）
- **前端**：~33,700 行（65+ TSX/TS 文件）
- **工作流文档**：~367KB（19 个 `.md` 文件）

核心问题是**多轮快速迭代后的组织混乱**：重复逻辑、巨型文件、文档膨胀、双 runtime 并存。

---

## 二、问题清单与修改方案

### 问题 1：双 Runtime 并存，职责不清

**现状：**
- `claude.rs`（1,995 行）— 通过 Claude CLI 子进程交互的原始 runtime
- `agent/` 目录（~9,500 行）— 自建的本地 Agent runtime（OpenAI/MiniMax/DeepSeek）
- 两者在 `lib.rs:353-379` 同时注册完整 Tauri command 集
- 前端通过 `settings.integrations.agent.runtime: "claude_cli" | "local_agent"` 切换

**问题：**
- 两套独立的 session 管理、事件模型、错误处理、消息适配，维护成本翻倍
- 前端 `agent-chat-store.ts` 需要处理两种完全不同的事件流
- 架构范式不同：CLI 子进程 vs HTTP API 调用

**修改方案：**
1. 明确 `local_agent` 为主线方向，将 `claude_cli` 标记为 deprecated
2. 统一 agent 事件模型：两个 runtime 共享同一个 `AgentEventPayload` 枚举
3. 中期：用 adapter pattern 将 `claude_cli` 包装为 `local_agent` 的一个 provider variant

**涉及文件：**
- `apps/desktop/src-tauri/src/claude.rs`
- `apps/desktop/src-tauri/src/agent/mod.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src/stores/agent-chat-store.ts`
- `apps/desktop/src/hooks/use-agent-events.ts`

---

### 问题 2：意图检测逻辑前后端重复

**现状：**

完全相同的关键词匹配逻辑在前后端各实现了一套：

| 功能 | Rust (`agent/mod.rs`) | TypeScript (`agent-chat-store.ts`) |
|---|---|---|
| 建议请求 | `prompt_explicitly_requests_suggestions()` | `isSuggestionOnlyRequest()` |
| 编辑请求 | `prompt_explicitly_requests_edit()` | `isEditIntentRequest()` |
| 深度分析 | `prompt_explicitly_requests_deep_analysis()` | `isDeepAnalysisRequest()` |
| 分析请求 | — | `isAnalysisRequest()` |

两边关键词列表高度相似但**不完全一致**（Rust 有 `"walk me through"` 而 TS 没有），中文关键词覆盖也有差异。

**问题：**
- 修改关键词必须同步两处，极易遗漏导致行为不一致
- 前端做了一次意图推断构建 `turnProfile`，后端在 `resolve_turn_profile()` 中重新推断，可能冲突
- 没有"谁说了算"的优先级约定

**修改方案：**
1. **确立单一权威层**：意图推断只在后端 Rust 侧执行
2. 前端只负责传递结构化的显式标记（用户通过 UI 按钮触发的 refine/suggest 等），不做关键词匹配
3. 删除前端的 `isSuggestionOnlyRequest()`, `isEditIntentRequest()`, `isAnalysisRequest()`, `isDeepAnalysisRequest()`
4. 后端关键词匹配作为 fallback，仅在前端没有传递显式 `task_kind` 时生效

**涉及文件：**
- `apps/desktop/src/stores/agent-chat-store.ts`（删除 4 个函数，简化 `sendPrompt` 中的 intent 推断）
- `apps/desktop/src-tauri/src/agent/mod.rs`（`resolve_turn_profile()` 已是权威层，无需改动）

---

### 问题 3：巨型文件问题

**严重超重文件：**

| 文件 | 行数 | 混合的职责 |
|---|---|---|
| `citation.rs` | 4,124 | 文献检索 + 评分 + 格式化 |
| `settings.rs` | 3,224 | 配置读写 + 验证 + keychain + connectivity test |
| `claude.rs` | 1,995 | CLI 发现 + 进程管理 + session 管理 |
| `agent/tools.rs` | 1,799 | 工具注册 + 策略检查 + 结果适配 |
| `agent/chat_completions.rs` | 1,523 | 流解析 + 消息构建 + turn 循环 |
| `settings-dialog.tsx` | 2,167 | 所有设置 Tab UI 在一个组件 |
| `latex-editor.tsx` | 2,447 | 编辑器核心 + toolbar 联动 + 事件处理 |
| `agent-chat-store.ts` | 1,485 | store + 业务逻辑 + 意图推断 + 事件处理 |

**修改方案：**

**Rust 侧：**
- `citation.rs` → 拆为 `citation/search.rs`, `citation/scoring.rs`, `citation/format.rs`
- `settings.rs` → 拆为 `settings/storage.rs`, `settings/validation.rs`, `settings/keychain.rs`, `settings/commands.rs`

**前端侧：**
- `settings-dialog.tsx` → 每个 Tab 拆为独立组件（`GeneralTab.tsx`, `ProvidersTab.tsx`, `CitationTab.tsx`, `AdvancedTab.tsx`）
- `agent-chat-store.ts` → 抽出 `agent-event-handler.ts`（事件处理逻辑）

**涉及文件：** 上述 8 个文件

---

### 问题 4：常量/代码重复定义

**`AGENT_CANCELLED_MESSAGE` 定义分散：**
- `agent/mod.rs:30` — `const AGENT_CANCELLED_MESSAGE: &str = "Agent run cancelled by user.";`
- `agent/chat_completions.rs:28` — 独立重新定义了同名常量
- `agent/tools.rs` — 通过 `use super::AGENT_CANCELLED_MESSAGE` 正确引用

`chat_completions.rs` 自行重新定义了一个同名常量而非 `use super::` 引用。

**修改方案：**
- 删除 `chat_completions.rs:28` 的重复定义
- 添加 `use super::AGENT_CANCELLED_MESSAGE;`

**涉及文件：**
- `apps/desktop/src-tauri/src/agent/chat_completions.rs`

---

### 问题 5：工作流文档过度膨胀且碎片化

**现状：** `.workflow-local/` 包含 19 个文档共 367KB：

| 文件 | 大小 | 状态 |
|---|---|---|
| `SESSION_LOG.md` | 155KB | 流水账，大部分已过时 |
| `AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md` | 57KB | 大部分已完成 |
| `TASK_BOARD.md` | 39KB | 100+ 已完成项堆积 |
| `CHAT_COMPLETIONS_AGENT_MIGRATION_PLAN.md` | 23KB | 核心迁移已完成 |
| `claude-code-agent-study.md` | 23KB | 研究文档，已消化 |

多个文档之间存在大量信息重叠：
- `TASK_BOARD.md` 与 `ISSUES_WRITING_AGENT_V2.md` 追踪同样的工作
- `AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md` 与 `CHAT_COMPLETIONS_AGENT_MIGRATION_PLAN.md` 内容高度重复
- `PHASE_STATUS.md` 与 `TASK_BOARD.md` 的状态经常不同步

**修改方案：**
1. **归档已完成工作**：`TASK_BOARD.md` 中所有已勾选项移到 `ARCHIVE.md`，只保留 active/next items
2. **合并重复文档**：
   - `AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md` + `CHAT_COMPLETIONS_AGENT_MIGRATION_PLAN.md` → 合并为 `AGENT_ARCHITECTURE.md`
   - `PHASE_STATUS.md` 内容并入 `TASK_BOARD.md`
3. **清理 SESSION_LOG.md**：只保留最近 2 天的 entries，历史归档
4. **删除已过期文档**：`RELEASE_CHECK_2026-03-28.md`, `SETTINGS_AUDIT_2026-03-28.md`

**涉及文件：** `.workflow-local/*.md`

---

### 问题 6：Settings 系统过度复杂

**现状：** Settings 横跨 5 个文件，总计 ~7,000 行：

| 文件 | 行数 | 职责 |
|---|---|---|
| `settings.rs` | 3,224 | 读/写/验证/keychain/connectivity |
| `settings-schema.ts` | 921 | TypeScript 类型定义 |
| `settings-store.ts` | 483 | Zustand store |
| `settings-api.ts` | — | Tauri invoke 封装 |
| `settings-dialog.tsx` | 2,167 | 全部 Tab 的 UI |

配置 schema 在 Rust 和 TypeScript 两侧**分别定义**，无代码生成或共享 schema。
`citation.search.queryExecution` 下的专家参数（`mmrLambda`, `hitScoreThreshold` 等）直接暴露给用户 UI。

**修改方案：**
1. 面向专家的参数从 Settings UI 移到 Advanced 面板或隐藏，用 preset 模式（`QUERY_MODE_PRESETS` 已有雏形，提升为主 UI）
2. 考虑从 Rust 侧 schema 自动生成 TypeScript 类型
3. `settings.rs` 拆分如问题 3 所述

**涉及文件：**
- `apps/desktop/src/components/workspace/settings-dialog.tsx`
- `apps/desktop/src-tauri/src/settings.rs`
- `apps/desktop/src/lib/settings-schema.ts`

---

### 问题 7：前端 Store/Hook 边界模糊

**现状：**
- `agent-chat-store.ts` 不仅管理聊天状态，还包含意图推断、消息适配、事件发射等业务逻辑
- `use-agent-events.ts`（865 行）hook 承担大量事件分发和状态更新
- `sidebar.tsx`（1,548 行）混合文件树、搜索、面板切换多种职责

**修改方案：**
1. `agent-chat-store.ts` → 纯状态管理（messages, sessions, streaming state）
2. 抽出 `agent-actions.ts`（sendPrompt, cancel, approve 等操作）
3. 抽出 `agent-event-reducer.ts`（事件到状态的映射）
4. `sidebar.tsx` → 拆为 `FileTree.tsx`, `SidebarSearch.tsx`, `SidebarPanelSwitcher.tsx`

**涉及文件：**
- `apps/desktop/src/stores/agent-chat-store.ts`
- `apps/desktop/src/hooks/use-agent-events.ts`
- `apps/desktop/src/components/workspace/sidebar.tsx`

---

### 问题 8：前端缺少路由

**现状：** `App.tsx` 用 `projectRoot ? <WorkspaceWithAgent /> : <ProjectPicker />` 做顶层切换，debug 页面靠 URL query param `?debug=1`。没有路由库。

**影响：** 当前规模下可接受，但继续增加页面级视图时条件分支会失控。

**修改方案：** 暂不执行。如果未来增加 3+ 个页面级视图，引入轻量路由。

---

## 三、优先级矩阵

| 优先级 | 问题 | 修复成本 | 影响面 | 风险 |
|---|---|---|---|---|
| **P0** | #4 常量重复定义 | 极低（1 行） | 低但是坏味道 | 无 |
| **P0** | #2 意图检测前后端重复 | 低 | 每次调整意图逻辑都有不一致风险 | 低 |
| **P1** | #5 工作流文档清理 | 低 | 减少认知负担 | 无 |
| **P1** | #3 巨型文件拆分（settings-dialog） | 中 | 长期维护效率 | 需确保 UI 行为不变 |
| **P1** | #7 Store 边界重构 | 中 | 可测试性和可维护性 | 需确保事件流不中断 |
| **P2** | #3 巨型文件拆分（Rust 侧） | 中-高 | 长期维护效率 | 需 `cargo test` 验证 |
| **P2** | #6 Settings 简化 | 中 | 用户体验 + 开发体验 | 需 migration |
| **P2** | #1 双 Runtime 收敛 | 高 | 架构根本问题 | 需完整回归 |
| **P3** | #8 前端路由 | 低 | 预防性改进 | 无 |

---

## 四、执行阶段

### Phase 1：低成本高收益（1-2 天）

- [ ] **1.1** 删除 `chat_completions.rs:28` 的重复 `AGENT_CANCELLED_MESSAGE`，改为 `use super::AGENT_CANCELLED_MESSAGE`
- [ ] **1.2** 删除前端 4 个意图检测函数（`isSuggestionOnlyRequest`, `isEditIntentRequest`, `isAnalysisRequest`, `isDeepAnalysisRequest`），简化 `sendPrompt` 中的 intent 逻辑，让后端 `resolve_turn_profile()` 作为唯一权威
- [ ] **1.3** 清理 `TASK_BOARD.md`：已完成项移到 `ARCHIVE.md`，只保留 active/next items
- [ ] **1.4** 合并/删除过期工作流文档：
  - 合并 `AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md` + `CHAT_COMPLETIONS_AGENT_MIGRATION_PLAN.md` → `AGENT_ARCHITECTURE.md`
  - 删除 `RELEASE_CHECK_2026-03-28.md`, `SETTINGS_AUDIT_2026-03-28.md`
  - 清理 `SESSION_LOG.md` 只保留最近 entries
- [ ] **1.5** 验证：`cargo check` + `pnpm tsc --noEmit` 通过

### Phase 2：前端模块拆分（1 周）

- [ ] **2.1** `settings-dialog.tsx` 按 Tab 拆分为 `GeneralTab.tsx`, `ProvidersTab.tsx`, `CitationTab.tsx`, `AdvancedTab.tsx`
- [ ] **2.2** `agent-chat-store.ts` 抽出事件处理逻辑到 `agent-event-handler.ts`
- [ ] **2.3** `sidebar.tsx` 拆分为 `FileTree.tsx`, `SidebarSearch.tsx`, `SidebarPanelSwitcher.tsx`
- [ ] **2.4** 验证：全部 UI 交互行为不变，`pnpm tsc --noEmit` + `pnpm test` 通过

### Phase 3：Rust 侧模块拆分（1 周）

- [ ] **3.1** `settings.rs` → 拆为 `settings/` 目录（`storage.rs`, `validation.rs`, `keychain.rs`, `commands.rs`）
- [ ] **3.2** `citation.rs` → 拆为 `citation/` 目录（`search.rs`, `scoring.rs`, `format.rs`）
- [ ] **3.3** 验证：`cargo check` + `cargo test --lib` 通过

### Phase 4：架构收敛（2 周，可根据实际情况调整）

- [ ] **4.1** 明确 `claude_cli` 下线计划，标记 deprecated
- [ ] **4.2** 统一 agent 事件模型
- [ ] **4.3** Settings UI 简化：专家参数收入 Advanced，preset 模式提升为主 UI
- [ ] **4.4** 评估 Settings schema 代码生成方案（Rust → TypeScript）

---

## 五、回归验证命令

每个 Phase 完成后必须运行：

```bash
# Rust 编译检查
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml

# Rust 测试
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib

# TypeScript 类型检查
pnpm tsc --noEmit

# 前端测试
pnpm test

# 完整构建（Phase 完成时）
pnpm build
```
