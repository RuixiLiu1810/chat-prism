# Citation Query Scoring + Embedding Workflow

更新时间：2026-03-30

## 目标
- 让“检索词质量”可观测、可打分、可回归优化。
- 用轻量检索向量模型替代纯规则排序（非替代整套检索流程）。
- 维持当前应用轻量和可回退：模型失败时自动退回规则方案。

## 当前进度
- [x] M1：Query Score 基线（lexical 分项 + Debug 可视化）
- [x] M2：`query_embedding_provider`（none/local_embedding）+ timeout + 自动回退
- [x] M3：top-N + MMR + 预算联动（早停改为“高质量命中率”联动）
- [x] M4：可配置与回退收口（Settings 参数化 + runtime 接入 + Debug 展示）
- [ ] M5：离线评测与默认参数落参
  - 进展：已补 Debug early-stop 解释字段（reason/stage/hitRatio），评测可观测性已达标；样本已达成全标注（labeled=6 / unlabeled=0）。
  - 进展：已支持 `expected.no_match=true` 负样本标注并贯通到 check/evaluate 流程，减少无效样本对 M5 的阻塞。
  - 进展：v3 评测指标 `top1=0.8, top3=1, top5=1, mrr=0.9`，说明 hard case 仍在拉低 top1，默认值暂不写回，先扩样再校准。

## 结论约束（严格口径）
- 可行：轻量 embedding（优于原生 BERT）用于 query rewrite/rerank。
- 不可行：仅靠轻量模型就完全解决引用相关性；仍需后续候选打分。
- 推荐默认模型：`bge-small-en-v1.5`（首选）或 `e5-small-v2`（备选）。

## 架构方案
1. Query 生成层（保留）
- 继续使用现有 rule 生成器产出候选 query（anchor/phrase/keyword）。

2. Query 打分层（新增核心）
- 对每条 query 计算 `query_quality_score`（0~1）：
- `0.45 * semantic_sim`：query 与选中文本语义相似度（embedding cosine）
- `0.25 * anchor_coverage`：材料/方法/形貌锚点覆盖率
- `0.20 * specificity`：信息密度（去停词后 IDF 近似）
- `-0.15 * noise_penalty`：时间/单位/流程噪声惩罚
- `-0.05 * length_penalty`：过短/过长惩罚

3. Query 选择层（新增）
- 按 `query_quality_score` 排序，只取 top-N（建议 3~5）进入 provider。
- 对语义接近 query 做 MMR 去冗余，防止重复检索。

4. Provider 执行层（复用）
- 继续用 S2/OpenAlex/Crossref，不改协议。
- 预算与早停逻辑改为基于“高质量 query 命中数量”，非仅数量。

5. 人工反馈层（新增）
- Scholar Debug UI 展示每条 query 的分项得分。
- 支持人工 `+/-` 反馈，写入本地偏好（后续用于权重微调）。

## 集成位置
- Rust：`apps/desktop/src-tauri/src/citation.rs`
- 前端显示：`apps/desktop/src/components/workspace/scholar-panel.tsx`
- Settings：`apps/desktop/src/components/workspace/settings-dialog.tsx`
- Settings schema/runtime：`apps/desktop/src/lib/settings-schema.ts`、`apps/desktop/src-tauri/src/settings.rs`

## 开发里程碑

### M1：Query Score 基线（不接模型）
- 在 Rust 增加 `query_quality_score` 与分项字段（lexical 版）。
- Debug UI 显示 query 分项。
- 验收：可看到每条 query 的 score，排序与执行顺序一致。

### M2：Embedding 评分器接入（轻量）
- 新增 `query_embedding_provider` 抽象：`none | local_embedding`。
- 默认 `none`，开启后使用本地轻量 embedding 计算 `semantic_sim`。
- 验收：与 M1 相比，长段落场景 top-3 query 的人工相关性提升。

### M3：Rerank + MMR + 预算联动
- 按 query score 只执行 top-N；加入 query 多样性控制（MMR）。
- 预算与早停条件读取“高质量 query 命中率”。
- 验收：无关候选显著下降，provider 调用次数可控。

### M4：可配置与回退
- Settings 增加：provider 开关、topN、score 权重、模型超时。
- 任一阶段失败自动回退到规则策略。
- 验收：开关模型不会导致检索不可用。

### M5：离线评测与落参
- 在 `citation:evaluate` 增加 query-level 指标：
- `query_hit@k`、`query_precision@k`、`avg_query_quality`。
- 30+ 真实样本对比 baseline 后再改默认权重。

## 风险与控制
- 风险：模型包体与启动时延增加。
- 控制：模型懒加载 + 首次缓存 + 可关闭。
- 风险：领域迁移不足（化学术语）。
- 控制：保留锚点规则，模型仅作 rerank。

## 完成标准
- 同一批真实样本上：
- top1 命中率较当前提升（目标 +10% 以上）
- “明显不相关候选”占比下降（目标 -30% 以上）
- 平均检索延迟可控（目标 < +20%）
