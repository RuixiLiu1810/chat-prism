# 会话日志

## 2026-04-04
- 完成你确认的 `Phase 3: Prompt 与路由降噪` 首轮落地（文档问答策略层收敛，不另起一套 loop 引擎）。
  - 路由策略：
    - 文档证据已在 prompt 中可用时：`tool_choice=none`，直接组织回答
    - 二进制附件分析且证据不足时：`tool_choice=required`，强制走文档工具而不是 `auto` 漫游
  - 指令去重：
    - 附件相关指令改为更短、更少重复，保留核心约束（`read_document` / 禁止二进制 `read_file` / 非显式请求不做 shell exploratory extraction）
  - 测试与验证：
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::prompt_tests`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - 全部通过

- 启动并完成了 `Phase 14: Document Read Flow Simplification` 首轮收口（先固化 workflow，再执行代码）。
  - workflow 固化：
    - `AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md` 新增 Phase 14（P14-A/B/C/D）
    - `TASK_BOARD.md` 新增并勾选 Phase 14 子任务（首轮完成状态）
  - 本轮改动（runtime）：
    - 新增 canonical 文档工具 `read_document`（`path` 必填，`query/limit` 可选）
    - `default_tool_specs()` 不再向模型暴露 `inspect_resource/read_document_excerpt/search_document_text/get_document_evidence`
      - 这些 legacy document handlers 仍保留执行层兼容，避免旧 transcript/历史调用直接断裂
    - binary attachment policy / prompt / model-facing feedback 全部收敛到 `read_document` 单入口建议
    - `read_file` 对文档资源的错误提示改为直接引导 `read_document`
  - 本轮改动（UI）：
    - tool widget 映射新增 `read_document` 主语义（“Read document ...”），减少文档读取轨迹噪音
    - legacy tool name 仍保留展示兼容
  - 回归验证：
    - `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::tools::tests`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::prompt_tests`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

## 2026-04-03
- 在默认产品路径回切到 Claude CLI 之后，继续完成了“可选 runtime”收口，不再让聊天链只剩单一路径。
  - 本轮改动：
    - settings schema/store/Rust loader 新增 `integrations.agent.runtime = claude_cli | local_agent`
    - Settings 面板的 Agent Runtime 区域改为真实可保存的 `Chat Runtime` 选择，不再只是展示只读的 runtime mode
    - `agent-chat-store` 现在会按 runtime 分叉：
      - `claude_cli`：`execute_claude_code / resume_claude_code / cancel_claude_execution / list_claude_sessions / load_session_history`
      - `local_agent`：`agent_start_turn / agent_continue_turn / agent_cancel_turn / agent_set_tool_approval / agent_resume_pending_turn / agent_reset_tool_approvals / agent_list_sessions / agent_get_session_summary / agent_load_session_history`
    - `session-selector` 也会按 runtime 切换 session 源，不再固定只看 Claude sessions
  - 结果：
    - 当前产品主链不再是“要么全部回退、要么全部切到本地 agent”
    - 可以保留 Claude CLI 体验，同时让本地 agent runtime 作为显式可选模式继续存在
  - 验证：
    - `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop exec vitest run src/__tests__/lib/settings-schema.test.ts src/__tests__/lib/settings-api.test.ts`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 启动并完成了 PDF/DOCX `page-aware evidence surfacing` 首轮，不再只把 attachment search hits 扁平地塞进 prompt。
  - 本轮改动：
    - 新增 `formatRelevantResourceEvidence(...)`
      - 证据现在按文档分组，并保留 page / paragraph 标签
      - prompt block 从“零散 match 列表”升级成更稳定的：
        - `Document: ...`
        - `Page N / Paragraph N: snippet`
    - send-time prompt assembly 已改为注入 `[Relevant resource evidence: ...]`
      - 不再按每个附件单独重复展开一小串扁平 matches
    - attachment analysis instructions 同步加强：
      - 明确建议回答结构为：
        - Matching documents
        - Supporting evidence
        - Conclusion
      - 若 ingestion 证据不足，应直接说明，而不是继续发明工具调用或 shell 步骤
  - 结果：
    - PDF/DOCX 问答开始具备 `document -> page/paragraph -> snippet` 的 evidence 形态
    - 这更接近 Claude 的 document-ingestion 心智，而不是命令行提取结果堆叠
  - 验证：
    - `npx pnpm --filter @claude-prism/desktop exec vitest run src/lib/resource-ingestion.test.ts src/__tests__/lib/agent-message-adapter.test.ts`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib prompt_tests`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 参考 Claude 官方 PDF 支持文档与本地 `claude-code-fork-main` 附件链路，对“Claude 是如何读 PDF 的”做了一次对标校准，并把结论固化进 workflow。
  - 核心判断：
    - Claude 的 PDF 能力应被理解为 document ingestion 能力，而不是 prompt 指挥模型去运行 `pdftotext`
    - 官方公开能力表明 PDF 作为一等文档输入进入模型；实际效果来自文档资源表示，而不是 shell probing
    - 本地 `claude-code-fork-main` 也印证了客户端侧附件是先下载/落盘/引用，再交给 runtime 的文档读取路径处理
  - 对 ClaudePrism 的直接启发：
    - 对 PDF/DOCX 的后续对齐方向应是：
      1. 更高保真的 ingestion
      2. page-aware evidence surfacing
      3. `document -> page -> snippet` 的回答结构
    - 不再把“找一个 read docx skill”当成主解
    - 不再把 shell 提取路径当成默认文档阅读主线
  - workflow 已同步：
    - `AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md`
    - `TASK_BOARD.md`

- 收掉了一条真实的 attachment-analysis 失败路径：PDF/DOCX 附件虽然已经有 ingestion-first excerpt/search，但 agent 仍会在 analysis turn 中继续调用 `read_file` / `run_shell_command`，退化成反复 `pdftotext` probing，最终触发 round-budget abort。
  - 根因：
    - attachment-backed analysis 之前仍是 `tool_choice = auto`
    - runtime 没有在执行层阻止对二进制附件的 `read_file` 和 shell probing
    - 结果是模型即使已经拿到 `[Attached excerpt]` 和 `[Relevant resource matches]`，仍可能继续做错误的二次探索
  - 本轮修复：
    - binary attachment analysis 现在会把 `tool_choice` 直接降为 `none`
    - runtime hard guard 新增：
      - PDF/DOCX analysis turn 禁止 `run_shell_command`
      - PDF/DOCX analysis turn 禁止对 `.pdf/.docx` 调 `read_file`
    - instructions 也同步加强：
      - 明确说明不要再对二进制附件使用 `pdftotext` / raw read
      - 应优先消费 prompt 里已经给出的 excerpts 和 relevant matches
  - 验证：
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib prompt_tests`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - 全部通过

- 完成 Post-Phase-13 第二条 follow-up：PDF/DOCX ingestion-first 的 excerpt/search 主线首轮落地。
  - 之前的问题：
    - PDF 附件虽然已经不再伪装成 selection，但本质上仍只是 pin 时提取一小段 excerpt
    - DOCX 更严重：应用能预览，但 agent 没有 ingest 链，`buildPinnedContextForFile` 甚至会把 `.docx` 误当成普通 `other` 文本文件
    - 结果是 attachment-backed 问题仍容易退化成 shell probing，而不是先消费已附带资源
  - 本轮修复：
    - 新增前端 `resource-ingestion` 资源摄取层
      - PDF：基于 MuPDF 做 page-aware text ingestion
      - DOCX：基于 Mammoth 做 raw text ingestion
      - generic text attachments：走轻量文本 fallback
    - pin 资源时：
      - PDF / DOCX 现在都会进入统一 ingestion cache
      - `AgentPromptContext` 增加 `absolutePath`
    - 发送 prompt 时：
      - 对 attachment context 按当前用户问题做本地 lexical search
      - 新增 `[Relevant resource matches: ...]` prompt block
      - 让 agent 在真正分析前先看到高信号 excerpt + 匹配片段
    - prompt 语义同步：
      - instructions 现在明确把 relevant resource matches 视为高信号证据候选
      - attachment-backed analysis 优先消费这些上下文，而不是继续走 shell probing
  - 结果：
    - PDF/DOCX 不再只是“文件名 + 一小段随机摘录”
    - attachment 分析主线第一次具备了 excerpt/search 闭环
  - 验证：
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop exec vitest run src/lib/resource-ingestion.test.ts`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 完成 Post-Phase-13 第一条 follow-up：lightweight session memory / selective recall 首轮落地。
  - 关键判断：
    - 之前虽然已经把 `current_objective / current_target / last_tool_activity` 注入 prompt，
      但它更像 work-state dump，不是真正的 selective recall。
    - 更严重的是，`record_request_objective(...)` 会在每轮 turn 前刷新 `current_objective`，
      导致模型拿到的往往只是当前请求的回声，而不是连续工作上下文。
  - 本轮修复：
    - `AgentSessionWorkState` 新增 `recent_objective`
      - 当新 objective 进入时，旧 objective 会保留下来，作为轻量连续性线索
    - prompt 组装从 `[Current work state]` 原样注入，改成 `[Selective session recall]`
      - 只注入高信号连续性线索，而不是整包 work-state
      - 当前优先级：
        1. pending state
        2. recent objective
        3. task-relevant current target
        4. continuity-relevant last tool activity
        5. current objective（仅当不等于当前请求摘要时）
    - 结果：
      - runtime 不再把当前请求伪装成“memory”再喂回模型
      - 轻量 session memory 开始真正具备 selective recall 的形态
  - 验证：
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib prompt_tests`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::session::tests`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - 全部通过

- 参考 `https://ccb.agent-aura.top/docs/` 再次校准了 Claude Code 类 agent 的系统视角，并把收获固化成 Phase 13 之后的新 follow-up 方向。
  - 关键收获不是“又多了几个功能”，而是再次确认 Claude Code 强在闭环：
    - turn engine
    - message adapter
    - permission pipeline
    - review-first diff
    - session continuity
  - 对 ClaudePrism 的直接启发被收成 3 条后续纪律：
    1. 先做 lightweight session memory / selective recall
       - 不直接跳到重型 memory 系统
       - 优先复用已有 `current_objective / current_target / last_tool_activity`
    2. 权限系统继续从“可持久化状态”升级为“带 provenance 的规则层”
    3. 对 PDF / DOCX 等富文档，优先补 ingestion / excerpt / search 主线
       - 不再把“有没有 read docx skill”当成主要解法
  - workflow 已同步：
    - `AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md`
    - `TASK_BOARD.md`

- 完成 Phase 13 的首轮闭环收口，不再只是“参考 Claude Code 思路”，而是把高价值能力真正收成 provider-neutral 的前端/运行时边界。
  - Message adapter
    - 新增前端 `agent-message-adapter`
    - live `tool_result` events 与恢复历史里的 `tool_result` 现在都会统一规范成 UI-safe display content
    - tool widgets 不再直接依赖混合 raw JSON / string / error object 三种内容形态
  - 结果：
    - React 侧终于有了明确的 message adapter 边界
    - Phase 13 优先级最高的三条能力：
      - message adapter
      - permission runtime
      - review-first diff
      已可视为首轮完成
  - 验证：
    - `npx pnpm --filter @claude-prism/desktop exec vitest run src/__tests__/lib/agent-message-adapter.test.ts src/__tests__/stores/multi-tab-merge.test.ts`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 收口一轮新的 agent 界面层级与 MiniMax 深分析能力，不再把 approval 逻辑塞在 tool widget 里，也不再让文档/附件问答默认走偏浅的 balanced 档。
  - Approval / UI hierarchy
    - 新增 chat-level approval interrupt card
      - approval 现在显示在消息流底部、输入框上方，而不是嵌在 `run_shell_command` / `write_file` widget 内部
    - tool widget 退回轻量执行轨迹
      - `write_file` / precise edit / shell widget 不再内嵌审批按钮
      - review-ready / awaiting-approval 只保留轻量提示，避免和 diff panel / approval card 抢主界面心智
    - 聊天抽屉顶部信息降噪
      - session info 从多行卡片压成单行 compact session bar
      - status banner 仅保留失败 / 取消 / approval-resume 等真正需要用户注意的状态
  - Runtime / sampling
    - 新增正式 `analysisDeep` profile
      - 默认值：`temperature=0.3`, `top_p=0.92`, `max_tokens=12288`
    - attachment/resource 分析请求默认路由到 `analysisDeep`
    - 后端 instructions 现在会对 attachment-backed analysis 明确要求：
      - 基于附件资源给出结论
      - 说明哪些资源支持关键判断
      - 避免一句话就结束的过浅回答
    - Settings schema / Rust runtime loader / Settings UI 已同步新增 `analysisDeep`
  - 验证：
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop exec vitest run src/__tests__/stores/multi-tab-merge.test.ts src/__tests__/lib/settings-schema.test.ts src/__tests__/lib/settings-api.test.ts`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 完成工具层 correctness 主线的 Batch A/B/C，重点不再是“agent 体感”抽象问题，而是把仍然真实的工具层硬错误真正收口。
  - Batch A（tool correctness）
    - 清理了幽灵工具 `replace_file_range`
      - 从 precise/reviewable edit 判定、turn engine、tool widgets 中移除残留引用
    - `run_shell_command` 现在有了：
      - 30s timeout
      - stdout/stderr 32KB 上限
      - 截断标记字段
    - 编辑审批 bucket 现在分为：
      - `write_file`
      - `patch_file`
      避免精确 patch 与整文件覆写共用同一风险桶
  - Batch B（tool reliability）
    - `read_file` 改为 UTF-8 / 行边界安全截断，不再直接 raw byte cut + `from_utf8_lossy`
    - `apply_text_patch` 增加保守的 trim-based fallback
    - `list_files/search_project` 在 `rg` 不可用时返回明确 preflight error，而不是原始 OS spawn 错误
  - Batch C（tool/UI cleanup）
    - `tool-widgets.tsx` 已移除旧 Claude tool aliases 与死分支（含 `askuserquestion` / `todowrite`）
    - Tool status icon 不再直接读取全局 `isStreaming`，而是由当前消息上下文显式传入
    - `chat-messages.tsx` 移除了 assistant/result 文本去重
    - approval preview 不再通过 `getApprovalPayload()` 额外复制 `oldContent/newContent`
  - 同步补充：
    - shell approval 在展开时现在会显示 approval reason，而不再因为 preview 为空而“看不到审批按钮”
    - preview changed-line summary 改成 prefix/suffix 感知的近似 diff，而不是逐行索引暴力比较
  - 验证：
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::tools::tests`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib process_utils::tests`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 对一轮新的“Agent 工具逻辑全面评估报告”做了逐项去重，明确哪些问题仍然真实、哪些已经过期，并把结果收成新的工具层 correctness 主线。
  - 已确认过期/不再进入主线的项：
    - `chat_completions` 未调用 `to_chat_completions_tool_schema(...)`
      - 当前 `chat_completions.rs` 已通过 `to_chat_completions_tool_schema(spec, &config.provider)` 发 schema
    - shell approval UI 仍提示用户去 review diff
      - 当前 `run_shell_command` 的批准文案已改成 command-specific wording
  - 仍然真实、而且优先级最高的项：
    - `replace_file_range` 幽灵工具名仍残留在 precise/reviewable edit 判定与 widget 路由中
    - `run_shell_command` 仍缺 timeout 与输出大小上限
    - `read_file` 仍采用 raw byte truncate + `from_utf8_lossy`
    - `apply_text_patch` 仍缺保守的 fallback/retry
    - 精确 patch 与 `write_file` 仍共用同一个 approval bucket
    - `list_files/search_project` 仍硬依赖 `rg`
    - 前端 widget 层仍保留多处旧 Claude alias / global streaming / 脆弱 dedupe
  - workflow 主线已去重重排为三批：
    1. Tool correctness batch A
       - ghost tool
       - shell timeout/output caps
       - approval bucket split
    2. Tool reliability batch B
       - safe truncation
       - patch fallback
       - `rg` fallback/preflight
    3. Tool/UI cleanup batch C
       - dead aliases/widgets
       - per-tab status
       - remove fragile dedupe
       - reduce approval payload coupling

- 修复 attachment / PDF context 主链的结构性错误，避免 PDF 附件再被伪装成 selection 上下文。
  - 之前的问题：
    - chat composer 会把 PDF/非文本附件降级成 `[Attached file: ...]` 占位字符串
    - 发送时又统一包装成 `[Selection] + [Selected text]`
    - 同时继续注入 `[Currently open file: ...]`
    - 导致 agent 在“问附件里哪篇文章提到 hydrophobic”这类请求上，被错误引向当前打开的 tex 文件和项目 grep
  - 本轮修复：
    - `AgentPromptContext` 现在区分：
      - `selection`
      - `file`
      - `attachment`
    - attachment 不再走 selection marker
    - prompt 现在新增：
      - `[Attached resource: ...]`
      - `[Resource path: ...]`
      - `[Attached excerpt: ...]`
    - attachment-only 请求不再默认带当前 active file bias
    - pin PDF 文件时开始用现有 MuPDF worker 提取真实文本摘录，而不是只塞文件名占位符
  - 影响：
    - 这不是模型层小修，而是修正了“附件语义编码错误”的主路径
    - agent 现在至少能看到真实的 PDF 摘录内容，而不是把 PDF 当作无法读取的普通路径
  - 验证：
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 完成去重后的 `Sprint 2/3/4` 主线交付，并把状态正式回写到 workflow。
  - `Sprint 2: executioner stability`
    - `TurnBudget` 已进入运行时主路径
    - cancel/abort 已继续传播到 tool execution
    - `current_objective / current_target / last_tool_activity / pending state` 已注入模型上下文
  - `Sprint 3: permission runtime hardening`
    - pending turn 已持久化到本地 app config
    - approval / allow-once 已加入 TTL / stale cleanup
    - `ToolApprovalRecord { decision, source, granted_at, expires_at, remaining_uses }` 已落地
  - `Sprint 4: concurrency + compatibility + observability`
    - 只读工具已改为安全并发批执行
    - 写/变更工具保持串行
    - MiniMax-first tool schema adapter 已接入 provider path
    - 轻量 structured telemetry 已落地到本地 JSONL 日志
  - 验证：
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::`
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 基于对 `claude-code-fork-main` 的再次对比，进一步去重了 ClaudePrism 剩余差距，避免把已完成的 model-facing 修复重新当成主线任务。
  - 这次冻结的核心判断：
    - 真正剩余的差距，不再是 prompt/tool feedback 这类信息质量问题
    - 主要是“五个子系统没有闭环”：
      - turn execution
      - message adaptation
      - permission runtime
      - review-first edit artifacts
      - session working memory
  - 因此把后续主线去重成三个 Sprint：
    1. `Sprint 2: executioner stability`
       - `TurnBudget`
       - tool-level abort propagation
       - work-state prompt injection
    2. `Sprint 3: permission runtime hardening`
       - pending turn persistence
       - TTL / stale cleanup
       - approval record structure
    3. `Sprint 4: concurrency + compatibility + observability`
       - read-only tool parallelism
       - provider tool schema adapter
       - lightweight local telemetry
  - 执行纪律：
    - 不把已完成的 model-facing bundle 重新排进主线
    - Sprint 2 → Sprint 3 → Sprint 4 顺序执行
    - DeepSeek 继续 parked，直到这三轮闭环收口显著改善本地 agent 体感

- 固化一轮新的“model-facing runtime repair”方案，核心判断是：当前 agent 智能度不足，不应优先归因于 MiniMax 之类 provider 的模型能力，而要先修 runtime 传给模型的信息形态。
  - 用户提出的高价值判断：
    1. system prompt 太弱
    2. tool result 把内部控制 JSON 直接喂回模型
    3. 工具失败后的错误信息没有自我纠错价值
    4. selection edit 仍然缺 runtime 级强约束
  - 这轮被正式重排成三个 bundle：
    - Phase 1：信息质量 + 执行硬约束
      - 重写 `AGENT_BASE_INSTRUCTIONS`
      - 明确上下文标记语义
      - 清洁化 model-facing tool feedback
      - task-aware `tool_choice`
      - selection-edit runtime hard guard
    - Phase 2：工具可靠性
      - 修 streamed tool-call id/name 合并
      - 丰富工具错误上下文
      - 丰富工具描述边界
      - 提高 edit path token/output budget
    - Phase 3：连续性 + fallback
      - 修 transcript/history 的 tool context
      - 补中文/多语言 fallback intent detection
      - 按任务类型设置 turn loop 上限
  - 冻结规则：
    - Phase 1 不是“prompt 调教”，而是 runtime correctness
    - 不再把 Phase 1 拆成孤立小修
    - 在这三个 bundle 完成前，不继续扩 provider 主线
  - 当前状态更新：
    - `Phase 1/2/3 Bundle` 首轮实现已完成
    - 已落地：
      - 更强的 base instructions 与上下文标记语义说明
      - 清洁化 model-facing tool feedback
      - task-aware `tool_choice`
      - `selection_edit -> write_file` runtime hard guard
      - streamed tool-call id/name 合并修复
      - 更强的工具错误与工具描述边界
      - 更高的 edit/analysis output budget
      - transcript/history 中的可见 tool context 重建
      - 中文 fallback intent detection
      - 按任务类型设置的 round budget

- 固化一轮新的 agent runtime 结构性优先级判断，目的不是继续围着 provider 扩展或小交互打补丁，而是优先解决执行器本身的剩余硬缺陷。
  - 背景判断：
    - 用户提供的一轮全面分析里，有几条是成立且高优先级的：
      - tool execution 仍然串行
      - selection edit 仍需 runtime 级强约束，不能只靠 prompt 提示
      - turn loop 硬编码 `0..6` 过低
      - pending turn 仍未持久化
      - tool schema 仍缺 provider adapter
      - history 仍是 `serde_json::Value`
      - reasoning merge 仍较脆弱
    - 也修正了两条边界：
      - OpenAI 当前主链不是“完全 skeleton”，但遗留 trait 入口仍是死接口，后续应收口
      - DeepSeek 不应因为格式近似就立即升格，继续 parked 才是正确策略
  - workflow 冻结的新顺序：
    1. `selection_edit` runtime hard guard
    2. safe parallel tools（只读并发、写操作串行）
    3. round-budget uplift（不再固定 6 轮）
    4. pending turn persistence + TTL
    5. provider tool-schema adapter
    6. typed history + reasoning cleanup
  - 这一步的意义：
    - 现在 MiniMax / chat-completions 路径的主要差距，不再模糊归因为“模型弱”
    - 后续主线明确回到 runtime 结构本身
    - provider 扩展被降级到次要位置

- 完成 `TurnProfile / SamplingProfile` 第二轮收口，把“内部默认值”升级成正式 runtime config，并把显式 UI 动作与 profile 直接绑定。
  - `apps/desktop/src/lib/settings-schema.ts`
    - `integrations.agent` 新增 `samplingProfiles`
      - `editStable`
      - `analysisBalanced`
      - `chatFlexible`
    - 全局默认值、sanitize/migration 已同步接入
  - `apps/desktop/src/stores/settings-store.ts`
    - 初始 effective settings 同步补齐 agent sampling profiles
  - `apps/desktop/src-tauri/src/settings.rs`
    - Rust schema sanitize/default/validation 已接入 `samplingProfiles`
    - `AgentRuntimeConfig` 新增正式 `sampling_profiles`
  - `apps/desktop/src/components/workspace/settings-dialog.tsx`
    - Agent Runtime 区域新增 `Sampling Profiles` 配置块
    - 支持编辑：
      - `Edit Stable`
      - `Analysis Balanced`
      - `Chat Flexible`
    - 参数包括：
      - `temperature`
      - `top_p`
      - `max_tokens`
  - provider 层：
    - `apps/desktop/src-tauri/src/agent/chat_completions.rs`
    - `apps/desktop/src-tauri/src/agent/openai.rs`
    - 采样参数现在优先来自 runtime settings，而不再只靠硬编码 fallback
  - 显式 UI 行为：
    - `apps/desktop/src/stores/agent-chat-store.ts`
      - `sendPrompt(...)` 新增 `turnProfileOverride`
    - `apps/desktop/src/components/workspace/editor/latex-editor.tsx`
      - proofread / lint-fix / fix-all now pass explicit edit/file-edit profiles
    - `apps/desktop/src/components/workspace/preview/pdf-preview.tsx`
      - selection proofread / compilation-fix now pass explicit profiles
  - 这一步的意义：
    - 现在 agent 不再只是在 provider 里“偷偷调参数”
    - 而是把任务画像、执行偏好与采样策略一起收进正式 runtime contract
    - 也开始真正实践从 Claude Code fork 学到的核心原则：
      - 高价值行为尽量显式定义
      - 关键词只做弱兜底
  - 验证：
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib prompt_tests`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib chat_completions`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop exec vitest run src/__tests__/lib/settings-schema.test.ts src/__tests__/lib/settings-api.test.ts`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过

- 固化并落地 `TurnProfile / SamplingProfile` 首轮重构，目的不是继续堆 prompt engineering，而是把任务画像、执行偏好与采样策略收回到统一 runtime 协议层。
  - 背景判断冻结：
    - 之前的核心问题不是单一 provider，而是前后端双层关键词硬路由：
      - 前端 `agent-chat-store.ts` 负责猜 `suggestion/reviewable_edit/neutral`
      - 后端 `agent/mod.rs` 又重复做 prompt heuristic
      - provider 请求体没有按任务类型切 sampling params
    - 这会对 MiniMax 这类较弱模型造成双重损失：
      - 先被路由误判
      - 再被过宽的默认采样放大不稳定性
  - 后端：
    - `apps/desktop/src-tauri/src/agent/provider.rs`
      - 新增统一结构：
        - `AgentTaskKind`
        - `AgentSelectionScope`
        - `AgentResponseMode`
        - `AgentSamplingProfile`
        - `AgentTurnProfile`
      - `AgentTurnDescriptor` 新增 `turn_profile`
    - `apps/desktop/src-tauri/src/agent/mod.rs`
      - 新增 `resolve_turn_profile(...)`
      - `build_agent_instructions(...)` 改为消费整个 request，而不是只吃 prompt 字符串
      - prompt heuristic 仍保留，但已降级为弱兜底，不再是主路由
    - `apps/desktop/src-tauri/src/agent/session.rs`
      - pending turn resume 现在会保留 `turn_profile`
    - `apps/desktop/src-tauri/src/agent/turn_engine.rs`
      - pending approval/resume 路径同步保存并传递 `turn_profile`
  - Provider 层：
    - `apps/desktop/src-tauri/src/agent/chat_completions.rs`
      - `transcript_to_chat_messages(...)` 改为消费 `AgentTurnDescriptor`
      - MiniMax 请求体开始按 `sampling_profile` 注入内建参数：
        - `edit_stable` -> `temperature 0.2 / top_p 0.9 / max_tokens 4096`
        - `analysis_balanced` -> `temperature 0.4 / top_p 0.9 / max_tokens 4096`
        - `chat_flexible` -> `temperature 0.7 / top_p 0.95 / max_tokens 4096`
    - `apps/desktop/src-tauri/src/agent/openai.rs`
      - OpenAI Responses 请求体同步开始消费 `sampling_profile`
      - 目前同样注入：
        - `temperature`
        - `top_p`
        - `max_output_tokens`
  - 前端：
    - `apps/desktop/src/stores/agent-chat-store.ts`
      - 删除主路径上的 `[Execution route: ...]` prompt marker
      - 新增 TS 侧 `AgentTurnProfile`
      - `sendPrompt(...)` 现在会把：
        - `taskKind`
        - `selectionScope`
        - `responseMode`
        - `samplingProfile`
        - `sourceHint`
        显式传给 `agent_start_turn / agent_continue_turn`
      - 当前策略已经从“关键词决定硬路由”收成：
        - UI/context signal 优先
        - 关键词仅作弱补充
        - 选区上下文默认更偏 `selection_edit`
  - 测试与验证：
    - `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib prompt_tests`
    - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib chat_completions`
    - `npx pnpm --filter @claude-prism/desktop exec tsc --noEmit`
    - `npx pnpm --filter @claude-prism/desktop build`
    - 全部通过
- workflow 同步更新：
  - `.workflow-local/CHAT_COMPLETIONS_AGENT_MIGRATION_PLAN.md`
  - `.workflow-local/AGENT_RUNTIME_REPLACEMENT_WORKFLOW.md`
  - `.workflow-local/TASK_BOARD.md`

