# Agent CLI 配置初始化引导设计（方案 2）

日期：2026-04-22  
状态：Draft -> Proposed

## 1. 背景与目标

当前 `agent-runtime` 强依赖命令行参数（`--api-key` / `--provider` / `--model`），首次体验和日常修改成本高，无法达到类似 Claude Code 的自然上手路径。

本设计目标：

1. 提供首次运行“全屏向导”完成配置输入与保存。
2. 提供后续配置修改双入口：
- 命令入口：`agent-runtime config edit`
- REPL 入口：`/config`
3. 固化配置优先级：`CLI 参数 > 环境变量 > 本地配置文件 > 交互式输入`。
4. 保持 MVP-1 既有运行体验（REPL、多轮、human/jsonl 输出）不回退。

## 2. 范围与非目标

### 2.1 In Scope

1. 新增 `agent-cli` 统一配置子系统（加载、校验、保存、掩码输出、交互更新）。
2. 首次缺配置时自动进入全屏向导。
3. 增加 `config` 子命令族（最少 `init` / `edit` / `show` / `path`）。
4. REPL 增加 `/config` 命令，进入交互式修改菜单。
5. 本地配置文件持久化（不引入系统密钥链）。

### 2.2 Out of Scope

1. 多 profile / 团队配置同步。
2. 远程配置中心。
3. keychain/credential manager 集成。
4. agent-core 大规模改造（本轮只在 `agent-cli` 做体验层改进）。

## 3. 方案选型

### 方案 A：在 `main.rs` 直接拼接引导逻辑

- 优点：改动小。
- 缺点：读写校验与交互逻辑分散，后续维护成本高。

### 方案 B（采用）：统一配置子系统 + 双入口

- 优点：
  - 启动流程、`config edit`、REPL `/config` 全部共享同一套逻辑；
  - 错误语义、校验规则、优先级解析保持一致；
  - 后续扩展（profile/doctor）可增量演进。
- 缺点：首版模块会比 A 多。

### 方案 C：一次性做完整“配置中心”

- 优点：长期能力强。
- 缺点：超出当前阶段目标，交付风险高。

结论：采用方案 B。

## 4. 架构设计

## 4.1 新增模块

建议在 `crates/agent-cli/src/` 新增：

1. `config_store.rs`
- 本地配置文件路径解析、读写、原子落盘。
- 处理损坏配置文件的恢复策略（备份+重建）。

2. `config_model.rs`
- `StoredConfig`（文件结构）
- `ResolvedConfig`（最终运行态）
- 字段校验与 provider 默认值映射。

3. `config_resolver.rs`
- 实现优先级归并：`CLI > ENV > FILE > INTERACTIVE`。
- 输出 `ResolvedConfig` 与“缺失字段集合”。

4. `config_wizard.rs`
- 首次全屏向导与编辑向导。
- 交互步骤（provider -> model -> api key -> base_url -> output -> confirm）。

5. `config_commands.rs`
- `config init/edit/show/path` 子命令执行器。

6. `repl_commands.rs`
- 解析 REPL slash 命令（本轮最小支持 `/config` 和 `/help`）。

## 4.2 既有模块改造

1. `args.rs`
- 从“全必填参数”改为“可选参数 + 子命令”；
- 保留兼容：用户仍可传 `--api-key/--provider/--model`，优先级最高。

2. `main.rs`
- 启动时走 `config_resolver` 获取最终配置；
- 缺失关键字段时触发 `config_wizard`；
- 在 REPL 模式下对 `/config` 分流到配置编辑入口。

3. `turn_runner.rs`
- 使用 `ResolvedConfig` 替代散落参数拼装。

## 5. 命令面设计（Command Surface）

## 5.1 运行入口

1. 常规运行（若配置完整，直接进 REPL）
```bash
agent-runtime
```

2. 单轮运行
```bash
agent-runtime --prompt "Say hello"
```

3. 覆盖配置（临时）
```bash
agent-runtime --provider deepseek --model deepseek-chat
```

## 5.2 配置子命令

1. 初始化（总是运行全屏向导）
```bash
agent-runtime config init
```

2. 编辑（读取当前配置后进入向导，可回显默认值）
```bash
agent-runtime config edit
```

3. 展示当前有效配置（默认掩码 api_key）
```bash
agent-runtime config show
```

4. 显示配置文件路径
```bash
agent-runtime config path
```

## 5.3 REPL 内命令

1. `/config`
- 打开“配置修改菜单”，保存后继续当前 REPL。

2. `/help`
- 展示 REPL 内命令摘要（至少包含 `/config`, `exit`, `quit`）。

## 6. 配置模型设计

## 6.1 本地文件结构（建议 JSON）

`StoredConfig`（示例）：

```json
{
  "provider": "minimax",
  "model": "MiniMax-M1",
  "api_key": "<SECRET>",
  "base_url": "https://api.minimax.chat/v1",
  "output": "human"
}
```

## 6.2 必填与可推导字段

1. 必填：`provider`, `model`, `api_key`
2. 可推导：`base_url`（由 provider 默认值映射）
3. 可选：`output`（默认 `human`）

## 6.3 provider 默认映射（当前阶段）

1. `minimax` -> `https://api.minimax.chat/v1`
2. `deepseek` -> `https://api.deepseek.com/v1`

## 7. 数据流设计

## 7.1 启动流

1. 解析 CLI 参数（可为空）。
2. 读取环境变量。
3. 尝试读取本地配置文件。
4. 执行优先级归并，得到 `ResolvedConfig`。
5. 若关键字段缺失：进入全屏向导并保存配置。
6. 再次归并并校验。
7. 进入单轮或 REPL 执行。

## 7.2 编辑流（`config edit` / `/config`）

1. 加载当前有效配置（含 defaults）。
2. 向导逐步回显与编辑。
3. 用户确认后原子写入。
4. 输出保存结果与生效字段。
5. 若在 REPL 内触发：返回 REPL 并继续会话。

## 8. 交互引导设计（全屏向导）

## 8.1 首次引导文案骨架

1. 欢迎页：说明将配置 provider/model/api_key。
2. provider 选择页：
- `1) minimax`
- `2) deepseek`
3. model 输入页（带 provider 对应推荐默认值）。
4. api_key 输入页（回显掩码，不明文打印到日志）。
5. base_url 页（默认值 + 可覆盖）。
6. output 页（`human`/`jsonl`）。
7. 确认页（掩码展示）-> `save / back / cancel`。

## 8.2 修改引导

`config edit` 与 `/config` 使用同一套步骤；默认值取当前配置。

## 9. 错误处理设计

1. 配置文件不存在
- 触发首次向导，不报错。

2. 配置文件损坏（JSON 解析失败）
- 输出警告；
- 将原文件备份为 `config.json.bak.<timestamp>`；
- 进入向导重建。

3. 配置文件不可写
- 直接报错并退出（提供路径和权限建议）。

4. provider 非法
- 阻止保存并提示可选值。

5. model 为空
- 阻止保存并提示输入。

6. api_key 为空
- 阻止保存并提示输入。

7. REPL 中 `/config` 修改失败
- 输出错误但不退出 REPL。

## 10. 安全与隐私约束

1. 本轮按用户要求：写入本地配置文件（明文）。
2. 输出时默认掩码 api_key（仅展示前 3 + 后 2）。
3. 禁止在错误日志/事件流中打印明文 api_key。

## 11. 测试面设计

## 11.1 单元测试

1. `config_resolver`
- 覆盖优先级矩阵：CLI/ENV/FILE/INTERACTIVE。

2. `config_model`
- provider/base_url 默认映射与字段校验。

3. `config_store`
- 文件不存在、正常读写、损坏文件备份路径生成。

4. `config_wizard`
- 输入序列驱动测试（有效输入、取消、回退）。

5. `repl_commands`
- `/config`、`/help`、普通文本分类。

## 11.2 集成测试

1. 首次运行无配置 -> 自动进入向导 -> 保存后可运行。
2. `agent-runtime config edit` 修改后下一次运行生效。
3. CLI 参数覆盖配置文件值（优先级正确）。
4. `config show` 不泄露明文 api_key。
5. REPL `/config` 成功后不中断后续对话。

## 12. 兼容性与迁移策略

1. 保留现有 flags（向后兼容）。
2. 已设置环境变量的用户无需强制迁移。
3. 新用户无需先读文档即可完成初始化。

## 13. 成功标准（Definition of Done）

1. 首次启动在缺配置场景下可通过全屏向导完成初始化并成功运行。
2. `config init/edit/show/path` 可用且行为一致。
3. REPL `/config` 可修改并保存配置，返回 REPL 后继续使用。
4. 优先级 `CLI > ENV > FILE > INTERACTIVE` 被自动化测试覆盖并通过。
5. `cargo test -p agent-cli` 与 `cargo clippy -p agent-cli -- -D warnings` 通过。
