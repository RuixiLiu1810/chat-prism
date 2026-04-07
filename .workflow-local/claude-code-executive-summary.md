# Claude Code 架构 - 高管总结

## 一句话定义
Claude Code 是一个**流式、自主、可审计的 terminal-native agent**，能在执行过程中实时响应用户中断，并通过多层上下文压缩、智能错误恢复和权限检查来保证安全高效的工具调用。

---

## 核心架构（5 层）

```
┌─────────────────────────────────────────┐
│ 5. UI 层 (REPL.tsx)                    │  React/Ink 组件，50+ 状态，流式事件更新
├─────────────────────────────────────────┤
│ 4. 编排层 (QueryEngine.ts)            │  会话管理、权限拒绝追踪、使用量记录
├─────────────────────────────────────────┤
│ 3. 核心循环 (query.ts - 1732 行)      │  AsyncGenerator，while(true) 对话循环
├─────────────────────────────────────────┤
│ 2. API 层 (claude.ts - 3415 行)       │  流式请求、多 provider、缓存编辑
├─────────────────────────────────────────┤
│ 1. 工具系统 (Tool.ts)                 │  30+ 工具，权限检查，流式执行
└─────────────────────────────────────────┘
```

---

## 四大核心机制

### 1️⃣ AsyncGenerator 流式循环

**为什么不用 Promise?**
```ts
// ❌ Promise - 黑盒，无法中断
await query(...)  

// ✅ AsyncGenerator - yield 让渡控制
for await (const event of query(...)) {
  // UI 实时更新、用户能 Ctrl+C、状态能保存
}
```

**好处**：实时响应、可中断、支持长时间任务

---

### 2️⃣ 四层上下文压缩

| 层次 | 触发条件 | 作用 | 成本 |
|------|--------|------|------|
| **SNIP** | 历史过长 | 删除最早完整对话轮次 | 无额外 API |
| **MICROCOMPACT** | 缓存块需更新 | Prompt Caching 编辑，修改缓存而非重生 | 减少 creation tokens |
| **AUTOCOMPACT** | Token 接近上限 | 调用 Claude 生成摘要 + 结构化记录 | 消耗摘要 token |
| **COLLAPSE** | 超大对话 (>200k) | 深层消息融合，丢弃冗余细节 | 最少 token 损失 |

**关键特性**：独立运作，可混合启用，递进式而非一刀切

---

### 3️⃣ 智能错误恢复决策树

```
API 错误 →
├─ "Prompt too large"
│  ├─ 尝试: CONTEXT_COLLAPSE.drain()
│  ├─ 再尝试: REACTIVE_COMPACT()
│  └─ 最后: 减少输出限制 (ESCALATED_MAX_TOKENS)
│
├─ "max_output_tokens exceeded"
│  ├─ 第 1 次: 提高限制，重试
│  ├─ 第 N 次: 继续尝试（最多 3 次）
│  └─ 失败: 返回部分结果
│
├─ "模型不可用 (429/500)"
│  ├─ 切换 fallback model
│  └─ 重新发起请求
│
└─ 其他: 直接返回

设计：不是"重试"，是"智能降级"
```

---

### 4️⃣ 权限模型（三态分类）

```
权限检查流程：
1. 规则匹配 (alwaysAllow/alwaysDeny)
   ↓ 如无法匹配
2. Auto Mode 分类器 (LLM 判断，可选)
   ↓ 如需用户确认
3. UI 弹窗 (用户选择: Allow/Deny/Details)
   ↓
4. 记录到 audit trail

设计：fail-closed（权限不明确时拒绝）
权限规则存储在：~/.claude/permissions.json
```

**三态**: Allow / Soft Deny / Hard Deny

---

## 关键数据结构

### Message 体系
```ts
type Message = 
  | UserMessage (用户输入)
  | AssistantMessage (模型输出，带 UUID)
  | AttachmentMessage (钩子、摘要、结果)
  | ToolUseSummaryMessage (工具调用摘要)
  | TombstoneMessage (删除标记)
  | ProgressMessage (进度)

特点：完全不可变，所有修改都是追加
```

### State 对象（单次迭代的可变状态）
```ts
type State = {
  messages: Message[]                        // 累积消息
  toolUseContext: ToolUseContext            // 工具执行上下文
  autoCompactTracking: AutoCompactTrackingState
  maxOutputTokensRecoveryCount: number       // 恢复尝试次数
  turnCount: number                         // 轮次计数
  transition: Continue | undefined          // 上次迭代原因
  // ...
}
```

**设计特点**：每次 `continue` 时一次性更新整个 State，而不是分散赋值

---

## Token 管理（四层预算）

```
输入 tokens
  ↓
输出 tokens
  ↓
缓存 tokens (Prompt Caching，1h TTL)
  ↓
任务预算 (TOKEN_BUDGET feature，用户指定)

关键优化：
- 系统 prompt + 工具定义缓存 (全局作用域，跨会话)
- 缓存编辑：工具结果更新时只修改缓存块，不重新生成
```

---

## Feature Flag 系统（89 个特性）

```
编译时优化：
const someModule = feature('FLAG_NAME') 
  ? require('./module')  // 包含在打包中
  : null                 // 编译时剔除

运行时：feature() 始终返回 false（但编译时已剔除）
```

**按实现状态分类**：
- 11 个：已完全实现 (KAIROS, VOICE_MODE, TOKEN_BUDGET等)
- 8 个：部分实现 (PROACTIVE, BASH_CLASSIFIER)
- 15 个：纯 Stub (需要补全)
- 55+ 个：内部基础设施

---

## 工具系统

### Tool 接口
```ts
interface Tool {
  name: string
  description: string
  inputSchema: ToolInputJSONSchema
  call(input, context): AsyncGenerator<ToolResult>
}
```

### 执行模型：StreamingToolExecutor
```
特点：
- 并行执行多个工具（不阻塞模型流式响应）
- 每个工具独立权限检查
- 流式返回结果（不等所有工具完成）
- 动态发现（工具使用摘要在 post 生成）

核心优势：隐藏工具执行延迟，保持 UI 响应性
```

---

## API 多 Provider 支持

```
主力：Anthropic API
      ├─ 标准 claude.anthropic.com
      ├─ AWS Bedrock (ModelId 映射)
      └─ Google VertexAI

可选：Azure OpenAI、企业代理
      └─ HTTP 代理转发

处理：请求参数标准化、错误信息规范化、模型名称映射
```

---

## 对标 Claude Prism 的改进机会

| 维度 | Claude Code 做法 | Claude Prism 现状 | 建议 |
|------|-----------------|----------------|-----|
| 文档处理 | 自动纳入上下文，无页数限制 | 手动上传，16 页截断 | ✅ 移除限制、主动搜索 |
| 权限 | 外部化规则 + LLM 分类 | 代码内检查 | ✅ 建立权限配置文件 |
| 上下文压缩 | 四层递进式 | 未实现 | ✅ 实现压缩链 |
| 缓存 | Prompt Caching | 未利用 | ✅ 集成缓存支持 |
| 多模型 | Sonnet/Opus/Haiku 按场景 | 单一模型 | ✅ 支持模型选择 |
| 错误恢复 | 智能降级链 | 简单重试 | ✅ 实现决策树 |

---

## 优秀 Agent 的核心特征

### ✅ 技术
- 流式异步（AsyncGenerator）
- 多层上下文管理
- 智能错误恢复
- 缓存优化（Prompt Caching）
- 权限透明化

### ✅ 架构
- 特性门控隔离
- 消息完全不可变
- 工具间无耦合
- 多 provider 支持

### ✅ 用户体验
- 进度实时可见
- 成本透明计算
- 决策完全可审计
- 随时可中断

---

## 深层设计哲学

1. **自主性**：Agent 主动探索，而不是被动等待
2. **可审计性**：所有决策有记录、有理由、可追溯
3. **成本意识**：主动压缩和降级，而非无限制消耗

---

## 快速参考：关键文件 + 行数

| 文件 | 行数 | 核心职责 |
|------|------|--------|
| query.ts | 1732 | 核心对话循环 |
| claude.ts | 3415 | API 集成、缓存 |
| Tool.ts | 792 | 工具接口、权限 |
| REPL.tsx | 5009 | UI 组件、事件处理 |
| memdir.ts | 21174 | 长期记忆系统 |
| yoloClassifier.ts | ~500 | 权限分类器 |

---

**生成时间**: 深度学习完成  
**总结置信度**: 85% (核心架构)  
**适合读者**: 技术负责人、Agent 架构师
