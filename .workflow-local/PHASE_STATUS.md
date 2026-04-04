# 阶段状态总览

更新时间：2026-03-30

## 当前项目阶段
- [x] Phase 1：骨架与接口（基础命令与 store 已落地）
- [x] Phase 2：检索与标准化（S2/OpenAlex/Crossref + timeout/retry/circuit）
- [x] Phase 2.5：LLM 检索词增强（M1-M3 已完成：query plan/LLM rewrite/执行预算与融合加权）
- [x] Phase 3：相关性算法（真实样本阈值校准已完成，默认阈值已写回）
- [x] Phase 3.5：Query Scoring（M1-M5 完成，含 hardcase 修复验证与扩样评测）
- [x] Phase 4：落稿与 Zotero（Bib 幂等、Zotero 幂等、句位插入策略）
- [x] Phase 5：UI 集成（独立 Scholar 面板 + Zotero 面板解耦完成）
- [x] Phase 6：测试与发布（单测+集成+E2E-lite+GUI quick checklist+full release check 已完成）
- [~] Phase 7：应用设置系统（M1-M5 第一阶段完成；新增 M6 信息架构与入口去重收口）
- [~] Phase 8：论证结构引文流程（workflow v3 收敛执行中，S0/S1/S1.5/S2 已落地，S3 可选评估中）

## 当前主线
- 主线D（Citation Flow V3）：`CITATION_ARGUMENT_WORKFLOW.md` 已升级 v3；本轮已完成 S0/S1/S1.5/S2（含软门控与基线采集），当前进入 S3 可选评估阶段（S4 继续 Parked）。
- 主线A（Scholar）：本轮收口完成（30 条扩样校准、默认阈值落参、hardcase 单测通过、release checks 通过）。
- 主线A（Scholar）补充：当前看板未完成项已清零，后续仅在用户提出新需求时继续扩展。
- 主线A（Scholar）补充：下方为历史推进轨迹，保留用于回溯，不再代表待办状态。
- 主线A（Scholar）补充：Sidebar 历史上曾采用 `Scholar/Zotero` 双面板；当前已收敛为“选区悬浮窗发起检索 + Sidebar 仅保留 Zotero”。
- 主线A（Scholar）补充：已按交互改造将检索入口前移到编辑器选区悬浮窗（新增 `Search Citation`）；Sidebar 底部当前仅保留 Zotero。
- 主线A（Scholar）补充：已补齐悬浮窗内联反馈状态（searching/results/applied/error + retry/cite/close），解决“触发检索后无可见反馈”问题。
- 主线A（Scholar）补充：已在选区悬浮窗恢复 Debug 入口（`Debug Citation`），并提供 `Copy Raw / Copy Eval / Copy Labeled Top1 / Copy No Match`。
- 主线A（Scholar）补充：Debug 对话框已补齐“评估标注区”（Select/Top1/NoMatch/Clear + Labeled 导出），可直接用于样本标注回灌。
- 主线A（Scholar）补充：已完成 provider 解析容错与 OpenAlex/Crossref compact query，长段落检索稳定性提升，待 GUI 真实样本复测。
- 主线A（Scholar）补充：样本闭环脚本链路（merge/check/evaluate）已再验证通过，当前阻塞点主要在样本规模与难例覆盖，而非标注完整度。
- 主线A（Scholar）补充：阈值落参脚本已就绪（`citation:sync-thresholds`），待真实样本报告后执行 `--write`。
- 主线A（Scholar）补充：新增 `citation:label-template`，可从 `merged_results` 自动生成 DOI/Title 候选模板，降低人工标注成本。
- 主线A（Scholar）补充：`citation:calibrate` 已内置 `label-template` 生成，样本闭环可单命令执行。
- 主线A（Scholar）补充：新增 `citation:apply-labels`，可将审核后的模板幂等回写到数据集，并由 `citation:calibrate --labels` 直接接入评测链路。
- 主线A（Scholar）补充：新增 `citation:auto-label` 弱监督路径，可在不手工标注情况下按置信门控自动回填部分样本。
- 主线A（Scholar）补充：Debug 弹窗已支持可视化点选标注并导出 labeled sample，显著降低人工 JSON 编辑成本。
- 主线A（Scholar）补充：检索词链路已改为“锚点优先（材料/方法/形貌）+ 工艺噪声抑制”，并收紧早停条件，避免首轮低质结果占满候选池。
- 主线A（Scholar）补充：新增 `CITATION_QUERY_SCORING_WORKFLOW.md`，将“query 可打分 + embedding 替代原生 BERT”作为独立执行轨道（M1-M5）。
- 主线A（Scholar）补充：Phase 3.5 / M4 已完成（执行参数已下沉到 Settings 并接入运行时），下一步进入 M5（样本评测与默认值落参）。
- 主线A（Scholar）补充：M5 v3 样本状态已更新为 `labeled=6 / unlabeled=0`（positive=5, no_match=1），评测为 `top1=0.8, top3/top5=1, mrr=0.9`，结论仍是“可用但不稳”，需扩样后再落默认值。
- 主线A（Scholar）补充：M5 v4 已并入新 hard case（`sample_2026-03-30T03-29-42-140Z`），当前 `labeled=7 / unlabeled=0`，评测变为 `top1=0.6667, top3/top5=1, mrr=0.8333`，确认存在“泛化词命中压过领域相关文献”的排序缺陷。
- 主线A（Scholar）补充：已实现短句 `formula-signal penalty`（基于化学式元素一致性），目标是抑制“hydrothermal/nanotubes 词命中但材料体系不一致”的 top1 误排；待用同一 hard case 做复测确认收益。
- 主线A（Scholar）补充：hard case 探针复测已验证收益（`top_expected_rank: 2 -> 1`，文件 `/private/tmp/citation_hardcase_formula_penalty_probe.json`）；离线评估总表暂未变化是因为数据集中分数为历史快照，需采集新一轮 debug 样本再刷新全局指标。
- 主线A（Scholar）补充：`formula-signal penalty` 已做第二段增强（短句 + 化学式密集 claim 时进一步提高材料不一致惩罚）；当前等待新一轮 debug 样本确认真实排序收益。
- 主线A（Scholar）补充：已并入新真实样本 `sample_2026-03-29T17-03-22-514Z` 并形成 v5 数据集（8 条全标注）；评测较 v4 提升：`top1 0.6667 -> 0.7143`、`mrr 0.8333 -> 0.8571`，`top3/top5` 维持 1.0。
- 主线A（Scholar）补充：Debug 标注流程新增 `Append Local Dataset`，可直接把标注样本落到项目 `.workflow-local/citation_eval_samples.jsonl`，扩样效率明显提升。
- 主线A（Scholar）补充：Debug 面板已补齐 early-stop 解释（原因/阶段/hit ratio 与命中计数），便于定位“为什么提前停止”。
- 主线A（Scholar）补充：已支持 `expected.no_match=true` 负样本标注（UI 导出 + 脚本链路）；本轮已补齐最后 1 条遗留样本，unlabeled 清零。
- 主线B（Settings）：M6.5 关键项已落地（统一保存模型 + Providers 归位 + 作用域可视化 + 重置确认 + 连通性入口 + Keychain 优先 + 入口职责收口 + S10）。
- 主线B（Settings）补充：Citation 页已完成“简化模式”收口（`Fast/Balanced/Deep` + `LLM 开关` 默认可见，query 细参数折叠到高级区）。
- 主线C（测试）：`citation-store` 单测已补齐（门控/提示/bib 幂等），下一步推进集成与 E2E。
- 主线C（测试）补充：`citation-store` 集成流程测试已落地，当前聚焦 E2E 闭环与 GUI 回归。
- 主线C（测试）补充：测试基建已收口（storage/event mock），桌面端 `vitest` 全量 170/170 通过。
- 主线C（测试）补充：已提供发布前自动检查脚本（`qa:release`）与 GUI E2E 清单，进入实机验收阶段。
- 主线C（测试）补充：GUI quick checklist 已人工勾选 PASS，`qa:release:full` 现已通过（含 Rust）。
- 主线C（测试）补充：发布检查脚本已支持 `pnpm` 缺失时自动回退本地 `node_modules/.bin/{tsc,vitest}`；`qa:release` 与 `qa:release:full` 已复跑验证通过（Rust 失败仍按 optional 记录）。
- 主线C（测试）补充：`citation:calibrate` 也已支持 `runner=local-tsx` 回退；无 `pnpm` 环境下可完整跑通 check/evaluate/sync。
- 主线C（测试）补充：`release-check-desktop.ts` 新增 macOS Homebrew Rust env 自动注入，默认可打通 `cargo check`（PKG_CONFIG_PATH + harfbuzz include + C++17）。

## 关键风险
- Rust 本地依赖（tectonic 链）在当前机器已通过环境注入方案收口；跨机器仍可能因 Homebrew 路径差异出现回归。
- 若 LLM 生成 query 无 schema 约束，可能引入不稳定和不可复现问题。
- 设置项若不先划分“全局/项目/敏感”，后续会出现高返工成本。
- 设置入口重复（Sidebar 快捷入口 vs Settings 主入口）若不收敛，会造成用户心智冲突与行为不一致。
