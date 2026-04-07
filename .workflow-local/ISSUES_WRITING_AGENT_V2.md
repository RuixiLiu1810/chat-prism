# Writing Agent v2 可执行 Issue 清单

更新时间：2026-04-06  
来源：`implementation-roadmap.md`（Phase A-E）+ 当前仓库实现现状对照

---

## 使用方式

1. 先完成 `P0`（Phase A + Phase B）再进入 `P1`。  
2. 每条 issue 完成后勾选，并记录 PR/commit。  
3. 每阶段结束执行该阶段“统一回归命令”。

---

## 统一回归命令（每阶段结束必跑）

```bash
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::tools::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::prompt_tests
pnpm tsc --noEmit
```

---

## Phase A：证据主线收敛（P0）

- [ ] **A-01 Prompt 去噪并单入口化**  
  Priority: P0  
  依赖: 无  
  文件: `apps/desktop/src-tauri/src/agent/mod.rs`  
  改动:
  - 将文档主路径统一为 `read_document`。
  - 降级 legacy 文档工具为“兼容层说明”，不作为默认策略。
  验收:
  - 默认指令不再引导 `inspect/search/excerpt/evidence` 多工具串联。
  - prompt tests 全通过。

- [ ] **A-02 附件路径解析稳健化（空格/中文/特殊字符）**  
  Priority: P0  
  依赖: 无  
  文件: `apps/desktop/src-tauri/src/agent/tools/document.rs`, `apps/desktop/src-tauri/src/agent/document_artifacts.rs`, `apps/desktop/src/stores/agent-chat-store.ts`  
  改动:
  - 统一 attachment path canonicalization。
  - 明确 `@attachments/...` 与项目相对路径转换。
  验收:
  - `attachments/Materials Advances, 2025, 6, 7332 - 7354.pdf` 可稳定命中 artifact，不再误报 `No such file`.

- [ ] **A-03 Ingestion 就绪握手机制（queued/processing/ready/failed）**  
  Priority: P0  
  依赖: A-02  
  文件: `apps/desktop/src-tauri/src/agent/document_artifacts.rs`, `apps/desktop/src/stores/agent-chat-store.ts`, `apps/desktop/src-tauri/src/agent/turn_engine.rs`  
  改动:
  - 建立 ingestion 状态机。
  - 未 ready 时返回可恢复状态，不直接 hard fail。
  验收:
  - 文档问答无需反复 re-attach 才可读。

- [ ] **A-04 `read_document` 结果结构统一**  
  Priority: P0  
  依赖: A-03  
  文件: `apps/desktop/src-tauri/src/agent/tools/document.rs`, `apps/desktop/src/lib/agent-message-adapter.ts`, `apps/desktop/src-tauri/src/agent/turn_engine.rs`  
  改动:
  - 统一 evidence 字段：`source/page/snippet/confidence`。
  - legacy shape 在 adapter 层归一。
  验收:
  - 模型反馈与 UI 展示均使用统一 shape。

- [ ] **A-05 文档任务路由收敛（有证据直答，无证据读文档）**  
  Priority: P0  
  依赖: A-04  
  文件: `apps/desktop/src-tauri/src/agent/mod.rs`, `apps/desktop/src-tauri/src/agent/chat_completions.rs`, `apps/desktop/src-tauri/src/agent/openai.rs`  
  改动:
  - 有 `[Relevant resource evidence]` 时 `tool_choice=none`。
  - 无证据时文档任务 `tool_choice=required` 并优先 `read_document`。
  验收:
  - 文档问答平均 tool rounds 下降。

- [ ] **A-06 UI 默认单文档步骤展示**  
  Priority: P0  
  依赖: A-05  
  文件: `apps/desktop/src/components/agent-chat/chat-messages.tsx`, `apps/desktop/src/components/agent-chat/tool-widgets.tsx`  
  改动:
  - 默认仅显示高层 `Read document`。
  - 子步骤仅 debug 可见。
  验收:
  - 默认 UI 无多卡片混杂（inspect/gather/search/excerpt）。

---

## Phase B：写作闭环（P0）

- [ ] **B-01 Section 级任务模型落地**  
  Priority: P0  
  依赖: A-06  
  文件: `apps/desktop/src-tauri/src/agent/provider.rs`, `apps/desktop/src-tauri/src/agent/session.rs`, `apps/desktop/src-tauri/src/agent/mod.rs`  
  改动:
  - 增加 section 任务元数据（objective/audience/style/length）。
  - 引入写作任务 kind（draft/revise/verify）。
  验收:
  - 写作请求可进入 section 状态机，不再仅通用 analysis。

- [ ] **B-02 新增 `draft_section` 工具**  
  Priority: P0  
  依赖: B-01  
  文件: `apps/desktop/src-tauri/src/agent/tools.rs`, `apps/desktop/src-tauri/src/agent/tools/writing.rs`（新增）, `apps/desktop/src-tauri/src/agent/turn_engine.rs`  
  改动:
  - 输出 section 草稿 + evidence bindings。
  验收:
  - 生成结果可追溯到证据块。

- [ ] **B-03 新增 `verify_claims_with_evidence` 工具**  
  Priority: P0  
  依赖: B-02  
  文件: `apps/desktop/src-tauri/src/agent/tools.rs`, `apps/desktop/src-tauri/src/agent/tools/writing.rs`, `apps/desktop/src-tauri/src/agent/telemetry.rs`  
  改动:
  - claim 分类：`supported/weak/unsupported`。
  - 输出每条 claim 的证据链。
  验收:
  - 可统计 `unsupported_claim_rate`。

- [ ] **B-04 新增 `revise_section` 工具**  
  Priority: P0  
  依赖: B-03  
  文件: `apps/desktop/src-tauri/src/agent/tools.rs`, `apps/desktop/src-tauri/src/agent/tools/writing.rs`, `apps/desktop/src/components/agent-chat/tool-widgets.tsx`  
  改动:
  - 对 weak/unsupported claim 自动最小修订。
  验收:
  - 修订结果可显示“修订前后 + 证据变化摘要”。

- [ ] **B-05 Section 闭环编排器（plan -> draft -> verify -> revise）**  
  Priority: P0  
  依赖: B-04  
  文件: `apps/desktop/src-tauri/src/agent/turn_engine.rs`, `apps/desktop/src-tauri/src/agent/mod.rs`  
  改动:
  - 写作闭环最大轮次与终止条件。
  - 证据不足时自动降级表达强度。
  验收:
  - 一次请求可完成可审计的 section 闭环。

---

## Phase C：记忆与连续性（P1）

- [ ] **C-01 扩展 lightweight memory schema**  
  Priority: P1  
  依赖: B-05  
  文件: `apps/desktop/src-tauri/src/agent/session.rs`  
  改动:
  - 在 `AgentSessionWorkState` 增加 `audience/style/terminology/decisions`。
  验收:
  - 序列化兼容旧会话。

- [ ] **C-02 Turn 后记忆更新钩子**  
  Priority: P1  
  依赖: C-01  
  文件: `apps/desktop/src-tauri/src/agent/turn_engine.rs`, `apps/desktop/src-tauri/src/agent/session.rs`, `apps/desktop/src-tauri/src/agent/mod.rs`  
  改动:
  - 从 assistant/tool 结果提炼结构化记忆。
  验收:
  - 多轮任务术语和目标一致性提升。

- [ ] **C-03 Selective recall 打分与 token cap**  
  Priority: P1  
  依赖: C-02  
  文件: `apps/desktop/src-tauri/src/agent/mod.rs`  
  改动:
  - recall 改为相关性排序 + 上限注入。
  验收:
  - prompt 噪声下降，误导回忆降低。

- [ ] **C-04 记忆可视化与手动清理入口（debug）**  
  Priority: P1  
  依赖: C-03  
  文件: `apps/desktop/src/components/agent-chat/session-selector.tsx`, `apps/desktop/src/stores/agent-chat-store.ts`  
  改动:
  - debug 展示当前记忆摘要。
  - 支持清理当前会话记忆。
  验收:
  - 记忆问题可被用户主动恢复。

---

## Phase D：恢复链与预算治理（P1）

- [ ] **D-01 统一预算配置中心**  
  Priority: P1  
  依赖: C-04  
  文件: `apps/desktop/src-tauri/src/settings.rs`, `apps/desktop/src-tauri/src/agent/chat_completions.rs`, `apps/desktop/src-tauri/src/agent/openai.rs`, `apps/desktop/src-tauri/src/agent/turn_engine.rs`  
  改动:
  - 统一 per-call max_tokens 与 turn budget 规则。
  - 按任务类型动态预算。
  验收:
  - `output budget exceeded` 报错率显著下降。

- [ ] **D-02 错误恢复链标准化**  
  Priority: P1  
  依赖: D-01  
  文件: `apps/desktop/src-tauri/src/agent/chat_completions.rs`, `apps/desktop/src-tauri/src/agent/openai.rs`  
  改动:
  - 针对 `max_output_tokens`、`round_limit`、`prompt too long` 建立恢复链。
  验收:
  - 长任务失败后可继续或降级完成。

- [ ] **D-03 Tool loop 去重与防抖**  
  Priority: P1  
  依赖: D-02  
  文件: `apps/desktop/src-tauri/src/agent/turn_engine.rs`, `apps/desktop/src-tauri/src/agent/telemetry.rs`  
  改动:
  - 相同工具+参数短时重复拦截。
  验收:
  - 重复命令执行显著减少。

- [ ] **D-04 审批挂起语义修复**  
  Priority: P1  
  依赖: D-03  
  文件: `apps/desktop/src-tauri/src/agent/turn_engine.rs`, `apps/desktop/src/stores/agent-chat-store.ts`, `apps/desktop/src/components/agent-chat/approval-card.tsx`  
  改动:
  - `approval_required` 后立即 suspend，等待用户动作。
  - 未动作时不可继续输出/结束 completed。
  验收:
  - allow/deny/once 行为与挂起状态严格一致。

- [ ] **D-05 MiniMax 深度与长度调优**  
  Priority: P1  
  依赖: D-04  
  文件: `apps/desktop/src-tauri/src/agent/mod.rs`, `apps/desktop/src-tauri/src/settings.rs`, `apps/desktop/src-tauri/src/agent/chat_completions.rs`  
  改动:
  - AnalysisDeep 默认更高输出预算。
  - 文档问答默认结构化中长回答模板。
  验收:
  - “回复过短”体感显著改善。

- [ ] **D-06 Provider 路由一致性修复**  
  Priority: P1  
  依赖: D-05  
  文件: `apps/desktop/src-tauri/src/agent/mod.rs`, `apps/desktop/src-tauri/src/agent/openai.rs`, `apps/desktop/src-tauri/src/agent/chat_completions.rs`  
  改动:
  - 清晰标注 provider 能力级别（ready/experimental/disabled）。
  验收:
  - UI 状态与后端真实能力一致。

---

## Phase E：质量与可观测性面板（P1）

- [ ] **E-01 指标扩展（写作质量）**  
  Priority: P1  
  依赖: D-06  
  文件: `apps/desktop/src-tauri/src/agent/telemetry.rs`  
  改动:
  - 新增 `unsupported_claim_rate`、`citation_traceability_rate`、`user_rewrite_rate`。
  验收:
  - 可按任务类型输出质量指标。

- [ ] **E-02 评测集与回归脚本**  
  Priority: P1  
  依赖: E-01  
  文件: `.workflow-local/`（新增评测规范）, `scripts/`（新增）  
  改动:
  - 构建 20-50 条文档问答/写作 gold 样本。
  - 自动输出成功率、轮次、延迟、可追溯率。
  验收:
  - 每次改动可自动对比回归结果。

- [ ] **E-03 可观测面板（任务维度）**  
  Priority: P1  
  依赖: E-02  
  文件: 前端 debug/ops 页面（按你当前页面结构落地）  
  改动:
  - 指标趋势、失败样本、按 provider/task 过滤。
  验收:
  - 可在 1 天内定位问题阶段与模块。

---

## 严格执行顺序

1. A-01  
2. A-02  
3. A-03  
4. A-04  
5. A-05  
6. A-06  
7. B-01  
8. B-02  
9. B-03  
10. B-04  
11. B-05  
12. C-01  
13. C-02  
14. C-03  
15. C-04  
16. D-01  
17. D-02  
18. D-03  
19. D-04  
20. D-05  
21. D-06  
22. E-01  
23. E-02  
24. E-03

