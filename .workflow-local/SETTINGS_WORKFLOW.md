# 应用设置系统完整流程（整合版）

更新时间：2026-03-28

## 目标
- 建立统一“应用设置中心”，替代分散配置。
- 明确配置分层：全局、项目、敏感信息。
- 提供可迁移、可测试、可扩展的设置架构。

## 范围定义
- 本期纳入：
  - 主题与通用偏好（General）
  - 引用与检索偏好（Citation & Scholar）
  - 集成配置（Integrations：S2/Zotero 等）
  - 高级项（日志级别、调试开关）
- 本期不纳入：
  - 复杂账号系统
  - 云端同步
  - 多用户权限控制

## 配置分层规范
- `global`：跨项目生效，存于应用配置目录。
- `project`：按项目目录生效，存于 `<project>/.prism/config.json`。
- `secret`：敏感字段（API key/token），优先系统 keychain。

## Phase 7 里程碑

### M1：需求冻结（1天）
- 输出字段清单、默认值、分层归属。
- 输出冲突优先级：
  - project > global > default
- 输出禁忌项：
  - secret 不进导出文件
  - secret 不写日志

验收标准：
- `settings_contract.md` 确认完毕，可直接编码。

### M2：数据模型与迁移（1-2天）
- 建立 settings schema（建议 zod + version）。
- 设计 migration：
  - v1 -> v2 可回放
  - 无损迁移旧配置

验收标准：
- 启动时自动迁移成功，坏配置可回退默认并给出提示。

### M3：后端能力（2天）
- 增加 tauri 命令：
  - `settings_get`
  - `settings_set`
  - `settings_reset`
  - `settings_export`
  - `settings_import`
- 增加 secret 读写桥接：
  - keychain 可用则写 keychain
  - 否则走受限文件兜底（并标记风险）

验收标准：
- 命令可独立调用，字段级更新生效，错误可追踪。

### M4：前端设置中心（2天）
- 新增 `settings-store`，统一状态与持久化协议。
- 新增设置入口与页面骨架：
  - General
  - Citation & Scholar
  - Integrations
  - Advanced
- 支持“恢复默认”“导入导出”。

验收标准：
- UI 修改后即时生效，重启后不丢失。

### M5：配置迁移接入（1-2天）
- 把现有分散配置接入统一中心：
  - theme
  - debug flag
  - citation style policy
  - S2 API key
  - 未来可扩展 provider 参数

验收标准：
- 旧入口仍可用，但底层统一走 settings-store。

### M6：测试与发布（2天）
- 单测：
  - schema 校验
  - migration
  - 默认值与分层覆盖
- 集成测试：
  - tauri 命令链路
  - 导入导出
  - secret 脱敏
- 回归测试：
  - Zotero 登录/同步
  - citation search 与 debug

验收标准：
- 无阻断回归，关键链路通过。

### M6.5：信息架构与交互收口（1-2天）
- 目标：解决“设置项能配但难用”的问题，降低入口重复和认知成本。
- 任务：
  - 引入作用域可见化（Global / Project），明确当前修改写入层级。
  - 重排分组：`Citation` 保留策略与阈值，`Providers` 专注数据源与 API key，`Advanced` 仅开发者项。
  - 统一保存模型：避免同页“部分即时保存 + 部分按钮保存”混用。
  - 收敛重复入口：定义 Sidebar 快捷入口与 Settings 主入口的职责边界。
  - 增加 provider 连通性测试与结果提示（非阻塞）。

验收标准：
- 任一设置项都可回答三件事：改了什么、写到哪里、何时生效。
- 用户不需要在多个入口猜测“哪个才是主设置入口”。
- Tab 切换与保存行为一致，无感知跳变。

## 建议字段（首版）
- `general.theme`: `system|light|dark`
- `general.language`: `zh-CN|en-US`（预留）
- `citation.stylePolicy`: `auto|cite|citep|autocite`
- `citation.autoApplyThreshold`: number
- `citation.reviewThreshold`: number
- `integrations.semanticScholar.apiKey`: secret
- `integrations.semanticScholar.enabled`: boolean
- `integrations.zotero.autoSyncOnApply`: boolean
- `advanced.debugEnabled`: boolean
- `advanced.logLevel`: `info|debug|warn|error`

## 风险与应对
- 风险：设置入口与旧入口行为不一致。
- 应对：先做“单一写路径”，旧入口调用统一 API。
- 风险：secret 在日志或导出泄露。
- 应对：统一脱敏层，导出白名单。
- 风险：项目级配置污染全局。
- 应对：所有读取走 `resolve(project, global, default)`。

## 执行顺序（与现有 workflow 整合）
1. 先完成 Phase 7 M1-M2（冻结需求 + schema）。
2. 并行推进 Scholar Phase 2 的 Crossref/熔断补强。
3. Phase 7 M3-M4 落地后，再做 M5 迁移接入。
4. 与 Phase 6 测试合并收口发布。

## 当前落地状态（2026-03-28）
- 已完成：M1 / M2 / M3 / M4
- 进行中：M5（旧入口迁移 + 设置页组件化统一）
- 进行中：M6.5（已完成 S1/S2/S3/S4/S5/S7；与 S6 协同收口）
- 未开始：M6（系统化测试与发布收口）

## 本轮落地清单
- [x] 建立 `settings-store.ts`
- [x] 建立 `settings-api.ts`
- [x] 建立 tauri `settings.rs`
- [x] 连接 Sidebar 设置入口
- [x] 完成 M1 字段清单评审
- [ ] M5：迁移剩余旧入口到统一 settings 写路径
- [ ] M5：将设置页剩余原生控件替换为 `components/ui`
- [x] M6.5：作用域可见化（Global/Project）
- [ ] M6.5：入口去重与职责边界（Theme/Citation/Providers）
- [x] M6.5：统一保存模型
- [x] M6.5：Citation/Providers 分组收口（LLM 配置归位）
- [x] M6.5：全局重置风险控制（Danger Zone + 二次确认）
- [x] M6.5：API Key 体验改造（mask/reveal + 单按钮保存）
- [x] M6.5：Provider 连通性测试入口（S2/OpenAlex/Crossref/LLM endpoint）
- [x] S6：Secret 存储升级（macOS Keychain 优先 + 文件兜底）
- [x] M6.5：入口职责收口（Selection Citation 样式入口改为只读提示）
- [x] S10：Rust 单一真源收口（前端运行时不再依赖 settings sanitize/resolve）
