# Claude Prism Writing Agent 实施路线图 v2

基于 `claude-code-agent-study` 与 `claude-code-executive-summary` 的学习结论，本版本不再以通用 coding agent 为默认目标，而是以“证据驱动的学术写作 agent”为主线。

---

## 0. 北极星目标

把 Claude Prism 从“能调用工具的聊天体”升级为“可追溯、可审计、可迭代的写作协作体”。

核心定义：

1. 每个关键结论都能回溯到来源（文档、页码、片段）。
2. 写作流程以 section 为单位迭代（不是一次性整篇输出）。
3. 失败时优先自动恢复和降级，而不是直接中断。
4. 用户始终能看到当前阶段、证据来源和修改差异。

---

## 1. 可借鉴与不照搬

### 1.1 高价值借鉴（直接采用）

1. AsyncGenerator/事件流驱动的执行循环（可中断、可恢复）。
2. 不可变消息历史 + 明确审计轨迹。
3. 递进式错误恢复链（预算、压缩、降级、重试）。
4. 预算与可观测性（token、轮次、时延、失败类型）。
5. 权限与工具策略外部化（策略数据化，避免硬编码散落）。

### 1.2 写作场景不照搬（避免误配）

1. Bash-first 的探索路径（写作主线不应依赖 shell）。
2. 过早引入多 agent 递归/fork（先把单 agent 质量闭环打稳）。
3. 工具数量膨胀（先做少而强的写作核心工具）。
4. 只优化 token 成本、忽视证据可追溯性（会直接伤害写作可信度）。

---

## 2. 目标架构（写作专用闭环）

```
用户写作请求
  -> 任务路由 (写作/润色/证据问答/改写)
  -> 文档证据层 (ingestion + search + excerpt + quote)
  -> 写作执行层 (plan -> draft_section -> verify -> revise)
  -> 审阅交互层 (diff + approval + retry)
  -> 会话记忆层 (objective/style/terminology/decisions)
  -> 可观测性层 (latency/rounds/fallback/miss rate)
```

---

## 3. 最小写作工具集（MVP）

冻结原则：先强主线，再扩工具。

1. `read_document`：统一 PDF/DOCX 阅读入口。
2. `search_document_text`：关键词检索，返回结构化命中。
3. `get_document_evidence`：按主题汇聚证据块（source/page/snippet）。
4. `draft_section`：按 section 目标生成草稿。
5. `revise_section`：按反馈对既有 section 最小改写。
6. `verify_claims_with_evidence`：校验事实陈述是否有证据支撑。

注：
- 以上 6 个优先于新 shell 能力。
- 文档问答默认走文档专用路径，不走通用 shell 探测。

---

## 4. 分阶段实施（以写作质量为先）

## Phase A: 证据主线收敛（P0，1-2 周）

目标：
- 所有文档问答与写作引用都进入统一 evidence 管道。

任务：
1. 统一文档入口：
   - 强制 `read_document` 为主入口。
   - 历史兼容工具仅保留兼容，不对模型暴露多入口。
2. 证据结构标准化：
   - 统一返回 `source`, `page`, `snippet`, `confidence`。
3. 证据缺失处理：
   - artifact miss/fallback 明确记录与提示，不再“静默失败”。
4. UI 收口：
   - 默认只展示高层 `Read document`。
   - 子步骤只在 debug 展示。

验收标准：
1. 文档问答成功率 >= 95%（有 artifact 的场景）。
2. 证据返回结构字段完整率 >= 99%。
3. 默认 UI 不再出现多卡片文档步骤混杂。

---

## Phase B: 写作闭环（P0，2-3 周）

目标：
- 从“问答工具链”升级为“section 级写作链”。

任务：
1. section 规划：
   - 为每段写作请求生成简短 section 计划（目标、受众、语气、长度）。
2. 分段生成：
   - `draft_section` 输出必须带“证据绑定清单”。
3. 事实校验：
   - `verify_claims_with_evidence` 对每个 claim 打标签：
     - supported / weak / unsupported。
4. 修订循环：
   - 对 weak/unsupported claim 触发自动重写或降级陈述。

验收标准：
1. 无证据陈述率 <= 10%。
2. 用户一次通过率（无需手工大改）>= 60%。
3. 每次修订都可显示明确差异和证据变化。

---

## Phase C: 记忆与连续性（P1，2 周）

目标：
- 让 agent 记住“在写什么、按什么标准写”。

任务：
1. lightweight session memory：
   - 记录 `objective`, `audience`, `style`, `terminology`, `decisions`。
2. selective recall：
   - 下一轮仅注入“当前任务相关记忆”，避免全量回放噪声。
3. 冲突处理：
   - 用户新指令与旧记忆冲突时，优先新指令并写入变更记录。

验收标准：
1. 连续多轮后风格漂移率显著下降。
2. 专有术语一致性 >= 90%。
3. 不出现“记忆污染导致误改任务”的高频问题。

---

## Phase D: 恢复链与预算治理（P1，2 周）

目标：
- 降低“长任务中断/超预算失败”的体感。

任务：
1. 输出预算治理：
   - 区分“单请求 max tokens”和“整轮 turn budget”。
   - turn budget 采用任务感知动态上限。
2. 错误恢复链：
   - output budget exceeded -> 提示继续策略或自动降级续写。
   - prompt too long -> 先压缩后重试。
3. 文档任务专用降级：
   - 有证据时优先 `tool_choice=none` 回答。
   - 无证据时优先文档工具而非 shell。

验收标准：
1. “output budget exceeded”终止率下降 >= 70%。
2. round limit 触发率下降 >= 50%。
3. 文档类任务平均轮次下降。

---

## Phase E: 质量与可观测性面板（P1，1-2 周）

目标：
- 写作质量可量化，改进方向可证据化。

新增指标：
1. `artifact_miss_rate`
2. `fallback_rate`
3. `doc_tool_rounds_per_question`
4. `end_to_end_latency_ms`
5. `unsupported_claim_rate`
6. `citation_traceability_rate`
7. `user_rewrite_rate`

任务：
1. 埋点落盘结构化。
2. 增加按任务类型的统计视图（文档问答/润色/重写/总结）。
3. 建立回归基线与报警阈值。

验收标准：
1. 每周可产出稳定质量报表。
2. 任一核心指标劣化可在 1 天内定位到阶段/模块。

---

## 5. Prompt 与策略升级（写作场景）

冻结规则：

1. 文档相关请求：
   - 有证据 -> 可直接回答（`tool_choice=none`）。
   - 无证据 -> 必须先走文档工具。
2. 改写请求：
   - 优先最小改动（section 或选区级），避免整文重写。
3. 校验请求：
   - 输出必须包含“证据映射”而非纯观点。

建议新增系统指令块：

1. 写作目标约束（受众、语气、篇幅）。
2. 证据引用约束（结论需可追溯）。
3. 修订最小化约束（不破坏无关段落）。
4. 不确定性表达约束（证据不足时降级断言强度）。

---

## 6. 风险与控制

1. 风险：过度追求证据导致写作流畅性下降。  
   控制：分离“草稿生成”和“事实校验”，二阶段输出。

2. 风险：工具链变长导致延迟上升。  
   控制：文档问题优先证据检索，命中后减少无效轮次。

3. 风险：记忆注入过多导致 prompt 噪声。  
   控制：selective recall + token cap + relevance filtering。

4. 风险：指标多但无行动。  
   控制：每个核心指标绑定负责人和阈值策略。

---

## 7. 里程碑与完成定义

## Milestone M1（Phase A+B 完成）

完成定义：
1. 文档读取主线稳定（单入口）。
2. section 级写作闭环上线。
3. unsupported claim 有自动校验与修订。

## Milestone M2（Phase C+D 完成）

完成定义：
1. 记忆与连续性稳定。
2. 预算和恢复链显著降低中断率。
3. 文档问答轮次和失败率可控。

## Milestone M3（Phase E 完成）

完成定义：
1. 质量指标面板可用于周度运营。
2. 关键指标有回归阈值和处置流程。
3. 可以进行下一轮模型/工具优化的量化评估。

---

## 8. 执行顺序（严格）

1. Phase A（证据主线）  
2. Phase B（写作闭环）  
3. Phase C（记忆连续性）  
4. Phase D（恢复与预算）  
5. Phase E（质量面板）

不满足 A/B，不进入 C/D/E。

---

## 9. 一句话执行原则

先把“证据可信 + 写作可控 + 失败可恢复”做稳，再追求“更多模型、更多工具、更多自动化”。
