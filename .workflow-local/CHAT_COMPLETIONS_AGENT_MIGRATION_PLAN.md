# Chat Completions Agent 构建与迁移方案

## 目的

基于 `claude-code-fork-main` 的高价值技术内容，抽象出一套不依赖 Claude 专属 runtime、可落在 `chat completions API` 之上的完整 agent 架构与迁移方案。

这份方案的目标不是“把 MiniMax 调得更像聊天机器人”，而是：

1. 用 `chat completions` 跑出稳定的本地 agent runtime
2. 把 Claude Code 的关键系统能力内建进 ClaudePrism
3. 为 MiniMax / OpenRouter / 其他 OpenAI-compatible provider 提供统一底座

## 一、从 Claude Code Fork 提炼出的核心技术内容

### 1. QueryEngine：turn executor 是一级概念

来源：

- `claude-code-fork-main/src/QueryEngine.ts`

结论：

- 一个会话对应一个执行引擎
- 一次 `submitMessage()` 不是一次 HTTP 调用，而是一整轮任务生命周期
- 它统一管理：
  - message history
  - abort
  - permission denials
  - usage
  - system prompt assembly
  - tool lifecycle

对 ClaudePrism 的含义：

- 不能继续把 turn 状态散落在 provider 文件和前端 store 里
- `chat completions` agent 必须先有自己的 `TurnEngine`

### 2. sdkMessageAdapter：provider 消息必须先适配，不能直进 UI

来源：

- `claude-code-fork-main/src/remote/sdkMessageAdapter.ts`

结论：

- assistant text
- stream_event
- tool_result
- status
- compact boundary

这些是不同语义对象，不是“都算消息”。

对 ClaudePrism 的含义：

- provider 输出必须先归一化成内部事件协议
- React 组件不能直接理解 provider payload

### 3. structuredIO + toolExecution：权限不是按钮，而是 runtime pipeline

来源：

- `claude-code-fork-main/src/cli/structuredIO.ts`
- `claude-code-fork-main/src/services/tools/toolExecution.ts`

结论：

- 权限决策不是 UI 特效
- 它是工具执行链的一部分
- `allow once / allow session / deny` 背后要有规则与恢复语义

对 ClaudePrism 的含义：

- 当前审批卡片只是第一层外壳
- 真正要补的是：
  - pending approval
  - in-place resume
  - rule provenance
  - session-scoped policy

### 4. DiffDialog：review-first 是编辑主路径，不是补丁

来源：

- `claude-code-fork-main/src/components/diff/DiffDialog.tsx`

结论：

- “想改但还没写入”也应该成为可审阅对象
- diff 是编辑主界面，不是失败后的调试面板

对 ClaudePrism 的含义：

- 选区编辑请求不能再直接落到全文件 `write_file`
- 必须优先形成可审阅的 change object

### 5. sessionHistory + SessionMemory：history 不是列表，而是工作连续性

来源：

- `claude-code-fork-main/src/assistant/sessionHistory.ts`
- `claude-code-fork-main/src/services/SessionMemory/prompts.ts`

结论：

- 历史不只是恢复文本
- 还包括：
  - 当前工作状态
  - 重要文件/函数
  - 常用命令
  - 失败路径
  - 下一步

对 ClaudePrism 的含义：

- 不能只做 session list
- 要逐步引入 working memory，而不是只做 transcript replay

## 二、当前 ClaudePrism 的结构性问题

### 1. `chat_completions` runtime 还没有独立的 turn engine

现状：

- provider 文件仍承担太多调度责任
- turn loop、tool loop、状态发送、continuation 还没有完全抽成统一执行器

结果：

- 行为容易漂
- provider 替换成本高

### 2. 选区编辑还在用全文件 `write_file`

现状：

- `apps/desktop/src-tauri/src/agent/tools.rs`
- `write_file` 的 `content` 是 “Full replacement file content.”

结果：

- 用户只要求改一段
- 模型却可能输出“只有那一段的新文本”
- runtime 会把整篇文件替换掉

这是当前最严重的系统缺陷。

### 3. 审批流和工具执行流仍未完全合一

现状：

- 已经有审批卡片
- 已经有 auto-resume
- 但底层仍是“审批后补发 continuation”

结果：

- 语义仍然比 Claude Code 更重

### 4. session continuity 仍是轻量 identity，不是 working memory

现状：

- 已完成 session title / preview / currentWorkLabel / recentToolActivity

不足：

- 还没有把“当前任务状态”和“失败经验”结构化保存下来

### 5. turn intent 与 sampling 仍然不是统一对象

现状：

- 前端 `agent-chat-store.ts` 过去依赖关键词把请求猜成：
  - suggestion
  - reviewable_edit
  - neutral
- 后端 `agent/mod.rs` 又有一套独立的 prompt heuristic
- provider 请求体里没有按任务类型切换的 sampling profile

结果：

- 同一轮请求会被前后端重复猜测
- MiniMax 这类较弱模型会先被错误路由，再被过宽的默认采样放大不稳定性

冻结规则：

- 后续所有 agent 行为决策必须优先收敛到统一的 `TurnProfile`
- route / execution bias / sampling profile 不再分散在 prompt 拼接、后端 heuristics 和 provider 默认值里

## 二点五、剩余差距的去重判断

经过前几轮迁移与修复，以下问题已经不应再作为“未开始”的主线任务重复推进：

- model-facing tool result 污染模型
- 过弱的 base instructions / 无上下文标记语义
- task-aware `tool_choice`
- streamed tool-call `id/name` 合并 bug
- transcript fallback 丢失可见 tool context
- 中文 fallback intent detection
- 固定 `0..6` 轮次上限

当前真正剩余、且决定 runtime 是否形成闭环的差距，集中在三条 Sprint 主线：

1. `Sprint 2`
   - turn executor 仍不是资源感知、状态集中的执行器
   - work-state 仍主要停留在 runtime/UI state，未稳定注入模型上下文
2. `Sprint 3`
   - pending turn 仍未持久化
   - approval 仍缺 durable record / TTL / provenance
3. `Sprint 4`
   - tool execution 仍是串行
   - provider tool schema 仍缺 adapter
   - runtime 缺少轻量 structured telemetry

## 三、目标架构：基于 Chat Completions API 的完整 Agent

### 补充：TurnProfile / SamplingProfile 层

在 `Turn Engine` 之上增加一层统一任务画像：

- `task_kind`
  - `general`
  - `selection_edit`
  - `file_edit`
  - `suggestion_only`
  - `analysis`
- `selection_scope`
  - `none`
  - `selected_span`
- `response_mode`
  - `default`
  - `reviewable_change`
  - `suggestion_only`
- `sampling_profile`
  - `default`
  - `edit_stable`
  - `analysis_balanced`
  - `chat_flexible`

设计规则：

1. 前端优先提供显式 UI 信号
   - 当前文件
   - 当前选区
   - 明确的工具/入口动作
2. 关键词只允许作为弱兜底，不允许再做硬路由
3. provider 请求体必须消费 `sampling_profile`
4. `TurnProfile` 是 provider-neutral 协议的一部分，不得再退回字符串 prompt marker 作为主路径

## Layer 1：Turn Engine

新增独立执行层：

- `agent/turn_engine.rs`

职责：

1. 持有一轮任务的状态机
2. 组装 system prompt / route / working memory
3. 调 provider
4. 接收 provider stream
5. 检测 tool calls
6. 调用 Tool Orchestrator
7. 处理 pending approval / resume
8. 发内部事件给 UI

并且统一负责：

9. 消费 `TurnProfile`
10. 给 provider 注入对应 `SamplingProfile`

状态机建议：

- `planning`
- `streaming`
- `tool_running`
- `awaiting_approval`
- `review_ready`
- `resuming`
- `completed`
- `cancelled`
- `failed`

规则：

- 一个 tab 对应一个 active turn engine
- provider 不再自己管理整轮执行

## Layer 2：Provider Adapter

建议目录：

- `agent/providers/openai_responses.rs`
- `agent/providers/chat_completions.rs`
- `agent/providers/minimax.rs`
- `agent/providers/openrouter.rs`

职责：

1. 把内部消息转换成 provider 请求
2. 把 SSE / stream chunk 解析成内部增量事件
3. 把 tool call payload 解析成统一结构
4. 不负责本地工具执行
5. 不负责审批逻辑

`chat completions` adapter 需要支持：

1. full local message history
2. streaming delta merge
3. tool call assembly
4. finish_reason handling
5. reasoning / hidden content 兼容抽象
6. profile-driven sampling params
   - `temperature`
   - `top_p`
   - `max_tokens`

第一轮内建参数策略冻结为：

- `edit_stable`
  - `temperature = 0.2`
  - `top_p = 0.9`
  - `max_tokens = 4096`
- `analysis_balanced`
  - `temperature = 0.4`
  - `top_p = 0.9`
  - `max_tokens = 4096`
- `chat_flexible`
  - `temperature = 0.7`
  - `top_p = 0.95`
  - `max_tokens = 4096`

## Layer 3：Message Adapter / Internal Protocol

新增统一内部协议：

- `agent/protocol.rs`

建议对象：

1. `AgentTurnEvent`
   - `TextDelta`
   - `ToolCallProposed`
   - `ToolProgress`
   - `ToolResult`
   - `Status`
   - `ApprovalRequested`
   - `ReviewReady`
   - `TurnCompleted`
   - `TurnFailed`

2. `AgentToolInvocation`
   - `tool_name`
   - `call_id`
   - `input`
   - `origin_turn_id`

3. `AgentReviewArtifact`
   - `artifact_type`
   - `target_path`
   - `old_content`
  - `new_content`

## 四、执行顺序补充（冻结）

在后续继续扩 provider 或继续打磨 prompt 之前，先固定下面顺序：

1. `TurnProfile`
   - 先消除前后端双层关键词硬路由
   - 让前端传结构化 `turnProfile`
   - 后端以 `TurnProfile` 为主、heuristic 为弱兜底

2. `SamplingProfile`
   - 让 provider 请求体真正按任务类型切参数
   - OpenAI / MiniMax 路径都要接入
   - status: completed for the current migration scope
     - schema/settings/runtime 已正式托管 `samplingProfiles`
     - provider 不再只依赖硬编码内部默认值
     - 明确 UI 动作可以直接传 `TurnProfile`，减少对 prompt wording 的依赖

3. 之后才继续：
   - 更深的 permission runtime
   - 更深的 session memory
   - 新 provider 扩展
   - `selection_range`
   - `summary`

规则：

- 所有 provider 都只产出内部协议
- 前端永远不直接消费 provider 原始 payload

## 四点五、后续 Sprint 顺序（去重后）

### Sprint 2：executioner stability

- 引入 `TurnBudget`
- 把 abort/cancel 从 stream 层继续传播到 tool execution
- 把 `current_objective/current_target/last_tool_activity` 注入模型上下文

状态（2026-04-03）：
- 已完成当前迁移范围内的首轮交付
- 已落地：
  - `TurnBudget`
  - tool-level abort propagation
  - work-state prompt injection

### Sprint 3：permission runtime hardening

- pending turn 本地持久化
- TTL / stale cleanup
- `ToolApprovalRecord`
  - `decision`
  - `source`
  - `granted_at`
  - `expires_at`

状态（2026-04-03）：
- 已完成当前迁移范围内的首轮交付
- 已落地：
  - pending turn persistence
  - TTL / stale cleanup
  - `ToolApprovalRecord`

### Sprint 4：concurrency + compatibility + observability

- 只读工具并发、写工具串行
- MiniMax chat-completions tool schema adapter
- 轻量本地 telemetry / structured logs

状态（2026-04-03）：
- 已完成当前迁移范围内的首轮交付
- 已落地：
  - safe parallel read-only tools
  - mutation serialization preserved
  - MiniMax-first tool schema adapter
  - lightweight local telemetry

## Layer 4：Tool Orchestrator

新增：

- `agent/tool_orchestrator.rs`

职责：

1. 执行本地工具
2. 做参数校验
3. 做审批门控
4. 生成 review artifact
5. 返回结构化 tool result

工具应分层：

### 只读工具

- `read_file`
- `list_files`
- `search_project`

### 精确编辑工具

- `replace_selected_text`
- `apply_text_patch`
- `replace_file_range`

### 粗粒度写入工具

- `write_file`

### 高风险执行工具

- `run_shell_command`

核心规则：

- **选区编辑默认禁止直接走 `write_file`**
- `write_file` 只允许：
  - 创建新文件
  - 明确整文件重写
  - review/apply 最后提交阶段

## Layer 5：Permission Runtime

新增：

- `agent/permission_runtime.rs`

职责：

1. 接受工具请求
2. 判断：
   - allow once
   - allow session
   - deny session
   - review-first
3. 挂起 turn
4. 用户审批后原地恢复

当前本地 auto-resume 只是过渡实现。

最终目标：

- approval 不再通过“补发 continuation”模拟
- 而是 turn engine 内部真正 resume

## Layer 6：Review Runtime

新增：

- `agent/review_runtime.rs`

职责：

1. 把 blocked edit / prepared edit 统一变成 review artifact
2. 推给 diff panel
3. 等待用户 accept / reject
4. accept 后再真正 apply

规则：

- 编辑主路径必须是：
  - generate review artifact
  - review
  - apply
- 而不是：
  - 先直接写文件
  - 写坏了再回头看 diff

## Layer 7：Session Memory

新增：

- `agent/session_memory.rs`

分两层：

### L1：Working Identity

已有基础：

- `currentWorkLabel`
- `recentToolActivity`

继续补：

- current objective
- current target file
- pending approval / pending review

### L2：Structured Session Memory

后续目标：

- current state
- task specification
- important files/functions
- workflow / commands
- errors & corrections
- learnings / avoidances

这层不要立刻做重型自动总结，先提供结构和写入接口。

## 四、Chat Completions Agent 的构建方案

## Build Phase 0：冻结协议与工具分层

产出：

1. `AgentTurnEvent`
2. `AgentReviewArtifact`
3. 工具分层表
4. 编辑工具规范

门禁：

- 在这一步之前，不再继续往 `write_file` 上叠补丁

## Build Phase 1：抽 `TurnEngine`

目标：

- 把 `chat_completions.rs` 里的控制逻辑外移

完成标准：

- provider 只管 transport
- turn engine 统一驱动：
  - prompt assembly
  - stream handling
  - tool loop
  - status transitions

## Build Phase 2：落 selection-aware edit primitive

目标：

- 解决“改一段却重写全文”的根缺陷

建议新增工具：

1. `replace_selected_text`
   - 输入：
     - `path`
     - `selection_anchor`
     - `expected_selected_text`
     - `replacement_text`

2. `replace_file_range`
   - 输入：
     - `path`
     - `start_line`
     - `start_col`
     - `end_line`
     - `end_col`
     - `replacement_text`

3. `apply_text_patch`
   - 输入：
     - `path`
     - `expected_old_text`
     - `new_text`

推荐默认主路径：

- 选区编辑优先 `replace_selected_text`
- 失败再回退到 `apply_text_patch`
- 最后才允许 `write_file`

## Build Phase 3：review artifact 统一化

目标：

- 所有编辑请求都先产出 review artifact

包括：

- approval blocked edit
- prepared edit
- applied edit

完成标准：

- diff panel 成为唯一编辑审阅面
- 聊天区只负责状态和摘要

## Build Phase 4：真正 pending/resume 的 permission runtime

目标：

- 审批后原地恢复 turn

而不是：

- 批准后人工 continuation
- 或本地伪 continuation

完成标准：

- turn 被挂起
- user decision 回注 runtime
- tool call 继续执行
- provider 继续推理

## Build Phase 5：session continuity 升级

目标：

- 不只恢复 transcript
- 恢复工作状态

完成标准：

- resume 后能显示：
  - current objective
  - current target
  - last tool action
  - pending review / approval

## Build Phase 6：provider 扩展

顺序建议：

1. MiniMax
2. OpenRouter
3. DeepSeek 继续 parked，除非它在工具与稳定性上有明确收益

规则：

- 新 provider 只能接 adapter
- 不得侵入 turn engine / permission runtime / review runtime

## 五、从当前 ClaudePrism 迁移的具体方案

## Migration Step M1：停止扩写 `write_file` 语义

立即规则：

- `write_file` 明确定义为 whole-file write
- 不再承载选区编辑

## Migration Step M2：为选区编辑新增独立工具

修改位置：

- `apps/desktop/src-tauri/src/agent/tools.rs`

新增：

- `replace_selected_text`
- `apply_text_patch`

前端 prompt route 保持不变，但后端 instructions 改为：

- selection edit requests prefer precise edit tools
- not `write_file`

## Migration Step M3：provider prompt contract 改写

当前 prompt 应从：

- “prefer write_file”

改成：

- “for selection-scoped edits, use precise edit tools first; only use write_file for whole-file rewrites or final apply”

## Model-Facing Runtime Repair Plan

After the first TurnProfile / SamplingProfile pass, the next major gap is not
provider reach but model-facing information quality.

Frozen diagnosis:

1. the model still receives weak instructions
2. the model still receives overly raw internal tool-result payloads
3. selection-edit safety still relies too much on soft prompt language
4. task-aware tool forcing is still missing on the main path

This means weak `chat_completions` models are not just "less capable"; they are
being shown the wrong shape of information.

### Phase 1 Bundle: Information Quality + Hard Execution Guard

These tasks should be implemented together. Treat them as a single bundle, not
independent micro-fixes.

1. rewrite `AGENT_BASE_INSTRUCTIONS`
   - replace soft preference language with more explicit operational guidance
   - explain the meaning of context markers such as:
     - `[Currently open file: ...]`
     - `[Selection: @path:startLine:startCol-endLine:endCol]`
     - `[Selected text: ...]`
   - add stronger tool selection guidance

2. sanitize tool feedback before it is returned to the model
   - do not send raw internal control JSON back as the model-visible tool message
   - approval / review-control metadata must be translated into compact,
     model-facing feedback instead of leaking:
     - `approvalRequired`
     - `reviewArtifact`
     - `oldContent`
     - `newContent`
     - `reviewArtifactPayload`

3. add task-aware `tool_choice`
   - `SelectionEdit` / `FileEdit` should not default to unconstrained `"auto"`
   - suggestion-only and analysis turns should avoid forced edit tools

4. add runtime hard guard for `selection_edit`
   - selection-scoped edit turns must not silently fall back to `write_file`
   - precise edit tools should be enforced at runtime:
     - `replace_selected_text`
     - `apply_text_patch`
   - prompt wording alone is not sufficient protection

Project rule:

- do not treat Phase 1 as "prompt tuning"
- treat it as a runtime correctness layer for model-facing information

### Phase 2 Bundle: Tool Reliability

1. fix streamed tool-call id/name assembly
   - tool ids are not normal text deltas and should not be merged with generic
     text-fragment logic

2. improve tool failure messages
   - error feedback should help the model self-correct:
     - include exact failure mode
     - explain the next corrective action
     - prefer "read the file first and retry with exact text" over bare failure

3. enrich tool descriptions with hard boundaries
   - especially for:
     - `apply_text_patch`
     - `replace_selected_text`
   - descriptions must explain exact-match requirements and when not to use the
     tool

4. raise edit-path output/token budgets
   - edit tasks should have more generous output ceilings than suggestion or
     chat turns

### Phase 3 Bundle: Continuity + Fallback

1. preserve tool context in transcript/history reconstruction
   - multi-turn continuation must not lose prior tool calls or tool results

2. extend fallback intent detection for Chinese / multilingual prompts
   - this is a fallback layer only, not the main routing system

3. make turn-loop limits task-aware
   - simple suggestion turns may stay short
   - file/edit/tool turns need higher round budgets

Execution rule:

- complete Phase 1 before Phase 2
- complete Phase 2 before Phase 3
- do not evaluate new providers while these bundles remain incomplete

## Migration Step M4：proposed changes 接 edit artifact，而不是只接 `write_file`

现状：

- proposed changes 主要通过 `write_file` 的 `oldContent/newContent` 进入

目标：

- `replace_selected_text` / `apply_text_patch` 也能产出统一 artifact

## Migration Step M5：审批 runtime 脱离 widget 驱动

现状：

- widget 按钮驱动 auto-resume

目标：

- approval 成为 runtime state transition

## Migration Step M6：event adapter 升级

新增更明确的内部事件：

- `approval_requested`
- `review_artifact_ready`
- `tool_resumed`
- `turn_resumed`

## 六、验收标准

## 必须通过

### 1. 选区改稿安全性

场景：

- 选中 `main.tex` 一段
- 输入 `refine this paragraph`

通过标准：

- 不允许整篇文件被清空并替换成一段文本
- diff 必须只围绕目标区域

### 2. review-first 一致性

通过标准：

- 聊天区不再和 diff panel 争主路径
- blocked edit 必须稳定进入 review artifact

### 3. 审批恢复一致性

通过标准：

- 批准后不需要用户重新描述任务
- 恢复流程由 runtime 负责

### 4. session continuity

通过标准：

- resume 后用户知道：
  - 这轮在做什么
  - 最近改了什么
  - 是否还有 pending review / approval

## 七、执行纪律

1. 不再继续围绕零碎 UI 文案打补丁
2. 先修编辑原语，再谈更像 Claude Code
3. 新 provider 一律后置
4. 所有后续 agent 收口，以这份方案为准，不再按“哪个 bug 先冒出来”随机推进

## 八、当前优先级

## 当前落地状态（2026-04-03）

已完成：

1. `P0` selection-aware edit primitive
   - `replace_selected_text`
   - `apply_text_patch`
   - 选区编辑默认不再以 whole-file `write_file` 为主编辑原语
2. `P1` 公共执行层首轮抽离
   - 新增 `agent/turn_engine.rs`
   - OpenAI / MiniMax 共用 tool execution、status emission、reviewable text-surface 规则
3. `P1` review artifact 统一首轮
   - 新增 `agent/review_runtime.rs`
   - 编辑类工具结果现在携带统一 `reviewArtifactPayload`
   - 前端 proposed changes 已优先兼容统一 artifact
4. `P1` event adapter 首轮升级
   - 新增内部事件：
     - `approval_requested`
     - `review_artifact_ready`
   - 前端不再只能通过字符串 status 反推审批与 review 状态
5. `P2` runtime-level pending/resume permission runtime
   - 审批后的恢复不再由 widget 拼 continuation prompt
   - 后端新增 pending turn state 与 `agent_resume_pending_turn`
   - runtime 负责恢复被审批打断的 turn，并发出结构化 resumed 事件
6. `P2` event adapter 第二轮
   - 新增内部事件：
     - `tool_resumed`
     - `turn_resumed`
   - 前端状态机与 work context 可明确表达“恢复执行”而不是模糊 running
7. `P2` session memory 第二层（working memory 第一层）
   - backend session summary 现在携带：
     - `currentObjective`
     - `currentTarget`
     - `lastToolActivity`
     - `pendingState`
     - `pendingTarget`
   - session selector / active session bar 能表达 pending review/approval 与当前工作对象

说明：

- 这份迁移方案里的核心任务已完成
- 后续 agent 工作一律以这份文档为主线
- 后续若继续扩展，属于 provider 扩展与 Claude Code 高价值能力吸收，不再属于本轮 chat-completions migration 阶段

### P0

- 新增 selection-aware edit primitive
- 禁止选区编辑默认落到 whole-file `write_file`

### P1

- turn engine 抽离
- review artifact 统一化

### P2

- 真正 pending/resume permission runtime
- session memory 第二层

### P3

- OpenRouter provider adapter
- DeepSeek 再评估
