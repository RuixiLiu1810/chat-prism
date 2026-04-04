# 本地流程工作区（不走 Git 同步）

这个目录用于记录开发流程与任务推进，避免上下文切换时丢失注意力。

- 作用范围：仅本机
- 同步策略：已加入 `.git/info/exclude`，不会进入 Git 跟踪
- 使用方式：每次开始开发前先看 `PHASE_STATUS.md` 与 `NEXT_STEPS.md`

建议更新顺序：
1. `SESSION_LOG.md` 追加本次目标
2. `TASK_BOARD.md` 移动任务状态
3. `PHASE_STATUS.md` 更新阶段完成度
4. `NEXT_STEPS.md` 写下下一次接续动作

口径约束（避免漂移）：
- `PHASE_STATUS.md`：只写“阶段级”状态，不写细碎任务。
- `TASK_BOARD.md`：只保留“任务级”状态，`In Progress` 不放已完成项。
- `NEXT_STEPS.md`：仅保留下一次实际可执行的 3-5 项动作。
