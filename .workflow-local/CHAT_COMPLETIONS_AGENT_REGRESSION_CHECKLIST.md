# Chat Completions Agent 回归清单

## 目的

在 `CHAT_COMPLETIONS_AGENT_MIGRATION_PLAN.md` 核心迁移完成后，用统一清单验证 chat-completions agent 主链没有回退成“能聊天但不会稳定干活”的状态。

这份清单覆盖三类风险：

1. 结构性编辑风险
2. approval / review 主路径风险
3. session continuity 风险

## 一、自动化回归护栏

### Rust agent 单测

必须通过：

1. `replace_selected_text` 只改目标选区，不误伤周围内容
2. approval-blocked 精细编辑会产出 `reviewArtifact`
3. `apply_text_patch` 只替换目标片段，不退化成整文件覆盖
4. session summary 会带回：
   - `currentObjective`
   - `currentTarget`
   - `lastToolActivity`
   - `pendingState`
   - `pendingTarget`
5. pending turn 可 store/take round-trip

建议命令：

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::tools::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib agent::session::tests
```

### TypeScript / desktop build

必须通过：

```bash
npx pnpm --filter @claude-prism/desktop exec tsc --noEmit
npx pnpm --filter @claude-prism/desktop build
```

## 二、真实交互回归

### 场景 A：选区改稿必须形成精细编辑

步骤：

1. 打开 `main.tex`
2. 选中单段文本
3. 输入：`refine this paragraph`

验收：

1. agent 不应直接给长篇 prose 建议作为主结果
2. 应优先进入：
   - `read_file`
   - `replace_selected_text` 或 `apply_text_patch`
   - `review_ready`
3. diff 中只能修改目标段落
4. 文件其它内容必须完整保留
5. 不允许出现“整篇文件被清空，只剩改后段落”

### 场景 B：approval-blocked 仍应 review-first

步骤：

1. 保持 `write_file` 未授权
2. 重复场景 A

验收：

1. 聊天区显示 pending approval / review ready 语义
2. diff panel 已经出现 proposed change
3. 不再要求用户重新描述任务
4. 聊天区不再误报：
   - `Wrote <file>`
   - `Ask the agent to retry`

### 场景 C：批准后自动继续

步骤：

1. 在 approval 卡片点：
   - `Allow Once`
   - 或 `Allow Session`

验收：

1. 不需要手动 `Retry Now`
2. runtime 自动继续当前挂起 turn
3. 事件语义中会出现：
   - `turn_resumed`
   - `tool_resumed`

### 场景 D：review-first diff 是主舞台

步骤：

1. 对选区执行改稿
2. 观察聊天区和 diff panel

验收：

1. 一旦进入 edit flow，聊天区不再并行保留一整套长篇改稿总结
2. diff panel 是主要审阅面
3. 聊天区只保留：
   - 状态
   - 工具轨迹
   - 审批语义

### 场景 E：session continuity

步骤：

1. 发起一次编辑任务
2. 完成到 review-ready 或 completed
3. 关闭 / 切换 tab
4. 从 session list 恢复

验收：

1. session list 能看到：
   - provider
   - model
   - preview
   - message count
2. 恢复后当前会话条能看到：
   - objective
   - current target
   - last tool activity
   - pending state / target
3. 用户能快速判断：
   - 这轮在做什么
   - 最近做到哪里

## 三、当前主线结论

chat-completions agent 若要被视为“稳定可用”，至少要同时满足：

1. selection-aware edit 不误伤整文件
2. approval-blocked edit 仍能先 review
3. approval 后由 runtime 恢复，不要求用户重述
4. diff 是主编辑面，而不是聊天 prose
5. session 恢复后保留 working identity

如果这五条里任意一条回退，就不应把问题简单解释成“模型弱”，而应先回到 runtime 行为层定位。
