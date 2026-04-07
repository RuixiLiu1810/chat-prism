# 任务看板

## In Progress
- [ ] Phase 8（Workflow 收敛）：S0-S2 已落地，进入 S3 可选评估阶段（S4 继续 Parked）
- [ ] Phase 10（Agent UX Hardening）：收口 tool UX / cancel / 状态机可观测性
- [ ] Phase 11（Tool Governance）：从 `write_file` / `run_shell_command` 的最小审批流开始推进
- [ ] Chat Completions Agent 回归：按 `CHAT_COMPLETIONS_AGENT_REGRESSION_CHECKLIST.md` 持续验证选区精细编辑 / approval-resume / review-first diff / session continuity
- [ ] Post-Phase-13：基于外部 Claude Code docs 参考，推进轻量 session memory / 权限规则来源 / 文档摄取优先级，而不是继续找"隐藏 skill"补洞

## Next
- [ ] Structural Runtime Gap / P2：history 强类型化 + reasoning buffer 解耦
- [ ] Post-Phase-13 / P1：在现有 `ToolApprovalRecord` 之上继续补 rule provenance / denial tracking，不把权限系统停留在"可持久化状态"层
- [ ] Tool System Consolidation / T4+：OCR fallback / image-only document recovery（未来 fidelity upgrade，不阻塞当前 resource-driven tool platform）
- [ ] Phase 10：完善 agent 功能（tool UX、cancel/interrupt、错误与状态反馈）
- [ ] Phase 11：补齐 Tool Governance（写文件 / shell 审批、session 级授权）
- [ ] Phase 12：引入 Proposed Changes / Diff Review
- [ ] Phase 12：选区编辑类请求（如 `refine this paragraph`）默认不应只返回 prose 建议，应进入"可审阅的编辑路径"
- [ ] Agent 体验收口：优先缩小 `chat_completions` runtime 与原始 Claude Code 的行为差距（执行偏好 / 权限摩擦 / review 节奏 / session continuity），而不把问题简化成"模型更弱"
- [ ] Phase 9.5 / B4：DeepSeek 继续维持未升格状态，默认不投入主线开发
- [ ] S3（可选）：基于 S1.5 数据评估类型路由加权偏置是否上线
- [ ] 段落模式继续受控实验（默认不开复杂 claim UI）
- [ ] S4 继续 Parked，待 S3 有明确收益后再评估

## Done

See `ARCHIVE.md` for completed items (244 entries archived on 2026-04-07).
