# Master Plan v2: Agent Harness 能力强化路线图（执行一致版）

> 本文是对 `2026-04-25-master-plan.md` 的执行一致性修订版。目标是降低返工风险，统一依赖关系、接口口径与验收标准。
>
> 单入口执行清单：`2026-04-25-execution-checklist-v2.md`

---

## 1. 目标与范围

**项目：** `claude-prism`  
**主线目标：** 对标高可用 agent harness，补齐取消链路、重试韧性、历史完整性、上下文治理、记忆能力、工具扩展能力与多 agent 委托。  

**v2 范围定义（修订）：**
- 主实现范围：`crates/agent-core` + `crates/agent-cli`
- 兼容范围：`apps/desktop/src-tauri/src/agent/*`（仅 adapter compatibility，不做功能扩写）

**不在本轮范围：**
- MCP/LSP 全量生态
- UI 视觉重构
- 非必要协议升级

---

## 2. 当前基线（以仓库现状为准）

以下能力**已存在**，后续计划不能重复建设：
- `NullEventSink` 已存在并已导出（`event_sink.rs`, `lib.rs`）
- `AgentToolProfile` / `AgentResourcePolicy` 已存在（`config.rs`）
- compact 模块已存在（`compact.rs`）
- provider 重试公共函数已存在（`providers/common.rs`）
- 内存基础设施与 `remember_fact` 工具已存在（`session.rs`, `tools.rs`, desktop `tools/memory.rs`）

因此 v2 的策略是：**增量增强 + 接口对齐**，避免重复发明。

---

## 3. 执行原则（强约束）

1. **Compatibility-first**：任何核心接口重构必须提供兼容层（至少 1 个阶段）。
2. **One source of truth**：命名与语义统一，避免 `memory_write` / `remember_fact` 双轨长期并存。
3. **Fail-fast validation**：每个阶段都必须有明确 DoD，未达标不进入下一阶段。
4. **Small reversible commits**：每个任务可独立回滚。
5. **Desktop 仅兼容，不扩 scope**：desktop 在本轮只做编译/接口适配。

---

## 4. 依赖链（修订后）

```text
P-1 Plan Hygiene（文档与接口口径对齐）
  -> P0 Lifecycle/Retry/Orphan
    -> P1 Context Preflight + Circuit Breaker
      -> P2 Memory Unification (remember_fact-first)
        -> P3 Tool Registry (compatibility-first)
          -> P4 Subagent Coordinator (single-level)
```

### 关键修订说明
- 原版中 `P2` 与 `P3` 互相依赖描述冲突。v2 明确：
  - `P2` 先做 **memory 能力统一与安全收口**，继续基于现有执行路径可落地。
  - `P3` 再做 **registry 抽象**，并提供对旧执行器的兼容桥。

---

## 5. 阶段计划总览（v2）

| 阶段 | 核心结果 | 依赖 | 风险级别 |
|---|---|---|---|
| P-1 | 文档与接口口径统一，修复子计划中的过时 API 假设 | 无 | 低 |
| P0 | cancel 信号贯通 + Retry-After + orphan pairing | P-1 | 中 |
| P1 | pre-send 上下文治理 + 熔断器 | P0 | 中 |
| P2 | memory 能力统一（以 remember_fact 为主语义） | P1 | 中 |
| P3 | ToolRegistry + ToolHandler（兼容旧 ToolExecutorFn） | P2 | 高 |
| P4 | spawn_subagent（单层委托，不可递归） | P3 | 高 |

---

## 6. 每阶段 DoD（Definition of Done）

## P-1: Plan Hygiene（新增）

**目标：** 先把后续执行风险降到可控。

**必须完成：**
- 修订 P0–P4 子计划中的过时类型引用（例如不存在的字段、旧结构体假设）
- 统一 memory 工具命名策略（推荐：保留 `remember_fact`，`memory_write` 作为别名或暂不引入）
- 统一 scope 文案（crate 主实现 + desktop 兼容）

**DoD：**
- [ ] 五个子计划文件均通过“现状对照审查”（无明显过时 API）
- [ ] 依赖链不再自相矛盾
- [ ] 每个子计划都包含“Non-goals”

---

## P0: Lifecycle + Retry + Orphan

**目标：** 解决“无法优雅取消”和“中断后历史污染”两类硬故障。

**交付：**
- `turn_runner`/REPL/TUI 的 cancel_rx 贯通到 provider 流式层
- 429/503 支持 Retry-After 优先策略（无头时退回指数退避）
- pre-send `ensure_tool_result_pairing`，修复孤立 tool_use/tool_result

**DoD：**
- [ ] Ctrl-C 在流式输出中触发 turn cancel（不是杀进程）
- [ ] TUI Escape 能取消当前 turn
- [ ] 人工构造 orphan 历史后，下一轮不再 400
- [ ] `cargo test -p agent-core` 通过
- [ ] `cargo test -p agent-cli` 通过

**回滚点：** 保留 retry 与 pairing 的 feature flag 或独立 commit，可单独回退。

---

## P1: Context Preflight + Circuit Breaker

**目标：** 从“溢出后处理”升级为“发送前治理”。

**交付：**
- 模型窗口映射与阈值函数（或配置化映射）
- provider pre-send compaction loop
- 连续失败熔断（工具或压缩失败）

**DoD：**
- [ ] 长会话不会在首轮 API 发送即 context overflow
- [ ] 触发熔断后返回可解释错误，避免无限循环
- [ ] 压缩行为有可观测日志/状态事件

**回滚点：** 熔断阈值可配置，不需要回退整个模块。

---

## P2: Memory Unification（remember_fact-first）

**目标：** 强化记忆，不引入语义分叉。

**推荐策略：**
- 继续以 `remember_fact` 为主工具名
- 如果必须支持 `memory_write`，只做 schema alias，内部同一路径处理

**交付：**
- key/value 安全约束收口
- 持久化读取在下一会话可注入 prompt
- 增加 list/read/write 辅助 API（仅在确有需求时）

**DoD：**
- [ ] 非法 key（`..`, `/`, `\\`）被拒绝
- [ ] 写入后新会话可看到 memory 注入
- [ ] 不出现双轨行为差异（`remember_fact` 与 `memory_write`）

**回滚点：** alias 可以删，主工具不动。

---

## P3: Tool Registry（compatibility-first）

**目标：** 提升工具扩展能力，避免一次性破坏现有调用链。

**交付：**
- 引入 `ToolHandler` / `ToolRegistry`
- 提供 `ToolExecutorFn` 兼容桥（`into_executor`）
- CLI 先迁移，desktop adapter 最小改动跟进

**DoD：**
- [ ] `ToolRegistry` 可注册/查找/执行工具
- [ ] 旧调用路径在兼容桥下不破坏
- [ ] desktop adapter 编译可通过（仅接口适配）

**风险控制：**
- 先在 crate 内完成 + 测试，再做 desktop 适配
- 禁止在同一 commit 混入无关重构

---

## P4: Subagent Coordinator（single-level）

**目标：** 增加受控子代理委托能力。

**交付：**
- `run_subagent_turn` 协调器
- `spawn_subagent` 工具（CLI）
- 子代理工具集显式剔除 `spawn_subagent`

**DoD：**
- [ ] 子代理可完成单次委托并返回文本结果
- [ ] 子代理无法再 spawn 子代理（硬约束）
- [ ] 父取消信号可中断子代理执行

**回滚点：** 一键关闭 `spawn_subagent` 注册项即可降级。

---

## 7. 统一验证矩阵（修订）

### A. 每阶段必跑
- `cargo test -p agent-core`
- `cargo test -p agent-cli`
- `cargo clippy -p agent-core -- -D warnings`

### B. 阶段里程碑（P3/P4）附加
- `cargo check -p claude-prism-desktop`（adapter compatibility）

### C. 全量回归（收官）
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`

> 说明：若出现与本计划无关的第三方依赖构建问题（如外部库版本漂移），需单独登记为“外部阻塞”，不应模糊为功能回归。

---

## 8. 风险清单（v2）

1. **接口漂移风险（高）**
- 现有子计划里存在过时类型示例，必须先做 P-1。

2. **跨层改动风险（高）**
- P3 涉及 core/cli/desktop，若无兼容桥易引发连锁失败。

3. **语义分叉风险（中）**
- memory 工具命名不统一会导致模型行为不稳定。

4. **可观测性不足风险（中）**
- P0/P1 如无明确状态事件，故障排查成本高。

---

## 9. 建议执行节奏

1. **先做 P-1（0.5 天）**：修完文档和 API 口径再开代码。
2. **P0/P1 分别单独合并**：稳定性先行，优先保护主链可运行。
3. **P2 不追求大而全**：先把统一语义和安全边界做扎实。
4. **P3 分两批提交**：
- 批次 A：core + cli
- 批次 B：desktop compatibility
5. **P4 最后做**：并限制为单级委托，不在本轮扩展递归或并行子代理。

---

## 10. 对原子计划的修订指引（建议）

- `2026-04-25-p0-cancel-retry-orphan.md`
  - 增加 Retry-After 解析优先级与超时上限说明。
- `2026-04-25-p1-context-engineering.md`
  - 将“模型窗口映射”设计成可扩展表，避免硬编码散落。
- `2026-04-25-p2-memory-system.md`
  - 改为 remember_fact-first；`memory_write` 仅 alias。
- `2026-04-25-p3-tool-registry.md`
  - 修正测试示例中的过时字段，严格对齐当前 `AgentToolCall/AgentToolResult`。
- `2026-04-25-p4-multi-agent.md`
  - 删除 NullEventSink 重复建设步骤，聚焦 coordinator + recursion guard。

---

## 11. 结论

v2 的核心是：**先对齐，再开发；先兼容，再替换；先可测，再扩展**。  
按此路线执行，可显著降低 P3/P4 的返工概率，并保证你当前“agent core 主线”持续可交付。
