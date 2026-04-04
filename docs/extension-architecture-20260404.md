# tiy-desktop 扩展架构设计文档 v1.1

> **v1.1 变更说明**：基于两轮 review 收敛，主要补充了：运行时接口定义（Plugin 结构化 command+args 执行协议、Hook 执行协议与硬约束）、权限模型与 PolicyEngine 对接方式、MCP config_error/stale snapshot/凭证脱敏、Skills tools≠授权/pin 机制、统一状态模型语义差异说明、Extensions Center 与 Marketplace Center IA 迁移策略、Implementation Alignment 小节（ToolGateway/PolicyEngine/AuditRepo 改造接缝）、UnifiedToolResult 包裹型定位、command-provider 定义、硬约束清单（Guardrails）、Phase 1 拆分为 1A+1B。

## 1. 总述

`tiy-desktop` 的扩展系统采用 **三类对象、统一宿主、配置驱动、宿主强治理** 的架构：

1. **Plugins**
   - 本地扩展包
   - 通过 manifest 描述
   - 支持 hook、tool-provider、command-provider、skill-pack
   - 一期允许执行外部命令，但必须经过宿主控制

2. **MCP**
   - 在产品层是独立的一类扩展对象
   - 在实现层由 Rust 侧 `McpHost` 管理连接、discover、状态、降级与执行

3. **Skills**
   - 作为 Agent 的能力资产层
   - 一期纳入统一 `Extensions Center`
   - 来源包括 builtin / workspace / plugin

### 已确认决策
以下三项已固定：

1. **扩展来源**
   - 一期支持：**本地安装 + Marketplace 清单**
   - 不支持远程代码包自动下载执行

2. **MCP 产品定位**
   - MCP 在扩展中心中 **独立成类**
   - 不作为 plugin capability 隐藏

3. **Plugin 外部命令执行**
   - 一期 **允许**
   - 但所有执行必须走 `ToolGateway + PolicyEngine + Approval + Audit`

4. **Skills 产品组织**
   - 一期 **统一纳入 `Extensions Center`**
   - 不单独拆独立入口

5. **扩展配置持久化**
   - 一期 **先使用现有 settings/policy KV**
   - 后续成熟后再考虑独立表结构

---

## 2. 设计目标与范围

### 2.1 目标
本架构目标是为 `tiy-desktop` 提供一套：

- 可安装
- 可发现
- 可配置
- 可启停
- 可审计
- 可失败降级
- 可面向用户展示

的桌面 Agent 扩展机制。

### 2.2 一期范围
一期包含：

1. Plugin Registry
2. MCP Registry
3. Skill Host
4. Extensions Center UI
5. Marketplace Listing 集成
6. 扩展工具接入现有 `ToolGateway`
7. MCP 以独立页签方式接入扩展中心
8. Skills 作为扩展能力资产纳入扩展中心

### 2.3 一期不包含
1. 远程插件代码自动下载并执行
2. 通用脚本沙箱
3. 动态前端组件注入
4. 多设备同步扩展
5. 完整权限虚拟化隔离层

---

## 3. 总体架构

系统分为四层。

### 3.1 Catalog Layer
负责“知道有哪些扩展”：

- 本地目录扫描
- Marketplace 清单加载
- 元数据解析
- 安装来源识别

### 3.2 Host Layer
负责“扩展如何被治理”：

- 注册
- 校验
- 启停
- 生命周期
- 权限绑定
- 错误状态
- 运行态注册表

### 3.3 Runtime Integration Layer
负责“扩展如何进入现有系统”：

- ToolGateway
- PolicyEngine
- AuditRepo
- Agent Runtime
- Settings / Thread / Terminal

### 3.4 UI & Product Layer
负责“用户如何感知与操作扩展”：

- Extensions Center
- Marketplace 页面
- 状态卡片
- 权限展示
- 日志与错误可见性

---

## 4. 三类扩展对象模型

> **重要约束**：三类对象（plugin / mcp / skill）在 UI 上统一归入 "Extensions"，但在生命周期、安装语义、健康状态和治理方式上并不完全同构。"统一展示"不等于"统一底层状态机"——各类型保持独立的内部模型，仅在产品展示层做轻量聚合。

## 4.1 统一产品抽象
前端产品层和 IPC 返回统一使用扩展摘要对象：

```ts
export type ExtensionKind = "plugin" | "mcp" | "skill";

export type ExtensionSource =
  | { type: "builtin" }
  | { type: "local-dir"; path: string }
  | { type: "marketplace"; listingId: string };

export type ExtensionInstallState =
  | "discovered"
  | "installed"
  | "enabled"
  | "disabled"
  | "error";

export type ExtensionHealth =
  | "unknown"
  | "healthy"
  | "degraded"
  | "error";

export type ExtensionSummary = {
  id: string;
  kind: ExtensionKind;
  name: string;
  version: string;
  description?: string;
  source: ExtensionSource;
  installState: ExtensionInstallState;
  health: ExtensionHealth;
  permissions: string[];
  tags: string[];
};
```

说明：

- 这是产品展示层统一模型，底层 plugin/mcp/skills 各自仍保持独立实现
- `installState` 是产品展示层状态，由各子系统内部状态映射而来（见 5.4 映射表）
- `health` 是**派生字段**，由各子系统运行态状态推导，不独立维护：
  - Plugin：`Enabled` → `healthy`，`Error` → `error`，`Disabled` → `unknown`
  - MCP：`connected` → `healthy`，`degraded` → `degraded`，`error`/`config_error` → `error`
  - Skill：解析成功 → `healthy`，解析失败 → `error`
- UI 可以用统一列表页、筛选器和状态组件

### `installState` 各类型语义差异

| installState | Plugin | MCP | Skill |
|---|---|---|---|
| `discovered` | 目录被发现，尚未登记 | — | — |
| `installed` | manifest 解析通过，已登记 | 配置已存在于 settings | 文件已索引 |
| `enabled` | hook/tool/command 注册就绪 | server 已启动且连接正常 | 已纳入 Select 候选池 |
| `disabled` | 已安装但停用 | 配置存在但未启动 | 已索引但不参与 Select |
| `error` | 校验/注册/运行时失败 | 连接/discover/运行时失败 | 文件解析失败 |

各类型详情页应展示专属字段（如 MCP 的 phase/tools/resources、Plugin 的 hooks/commands、Skill 的 triggers/budget），而非仅靠统一模型。

---

## 5. Plugin 架构

## 5.1 Plugin 定义
Plugin 是本地扩展包单元，支持以下能力类型（manifest `capabilities` 字段）：

- `hook` — 注册生命周期钩子
- `tool-provider` — 提供可被 Agent 调用的工具
- `command-provider` — 提供 prompt template 级命令（见 5.10）
- `skill-pack` — 提供 skill 文件目录

一期不支持：
- `ui-metadata`（前端组件注入，留待后续）
- 远程代码包执行
- 任意前端脚本注入
- 热加载沙箱

## 5.2 Plugin 目录结构
建议标准目录：

```text
<plugin-root>/
  plugin.json
  hooks/
  tools/
  skills/
  assets/
```

## 5.3 Plugin Manifest
```ts
export type PluginManifest = {
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  homepage?: string;
  defaultEnabled?: boolean;

  capabilities: Array<
    | "hook"
    | "tool-provider"
    | "command-provider"
    | "skill-pack"
    // "ui-metadata" 预留，一期不支持
  >;

  permissions: Array<
    | "workspace-read"
    | "workspace-write"
    | "shell-exec"
    | "network-access"
    | "terminal-control"
  >;

  hooks?: {
    preToolUse?: string[];
    postToolUse?: string[];
    onRunStart?: string[];
    onRunComplete?: string[];
  };

  tools?: Array<{
    name: string;
    description: string;
    command: string;
    args?: string[];
    env?: Record<string, string>;
    cwd?: string;
    timeoutMs?: number;            // 覆盖 plugin 级默认值
    requiredPermission: "read" | "write" | "exec";
  }>;

  commands?: Array<{
    name: string;
    description: string;
    promptTemplate?: string;       // 一期为 prompt template 级命令，见 5.10
  }>;

  timeoutMs?: number;              // plugin 级默认超时，tools/hooks 可各自覆盖

  skillsDir?: string;

  configSchema?: {
    type: "json-schema";
    path: string;
  };
};
```

## 5.4 Plugin 生命周期
状态机：

1. `Discovered`：目录被发现，但尚未纳管
2. `Installed`：已登记到系统配置中（含 manifest 解析与校验）
3. `Enabled`：已启用并完成 hook/tool/command 注册，运行就绪
4. `Disabled`：已安装但停用
5. `Error`：任意阶段失败（校验/注册/运行时）
6. `Uninstalled`：解除系统登记

说明：
- 校验（validate）是 `Installed → Enabled` 过渡中的内部步骤，不作为独立状态
- 注册（activate）是 `Enabled` 的入口动作，不单独暴露 `Active` 状态
- 这样与产品层 `ExtensionInstallState` 的 5 个值可直接映射

### 状态映射表

| Plugin 内部状态 | ExtensionInstallState | ExtensionHealth |
|---|---|---|
| `Discovered` | `discovered` | `unknown` |
| `Installed` | `installed` | `unknown` |
| `Enabled` | `enabled` | `healthy` |
| `Disabled` | `disabled` | `unknown` |
| `Error` | `error` | `error` |
| `Uninstalled` | （从列表移除） | — |

## 5.5 PluginHost 职责
Rust 侧 `PluginHost` 负责：

1. 发现本地插件目录
2. 解析并校验 `plugin.json`
3. 校验权限与目录边界
4. 安装 / 卸载 / 启用 / 禁用
5. 注册 hooks / tools / commands / skills
6. 维护运行态状态
7. 向前端暴露详情与错误信息

## 5.6 Plugin Tool 执行模型

### 结构化命令格式
一期**不采用**自由格式 shell 字符串（如 `"npx eslint --format json ${workspace}"`），而是使用结构化 `command + args`：

```jsonc
{
  "name": "lint-check",
  "description": "运行 lint 检查",
  "command": "npx",
  "args": ["eslint", "--format", "json"],
  "cwd": "${workspace}",
  "timeoutMs": 60000,
  "requiredPermission": "read"
}
```

这样做的原因：
- 避免 shell 字符串拼接的注入风险（特殊字符、空格等）
- 跨平台 shell quoting 规则不一致
- 与"宿主强治理"目标一致——宿主清楚知道执行了什么命令和参数
- 便于 PolicyEngine 审核和 AuditRepo 记录

### 变量替换
`args`、`cwd`、`env` 中支持以下预定义变量：
- `${workspace}` — 当前 workspace 路径
- `${plugin_dir}` — 当前 plugin 安装目录
- `${thread_id}` — 当前 thread ID（如有）

变量替换由宿主完成，不走 shell 展开。

### 执行协议
宿主通过 **command + args 直接调用（非 shell）+ stdin/stdout JSON** 方式执行：

1. 宿主根据 `command` + `args` 构建进程（直接 spawn，不经过 shell）
2. 通过 stdin 传入调用参数（JSON）
3. 读取 stdout 作为结果（JSON）
4. stderr 作为诊断日志
5. 超时取 tool 级 `timeoutMs` > plugin 级 `timeoutMs` > 默认 30s
6. 非零退出码视为执行失败

```ts
// stdin 输入
type PluginToolInput = {
  args: Record<string, unknown>;
  workspace: string;
  threadId?: string;
};

// stdout 输出
type PluginToolOutput = {
  success: boolean;
  result?: unknown;
  error?: string;
};
```

### 资源限制
- 单次调用超时：默认 30s，最大 300s（tool 级 > plugin 级 > 默认）
- 不允许插件自管常驻进程
- 不允许插件 fork 子进程脱离宿主追踪

## 5.7 Plugin 执行规则
这是一期固定决策：

> Plugin 可以执行外部命令，但不得绕过宿主。

所有 plugin 的外部命令执行必须满足：

1. 由宿主发起
2. 经 `ToolGateway`
3. 经 `PolicyEngine`
4. 高危操作可走 `Approval`
5. 全量进入 `AuditRepo`

### 这样做的价值
- 统一安全口径
- 与内建工具行为一致
- 线程 / run / workspace 上下文可追踪
- 为后续风控和日志排查打基础

## 5.8 Hook 机制
一期支持的 hooks：

- `pre_tool_use`
- `post_tool_use`
- `run_started`
- `run_finished`

用途：
- 审计增强
- 执行前检查
- 结果摘要
- 统计与告警
- 团队规范自动提醒

### Hook 执行协议

Hook 采用与 Plugin Tool 相同的命令行 + stdin/stdout JSON 方式：

```ts
// stdin 输入
type HookInput = {
  event: "pre_tool_use" | "post_tool_use" | "run_started" | "run_finished";
  payload: {
    toolName?: string;
    toolArgs?: Record<string, unknown>;
    toolResult?: unknown;
    runId?: string;
    threadId?: string;
    workspace?: string;
  };
};

// stdout 输出
type HookOutput = {
  action: "continue" | "block";   // 仅 pre_tool_use 可返回 block
  message?: string;               // block 时展示给用户的原因
  metadata?: Record<string, unknown>; // 附加信息，写入审计
};
```

### 执行规则
- `pre_tool_use` hook 可返回 `block` 阻断工具执行，其余 hook 点忽略 `action` 字段
- 同一 hook 点有多个 handler 时，**串行执行**，按 manifest 中声明顺序
- 任一 `pre_tool_use` handler 返回 `block`，后续 handler 不再执行
- 单个 hook 超时默认 5s，超时视为 `continue`（不阻断主流程）
- hook 执行错误记录到审计日志，不阻断主流程（`pre_tool_use` block 除外）

### 硬约束
- hook 只能读取输入 payload，并返回 `continue/block + message + metadata`
- hook **不得发起新的宿主工具调用**（禁止 hook → tool → hook 递归链，防止权限面放大和审计复杂度膨胀）
- hook **不得隐式提升权限**
- 一期 hook **不允许**修改 system prompt，`prompt-injection` 权限不纳入一期
- 若未来支持 prompt 修改，需单独权限并限制仅 append，不可 replace

### Hook 审计关联
- 每次 hook 执行产生独立的 audit event
- 必须携带原始 `tool_call_id` / `run_id`，确保与触发该 hook 的工具调用可关联
- Activity 页和详情页可展示"某次工具调用被哪个 hook 阻断/增强"

## 5.9 Command Provider 定义

一期 `command-provider` 为 **prompt template 级命令**：

- 本质是预定义的 prompt 片段，类似 slash command
- 用户在输入框中通过 `/command-name` 触发
- 触发后将 `promptTemplate` 内容注入到当前对话上下文
- **不触发**独立执行流程，**不纳入**权限与审批体系
- 不支持参数 schema（一期），后续可扩展

```ts
commands?: Array<{
  name: string;                   // 命令名，如 "review-code"
  description: string;            // 用户可见描述
  promptTemplate?: string;        // 注入的 prompt 内容
}>;
```

与现有系统的关系：
- 注册到 Agent Runtime 的 command 列表
- 前端在输入框 `/` 菜单中展示
- 来源标记为 `plugin:<plugin_id>`

---

## 5.10 Plugin 与现有运行时集成
Plugin 暴露的 tools 不直接塞进 UI 或 runtime，而是：

1. PluginHost 注册到 `ExtensionToolRegistry`
2. `ToolGateway` 层增加扩展路由：先查内建 executor，再查 `ExtensionToolRegistry`
3. 所有调用仍经过 `PolicyEngine` + `Approval` + `AuditRepo`

注意：改造点在 `ToolGateway`（路由层），不改 `executors/` 内部各 executor。

### 统一工具结果模型
所有工具（内建 / Plugin / MCP）的执行结果在 ToolGateway 层统一包裹为：

```ts
type UnifiedToolResult = {
  success: boolean;
  content: string;            // 主要结果文本，供 Agent 消费
  structuredData?: unknown;   // 可选结构化数据
  error?: string;
  provider: {
    type: "builtin" | "plugin" | "mcp";
    id: string;               // plugin_id 或 mcp_server_id
  };
};
```

与现有 `ToolOutput` 的关系详见 15.0 Implementation Alignment。

---

## 6. MCP 架构

## 6.1 MCP 定义
MCP 是扩展中心中的独立对象，代表“外部能力连接器”。

在产品层，它与 Plugin、Skills 并列。  
在实现层，它由 `McpHost` 单独管理，不走 Plugin 运行模型。

## 6.2 一期范围
一期支持：

1. `stdio`
2. 配置模型预留 `streamable-http`（MCP 协议已废弃独立 SSE transport，统一为 Streamable HTTP）
3. 工具与资源 discover
4. 连接状态治理
5. 降级与重连

一期不支持：
- 复杂认证流 UI
- marketplace 一键远程部署 server
- 多 transport 全量适配

## 6.3 MCP 配置模型
```ts
export type McpTransport = "stdio" | "streamable-http";

export type McpServerConfig = {
  id: string;
  label: string;
  transport: McpTransport;
  enabled: boolean;
  autoStart: boolean;

  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;

  url?: string;
  headers?: Record<string, string>;

  timeoutMs?: number;
};
```

> **安全提示**：`env` 和 `headers` 字段可能包含 API Key 等敏感凭证。要求：
> - 前端展示时对 env value 做 mask 处理（仅显示前4位 + `****`）
> - 审计日志中不记录 env/headers 的 value
> - 后续考虑引用系统 secret store

## 6.4 MCP 运行态模型
```ts
export type McpServerStatus =
  | "disconnected"
  | "config_error"    // 配置层错误：command 不存在、url 缺失、headers 不合法等
  | "connecting"
  | "connected"
  | "degraded"
  | "error";          // 运行时错误：连接断开、discover 失败、调用超时等

export type McpToolSummary = {
  name: string;
  qualifiedName: string;
  description?: string;
  inputSchema?: Record<string, unknown>; // JSON Schema object
};

export type McpResourceSummary = {
  uri: string;
  name: string;
  description?: string;
  mimeType?: string;
};

export type McpServerState = {
  id: string;
  label: string;
  status: McpServerStatus;
  phase:
    | "config_load"
    | "spawn_connect"
    | "initialize_handshake"
    | "tool_discovery"
    | "resource_discovery"
    | "ready"
    | "invocation"
    | "shutdown"
    | "error";
  tools: McpToolSummary[];
  resources: McpResourceSummary[];
  staleSnapshot: boolean;           // true 表示 tools/resources 为上次成功快照，当前 discover 失败
  lastError?: string;
  updatedAt: string;
};
```

## 6.5 McpHost 职责
Rust 侧 `McpHost` 负责：

1. 读取配置
2. 启动/关闭 stdio server
3. 初始化握手
4. discover tools/resources
5. 构建 registry
6. 调用 server tools
7. 维护 phase/status/error
8. 提供重试与重连能力
9. 将状态暴露给前端

## 6.6 MCP 命名规范
MCP 工具统一命名：

```text
mcp__<server_name>__<tool_name>
```

这样解决：
- 多 server 工具名冲突
- 前端工具列表可直接显示来源
- 运行态容易反查 provider

## 6.7 MCP Degraded Mode
MCP 子系统必须支持部分可用：

- 一个 server 错误，不影响其他 server
- 一个 tool discover 失败，不阻断其他 tool
- UI 能清晰展示 phase 与 lastError
- runtime 能跳过不可用 provider

### Stale Snapshot 策略
- discover 失败时，**保留上次成功发现的 tools/resources**，标记 `staleSnapshot: true`
- UI 展示 stale 标记，避免从"有数据"突然变"空"导致诊断困难
- runtime 可选择是否使用 stale 数据（默认继续使用，标记告警）

这是一期必须保留的设计能力，不是锦上添花。

## 6.8 MCP 凭证安全
MCP 配置中的敏感字段规则：

| 字段 | 敏感级别 | 处理 |
|---|---|---|
| `env` values | sensitive | 前端 mask、审计脱敏、日志不记录 |
| `headers` values | sensitive | 同上 |
| `url` | low | 可展示，但含 token 的 URL 应识别并 mask query params |
| `command` / `args` | non-sensitive | 正常展示和记录 |

即便一期继续使用 settings KV 存储，也必须确保：
- UI 不回显明文凭证
- 导出/诊断功能脱敏
- 日志中不记录 sensitive 字段的值

---

## 7. Skills 架构

## 7.1 Skills 定义
Skills 是 Agent 的能力资产层，不是执行代码层。

Skills 用于：
- 提供任务模式说明
- 辅助 prompt/context 构建
- 提供工作流约束
- 帮助用户理解当前系统能力

## 7.2 Skills 来源
一期固定为三类：

1. `builtin`
2. `workspace`
3. `plugin`

## 7.3 Skills 文件结构
建议每个 skill 一个目录：

```text
<skill-id>/
  SKILL.md
  examples/
  assets/
```

`SKILL.md` 采用 frontmatter + 正文。

示例：

```md
---
id: verify-change
name: Verify Change
description: 校验改动是否引入回归
tags: [verify, review]
triggers: [验证, 校验, 回归, review]
tools: [read, search, shell]
priority: medium
---

当用户要求验证改动时：
1. 明确变更范围
2. 优先读取关键文件和错误信息
3. 运行最小必要检查
4. 输出结果、风险与建议下一步
```

## 7.4 Skills 索引模型
```ts
export type SkillSource = "builtin" | "workspace" | "plugin";

export type SkillRecord = {
  id: string;
  name: string;
  description?: string;
  tags: string[];
  triggers: string[];
  tools: string[];              // 仅用于检索/推荐，不代表权限授予（见下方说明）
  priority?: "low" | "medium" | "high";
  source: SkillSource;
  path: string;
  enabled: boolean;
  pinned: boolean;              // 用户或 profile 显式钉选，跳过自动匹配直接纳入候选
  contentPreview: string;       // 截断摘要，用于索引层展示和 Select 阶段评估
  // 完整内容在 Assemble 阶段按需从 path 读取，不缓存在索引中
};
```

> **重要约束**：skill frontmatter 中的 `tools` 字段仅用于检索排序和推荐展示。它**不能**被解释为权限提升——skill 声明 `tools: [shell]` 不意味着该 skill 可以绕过 `PolicyEngine` 执行 shell 命令。所有工具调用仍然必须经过 `ToolGateway + PolicyEngine` 完整链路。

### Pin/Unpin 机制
- 仅依赖自动关键词检索可能导致技能选择不稳定
- 用户或 agent profile 可显式 pin skill（`pinned: true`），使其跳过 Select 匹配直接纳入候选
- pinned skills 仍受 Prompt Budget 限制
- UI 在 Skills 页提供 pin/unpin 操作

## 7.5 SkillHost 职责
Rust 侧 `SkillHost` 负责：

1. 扫描目录
2. 解析 frontmatter
3. 校验文件与路径安全
4. 建立技能索引
5. 提供预览、启停、重扫
6. 为 Agent session 提供候选技能

## 7.6 Skills 选择流程
不允许全量注入，必须三段式处理：

1. **Index**
   - 扫描目录，解析 frontmatter，构建 `SkillRecord` 索引

2. **Select**
   - 根据用户请求匹配候选技能
   - **一期匹配算法**：关键词子串匹配（对 `triggers` + `tags` + `name` 做 case-insensitive 子串搜索）
   - 后续可演进为 embedding 语义匹配
   - 多个命中时按 `priority` 排序，相同优先级按匹配度（命中字段数）排序

3. **Assemble**
   - 从 `path` 读取完整内容
   - 注入摘要或片段，而非整篇原文
   - 受 Prompt Budget 限制（见 7.7）

## 7.7 Prompt Budget
一期要内建预算限制：

- 单 skill 最大字符数
- 每次最多选 N 个 skills
- 总注入最大字符数
- 超限自动摘要或截断

## 7.8 产品组织
按已确认决策：

> Skills 一期纳入统一 `Extensions Center`

扩展中心共五个页签（三类扩展对象 + Marketplace + Activity），Skills 作为其中一个页签，不单独拆 `Skills Center` 一级入口。详见 13.1。

---

## 8. 宿主子系统设计

## 8.1 Rust 模块结构建议
```text
src-tauri/src/extensions/
  mod.rs
  host.rs
  catalog.rs
  registry.rs
  security.rs

  plugins/
    mod.rs
    manifest.rs
    host.rs
    hooks.rs
    tools.rs

  mcp/
    mod.rs
    host.rs
    transport_stdio.rs
    registry.rs
    lifecycle.rs

  skills/
    mod.rs
    host.rs
    loader.rs
    selector.rs
```

## 8.2 前端模块结构建议
```text
src/modules/extensions-center/
  ui/
  model/
  api/

src/shared/types/extensions.ts
src/shared/types/plugins.ts
src/shared/types/mcp.ts
src/shared/types/skills.ts

src/services/bridge/extension-commands.ts
src/services/bridge/mcp-commands.ts
src/services/bridge/skill-commands.ts
```

---

## 9. 注册表与工具分发设计

## 9.1 统一扩展注册表
Rust 侧新增：

```rs
pub struct ExtensionRegistry {
    plugins: HashMap<String, PluginRuntimeRecord>,
    mcp_servers: HashMap<String, McpServerRuntimeRecord>,
    skills: HashMap<String, SkillRuntimeRecord>,
}
```

用途：
- 为 UI 提供统一总览
- 为 runtime 提供统一查询入口
- 为 Marketplace 做“已安装/未安装/启用状态”映射

## 9.2 工具注册表
新增 `ExtensionToolRegistry`：

```rs
pub enum RegisteredToolProvider {
    Builtin,
    Plugin { plugin_id: String },
    Mcp { server_id: String, tool_name: String },
}

pub struct RegisteredTool {
    pub name: String,
    pub provider: RegisteredToolProvider,
    pub description: Option<String>,
    pub required_permission: String,
}
```

## 9.3 工具执行流程
统一执行路径：

1. Agent / UI 触发工具调用
2. 进入 `ToolGateway`
3. 做 policy / approval / audit
4. 查询 `ExtensionToolRegistry`
5. 路由到 Builtin / Plugin / MCP provider
6. 返回结构化结果

这保证所有工具行为都遵守同一套宿主规则。

---

## 10. 配置与持久化

## 10.1 一期持久化策略
按已确认决策：

> 一期优先使用现有 `settings/policy` KV

原因：
- 对现有项目侵入最小
- 能快速落地
- 足够支撑初期启停、配置、来源和状态偏好

## 10.2 建议配置项

> **命名空间约定**：所有扩展相关 key 统一使用 `extensions.` 前缀，与现有 settings/policy key 隔离，避免冲突。

### settings
- `extensions.marketplace.sources`
- `extensions.plugins.install_dir`
- `extensions.plugins.enabled_ids`
- `extensions.skills.workspace_enabled`
- `extensions.skills.max_prompt_chars`
- `extensions.skills.max_selected_count`
- `extensions.mcp.server_configs`

### policy
- `extensions.plugins.allowed_permissions`
- `extensions.plugins.trusted_sources`
- `extensions.mcp.allowed_transports` — 值为 `["stdio", "streamable-http"]`
- `extensions.mcp.auto_start_enabled`

## 10.3 后续演进
等扩展配置复杂后，再迁移为独立表：

- `extensions`
- `plugin_configs`
- `mcp_servers`
- `skills`
- `extension_events`

---

## 11. Marketplace 集成

## 11.1 Marketplace 一期定位
Marketplace 只承担：

- 扩展发现
- 元数据展示
- 安装说明
- 与本地已安装状态映射

不承担：
- 下载远程代码
- 自动执行远程包

## 11.2 Listing 元数据模型
```ts
export type MarketplaceExtensionListing = {
  id: string;
  kind: "plugin" | "mcp" | "skill";
  name: string;
  version: string;
  description?: string;
  author?: string;
  homepage?: string;
  repository?: string;
  tags: string[];
  permissions: string[];
  installHint?: string;
};
```

## 11.3 与本地安装的关系
用户流应为：

1. 在 Marketplace 看到 listing
2. 查看权限、说明、文档
3. 按说明进行本地安装或本地导入
4. 由宿主完成校验、注册、启停

这样既保留 Marketplace 价值，也避免一期安全边界失控。

---

## 12. 安全设计

## 12.1 路径安全与完整性
所有本地扩展对象都必须：

1. canonicalize
2. 检查目录存在
3. 拒绝非法 symlink 越界
4. 限制读取范围
5. 返回清晰错误

### Manifest 完整性校验
- 安装时对 plugin 目录计算文件树 hash（SHA-256），记录到扩展配置
- 每次启用时比对 hash，不一致则标记为 `Error` 并提示用户确认
- 防止安装后 plugin 目录被篡改

## 12.2 权限模型
权限采用两层：

1. **扩展声明权限（Extension declared permissions）**
   - manifest / config 中声明扩展需要的能力（如 `workspace-read` / `shell-exec`）
   - 仅表示"扩展声称自己需要这些能力"
   - **不直接等于策略审批规则**——声明权限不能自动转化为 `PolicyEngine` 的 allow/deny

2. **宿主运行时决策（Host runtime verdict）**
   - `PolicyEngine` 对每次具体调用做出 `AutoAllow / RequireApproval / Deny` 判断
   - 判断依据包括：tool_name、tool_input、workspace、run_mode，以及 **provider metadata**

### PolicyEngine 扩展适配

当前 `PolicyEngine::evaluate` 签名为 `(tool_name, tool_input, workspace_path, writable_roots, run_mode)`。为支持扩展工具，需要扩展输入参数：

`ToolExecutionRequest` 增加：
```rust
pub struct ToolExecutionRequest {
    // ... 现有字段 ...
    pub provider_type: Option<String>,  // "builtin" | "plugin" | "mcp"
    pub provider_id: Option<String>,    // plugin_id 或 mcp_server_id
}
```

`PolicyEngine::evaluate` 增加 provider 上下文：
```rust
pub async fn evaluate(
    &self,
    tool_name: &str,
    tool_input: &serde_json::Value,
    workspace_canonical_path: Option<&str>,
    writable_roots: &[String],
    run_mode: &str,
    provider_type: Option<&str>,     // 新增
    provider_id: Option<&str>,       // 新增
) -> Result<PolicyCheck, AppError>
```

这使得 PolicyEngine 可以对扩展工具施加额外约束（如：来自特定 plugin 的 shell-exec 始终 RequireApproval）。

### 建议权限项
- `workspace-read`
- `workspace-write`
- `shell-exec`
- `network-access`
- `terminal-control`

其中高危权限（启用需用户明确确认）：
- `shell-exec`
- `terminal-control`

### 一期不纳入的权限
- `prompt-injection`：一期 hook 和 plugin 均不允许修改 system prompt，此权限暂不实现

## 12.3 审计
扩展行为必须进入审计：
- 扩展 ID
- 类型
- run/thread/workspace
- 工具名
- 参数摘要
- 结果与错误

## 12.4 扩展冲突检测
当多个扩展注册同名资源时，按以下规则处理：

- **同名 tool**：拒绝后注册者，标记为 `Error`，提示用户解决冲突
- **同名 command**：同上
- **同一 hook 点多个 handler**：允许共存，按注册顺序串行执行（见 5.8）
- **同名 skill**：按来源优先级 `builtin > workspace > plugin`，低优先级的自动禁用并提示

冲突信息在 Extensions Center 中可见，用户可手动禁用一方解除冲突。

## 12.5 隔离原则
- 单个 plugin 失败不影响其他 plugin
- 单个 MCP server 失败不影响整体
- 单个 skill 解析失败不影响 skill host

---

## 13. 前端产品结构

## 13.1 Extensions Center
统一入口，包含五个页签：

1. **Plugins**
2. **MCP**
3. **Skills**
4. **Marketplace**
5. **Activity** — 扩展专属事件日志流（一期 lightweight 版本）

### 与现有 Marketplace Center 的 IA 迁移策略

当前仓库已有独立的 `src/modules/marketplace-center/`（含 skills/mcps/plugins/automations 四个 tab）。为避免双中心并存：

- **目标 IA**：统一为 Extensions Center，Marketplace 作为其中一个 tab
- **过渡策略**：保留现有 Marketplace Center 入口，但实际导航到 Extensions Center 的 Marketplace tab
- **迁移步骤**：
  1. Phase 1B 建立 Extensions Center 壳层时，将 Marketplace Center 的 catalog 数据和 UI 迁入 Marketplace tab
  2. 旧入口改为 redirect
  3. 稳定后移除旧模块代码

## 13.2 Plugins 页
展示：
- 已安装插件
- 来源
- 权限
- 启用状态
- 提供的 hooks/tools/commands
- 最近错误

## 13.3 MCP 页
展示：
- server 列表
- transport
- status
- phase
- tools/resources 数量
- 最近错误
- 手动重连入口

## 13.4 Skills 页
展示：
- 技能列表
- 来源
- tags / triggers
- 是否启用
- 内容预览
- prompt 预算占用

## 13.5 Marketplace 页（Extensions Center 第四个 tab）
展示：
- listing 列表
- 本地安装说明
- 是否已安装
- 权限与风险说明
- 文档跳转

## 13.6 Activity 页（Extensions Center 第五个 tab，lightweight 版本）

一期为轻量版，仅展示核心事件：
- 扩展安装/启用/禁用/错误
- MCP 连接/断开/discover 错误
- Plugin tool 调用成功/失败

后续增强项（不纳入一期）：
- Hook 详细执行流
- 全量筛选与跨类型关联
- 事件统计与趋势

---

## 14. IPC 设计

## 14.1 扩展总览
- `extensions_list`
- `extension_get_detail`
- `extension_enable`
- `extension_disable`
- `extension_uninstall`

## 14.2 Plugin
- `plugin_install_from_dir`
- `plugin_validate_dir`
- `plugin_update_config`

## 14.3 MCP
- `mcp_list_servers`
- `mcp_add_server`
- `mcp_update_server`
- `mcp_remove_server`
- `mcp_restart_server`
- `mcp_get_server_state`

## 14.4 Skills
- `skill_list`
- `skill_rescan`
- `skill_enable`
- `skill_disable`
- `skill_preview`

---

## 15. 与现有系统的实现对齐（Implementation Alignment）

## 15.0 当前系统接缝分析

扩展系统接入现有代码库时，以下接缝需要明确处理方式：

### ToolGateway 改造
- **改造方式**：在 `ToolGateway::execute_tool_call` 内部增加 provider 路由扩展点
- **不重写**现有 `executors::execute_tool` 分发逻辑
- 路由顺序：先匹配内建工具（现有 executor），未命中再查 `ExtensionToolRegistry`
- `ToolExecutionRequest` 增加 `provider_type` / `provider_id` 字段（内建工具为 `None`）

### PolicyEngine 扩展
- 当前 `PolicyEngine::evaluate` 按 `tool_name + tool_input` 分类判断
- 需要增加 `provider_type` / `provider_id` 参数，使 PolicyEngine 可对扩展工具施加额外策略
- 现有内建工具调用不受影响（provider 参数为 `None`，走原有逻辑）

### UnifiedToolResult 与现有 ToolOutput 的关系
- 当前 Rust 侧已有 `ToolOutput { success: bool, result: Value }`
- `UnifiedToolResult` 在 `ToolOutput` 基础上**包裹** provider metadata，不替代 `ToolOutput`
- 一期采用"包裹型"兼容方案：
  - 内建工具执行器继续返回 `ToolOutput`
  - `ToolGateway` 层将 `ToolOutput` + provider info 包装为 `UnifiedToolResult`
  - 扩展工具（Plugin / MCP）输出直接构造为 `UnifiedToolResult`
- 底层 executor 先不强制重构，后续成熟后再考虑统一

### AuditRepo 复用
- 先复用现有 `AuditInsert` 结构
- 通过 `source` 字段区分扩展来源（如 `"plugin:lint-tool"` / `"mcp:brave-search"`)
- 通过 `target_type` + `target_id` 标识扩展对象
- 后续可抽象为 extension-facing audit service

---

## 15.1 Session 构建阶段
在 session 构建中增加扩展上下文组装：

1. 当前 workspace
2. 当前可见工具清单
3. 已启用 skills 摘要
4. command / thread 相关扩展上下文

## 15.2 Tool 执行阶段
所有扩展工具统一走现有：
- `ToolGateway`
- `PolicyEngine`
- `AuditRepo`

## 15.3 Run 生命周期阶段
支持：
- `run_started`
- `run_finished`
- `pre_tool_use`
- `post_tool_use`

---

## 16. 分阶段实施建议

## 16.1 Phase 1A：后端骨架
交付：
- ExtensionRegistry（统一注册表）
- ExtensionToolRegistry（统一工具路由）
- ToolGateway provider 路由扩展点
- ToolExecutionRequest 增加 provider_type / provider_id
- PolicyEngine 增加 provider 上下文参数
- UnifiedToolResult 包裹型适配层
- shared types 初稿（`extensions.ts / plugins.ts / mcp.ts / skills.ts`）
- settings key namespace（`extensions.*`）

> 本阶段不含具体 provider 实现和前端 UI，只建立后端骨架和类型定义。

## 16.2 Phase 1B：前端壳层
交付：
- Extensions Center 外壳（tab 框架：Plugins / MCP / Skills / Marketplace）
- 各 tab 基础空页面
- 现有 Marketplace Center 入口接入统一导航（redirect）
- Marketplace tab 集成现有 catalog 数据

> Activity tab 不纳入本阶段，作为后续增强项。

## 16.3 Phase 2：MCP Host
交付：
- stdio transport
- server registry
- discover（tools + resources）+ stale snapshot
- config_error / status / phase / error UI
- MCP tool 注册到 ExtensionToolRegistry
- MCP tool 执行经 ToolGateway 全链路打通
- MCP 凭证脱敏展示

> MCP 是用户最直接感知价值的扩展类型，优先落地以验证整体架构。

## 16.4 Phase 3：Plugin Tool Execution + Hooks
交付：
- Plugin manifest 发现与校验（含完整性 hash）
- Plugin 启停生命周期
- Plugin tool execution（结构化 command + args，stdin/stdout JSON 协议）
- pre/post tool hooks（含审计关联）
- command-provider 接入（prompt template 级）
- 本地导入 plugin

> 与 Phase 2 的 MCP 执行路径复用 ExtensionToolRegistry + ToolGateway。

## 16.5 Phase 4：Skill Host
交付：
- builtin/workspace/plugin 扫描
- skill 索引（关键词子串匹配）
- skill 预览、启停、pin/unpin
- prompt 预算控制
- Skills tab UI

## 16.6 Phase 5：增强项
交付：
- Activity tab（lightweight 版本）
- 扩展冲突检测 UI
- hook 详细执行流日志
- 跨类型事件关联

---

## 17. 硬约束清单（Guardrails）

以下约束为一期不可违反的硬性规则，实现时不得绕过：

1. **Plugin tool 不允许绕过宿主执行外部命令** — 所有执行必须经 ToolGateway
2. **Plugin tool `entry` 不采用自由格式 shell 字符串** — 必须使用结构化 `command + args`
3. **Hook 不允许发起新的宿主工具调用** — 禁止 hook → tool → hook 递归链
4. **Hook 不允许修改 system prompt** — `prompt-injection` 权限一期不实现
5. **Skill frontmatter 中的 `tools` 不代表授权** — 不能绕过 PolicyEngine
6. **Extension manifest permission 不直接等于 policy approval rule** — 声明权限 ≠ 运行时决策
7. **MCP 凭证字段必须脱敏展示与脱敏记录** — env/headers 的 value 不可明文暴露
8. **Discover 失败时保留上次成功快照并标记 stale** — 防止 UI 突变
9. **三类对象各自保持独立内部状态机** — 统一展示不等于统一底层模型
10. **所有扩展工具调用必须经过 PolicyEngine + Approval + AuditRepo 完整链路**

---

## 18. 风险与控制

### 主要风险
1. 插件执行能力带来宿主风险面扩大
2. MCP 连接不稳导致用户感知差
3. skills 注入不受控导致 prompt 过大
4. UI 做成”展示页”而非”可诊断控制台”
5. Phase 1 范围过大导致交付延期

### 对应控制
- 严格要求所有扩展调用归一到 ToolGateway
- MCP phase/status/error 全量可见
- skills 强预算与选择器
- 所有扩展状态、权限、错误在 UI 可见
- Phase 1 拆为 1A（后端骨架）+ 1B（前端壳层），降低首次交付复杂度

---

## 19. 推荐的下一步

建议下一步直接进入 **实施规格文档**，把这份架构拆成可开发事项。
最合理的输出顺序是：

1. **Phase 1A 实施方案**
   - Rust 侧模块 skeleton
   - ToolGateway / PolicyEngine 改造点
   - 共享类型草案（`extensions.ts / plugins.ts / mcp.ts / skills.ts`）
   - settings key namespace 定义
2. **Phase 1B 实施方案**
   - Extensions Center UI 信息架构
   - Marketplace Center 迁移步骤
   - 各 tab 状态流
3. **Phase 2（MCP Host）实施方案**

如果继续推进，建议下一步产出：

- **《tiy-desktop 扩展系统实施方案 Phase 1A》**
