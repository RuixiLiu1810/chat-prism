# Settings Contract v0.1（M1 可编码版）

更新时间：2026-03-28
状态：Draft（可进入 M2 schema 实现）

## 1. 设计目标
- 提供统一设置源，替代分散的 localStorage / store 持久化。
- 明确三层配置：`global`、`project`、`secret`。
- 可迁移（versioned）、可回滚、可导出（非敏感）。

## 2. 配置分层与优先级

### 2.1 层定义
- `default`：内置默认值（代码内常量）。
- `global`：应用级，适用于所有项目。
- `project`：项目级，仅对当前项目生效。
- `secret`：敏感项，不参与普通 JSON 导出。

### 2.2 读取优先级
- `project` > `global` > `default`

### 2.3 写入规则
- `scope=project`：写入 `<project>/.prism/config.json`
- `scope=global`：写入 app config dir 的 `settings.json`
- `scope=secret`：写入 keychain（或受限兜底）

## 3. 存储位置
- Global: `<AppConfigDir>/settings.json`
- Project: `<project>/.prism/config.json`
- Secret: keychain（优先）

注：若 keychain 不可用，使用 `<AppConfigDir>/secrets.json` 兜底，文件权限应限制为当前用户可读写。

## 4. 设置字段清单（首版）

### 4.1 General（global）
- `general.theme`: `"system" | "light" | "dark"`，default=`"system"`
- `general.language`: `"zh-CN" | "en-US"`，default=`"zh-CN"`
- `general.openInEditor.defaultEditor`: `"cursor" | "vscode" | "zed" | "sublime" | "system"`，default=`"system"`

### 4.2 Citation & Scholar（global + project）
- `citation.stylePolicy`（global）: `"auto" | "cite" | "citep" | "autocite"`，default=`"auto"`
- `citation.autoApplyThreshold`（project 可覆盖）: `number`，default=`0.78`，范围 `[0,1]`
- `citation.reviewThreshold`（project 可覆盖）: `number`，default=`0.62`，范围 `[0,1]` 且需 `<= autoApplyThreshold`
- `citation.search.limit`（project 可覆盖）: `number`，default=`8`，范围 `[1,20]`

### 4.3 Integrations（global + secret）
- `integrations.semanticScholar.enabled`（global）: `boolean`，default=`true`
- `integrations.semanticScholar.apiKey`（secret）: `string | null`，default=`null`
- `integrations.zotero.autoSyncOnApply`（global）: `boolean`，default=`true`

### 4.4 Advanced（global）
- `advanced.debugEnabled`: `boolean`，default=`false`
- `advanced.logLevel`: `"info" | "debug" | "warn" | "error"`，default=`"info"`

## 5. 运行时行为契约

### 5.1 立即生效
- `theme`、`citation.stylePolicy`、`debugEnabled` 立即生效。

### 5.2 延迟生效
- 涉及 provider 的配置（如 `semanticScholar.enabled`）在下一次检索生效。

### 5.3 校验失败
- `settings_set` 返回结构化错误，不写入。
- UI 展示字段级错误（path + reason）。

## 6. API 契约（Tauri）

### 6.1 `settings_get`
输入：`{ projectRoot?: string }`
输出：
- `effective`: 合并后可直接用配置
- `global`
- `project`（若 projectRoot 提供）
- `secretsMeta`: 仅返回 secret 是否已设置，不返回明文

### 6.2 `settings_set`
输入：
- `scope`: `"global" | "project" | "secret"`
- `patch`: JSON patch object
- `projectRoot?`: project 写入时必填
输出：`{ ok: boolean, errors?: [{ path, message }] }`

### 6.3 `settings_reset`
输入：`{ scope, keys?: string[], projectRoot? }`
输出：`{ ok: boolean }`

### 6.4 `settings_export`
输入：`{ projectRoot?: string, includeProject?: boolean }`
输出：仅非敏感设置 JSON

### 6.5 `settings_import`
输入：`{ json, mode: "merge" | "replace", projectRoot?: string }`
输出：`{ ok, warnings?: string[], errors?: [...] }`

## 7. 安全与审计要求
- 任何日志不得打印 API key 明文。
- debug 面板只显示 `configured/unconfigured`。
- 导出文件不包含 secret。
- 导入 secret 需明确走独立入口（本期可不支持批量导入 secret）。

## 8. 迁移要求（M2 输入）
- schema 带 `version` 字段，起始 `version=1`。
- 已有分散配置迁移源：
  - theme（next-themes）
  - debug（localStorage debug）
  - citationStylePolicy（citation-store）
  - 未来 S2 key（环境变量可作为一次性导入源）

## 9. 测试验收（M1 关口）
- 字段清单完整且可映射到现有模块。
- 每个字段有：类型、默认值、作用域、校验规则。
- 已定义 API 输入输出与错误模型。
- 已定义 secret 处理边界。

## 10. 非目标（本轮不做）
- 多账户设置隔离。
- 云端设置同步。
- 插件级设置沙箱。

