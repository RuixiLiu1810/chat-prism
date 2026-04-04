# Settings 深度审计（2026-03-28）

来源：用户审计结论（已复核代码）

## 结论
- 审计结论总体准确，可直接作为 Phase 7 M6.5 的执行输入。
- 需按“依赖顺序”推进，而非仅按严重级别推进，避免返工。

## 问题清单（复核后）

### P0
1. 保存模型混用（即时保存 + 手动保存并存），用户心智不稳定。
2. Secret 仍为本地文件明文存储，未实现 Keychain 优先。

### P1
3. LLM Query 参数归属错位（应与 provider 同层）。
4. 全局重置缺二次确认，误触风险高。
5. API Key 交互弱（mask/reveal、状态提示、重复按钮逻辑）。
6. 前后端 sanitize/default 逻辑重复，维护成本高。

### P2
7. Global/Project/Secret 作用域不可见。
8. Import/Export 偏工程化，缺文件导入导出 UX。
9. 中英文文案混用。
10. 缺字段说明（范围、语义、约束关系）。

### P3
11. Unsaved changes 警示缺失（源于混合保存模型）。
12. logLevel 未实质接入日志过滤链路。
13. 缺 provider 连通性测试入口。
14. patch 后全量 reload，存在额外 I/O。

## 执行顺序（按依赖）
1. `S1` 统一保存模型（先定规则，再改 UI）
2. `S2` 信息架构重排（Citation/Providers/Advanced 归位）
3. `S3` 作用域可视化（含 project override）
4. `S4` 危险操作确认（reset 全局）
5. `S5` API Key 体验改造
6. `S6` Secret Keychain 化（macOS 先行 + fallback）
7. `S7` logLevel 接入 + provider test
8. `S8` Import/Export 文件化 + 文案/帮助文本统一
9. `S9` store I/O 优化（减少 get-after-set）
10. `S10` sanitize 单一真源收口（Rust authoritative）

## 本轮建议起步（最小风险）
- 先做 `S1 + S2 + S4`：不改存储协议，风险低，收益立刻可见。
- 再做 `S6`：涉及平台能力与迁移，需要单独验收。
