# tiy-desktop 扩展架构设计文档 v1.0

## 1. 结论

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

## 4.1 统一产品抽象
前端产品层和 IPC 返回统一使用扩展摘要对象：

```ts
export type ExtensionKind = "plugin" | "mcp" | "skill-pack";

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

- 这是产品层统一模型
- 底层 plugin/mcp/skills 各自仍保持独立实现
- 这样 UI 可以用统一列表页、筛选器和状态组件

---

## 5. Plugin 架构

## 5.1 Plugin 定义
Plugin 是本地扩展包单元，支持以下能力类型：

- `hook`
- `tool-provider`
- `command-provider`
- `skill-pack`
- `ui-metadata`

一期不支持：
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
    "hook" | "tool-provider" | "command-provider" | "skill-pack"
  >;

  permissions: {
    fs?: "none" | "workspace-read" | "workspace-write";
    shell?: boolean;
    network?: boolean;
    promptInjection?: boolean;
    terminalControl?: boolean;
  };

  hooks?: {
    preToolUse?: string[];
    postToolUse?: string[];
    onRunStart?: string[];
    onRunComplete?: string[];
  };

  tools?: Array<{
    name: string;
    description: string;
    entry: string;
    requiredPermission:
      | "read-only"
      | "workspace-write"
      | "danger-full-access";
  }>;

  commands?: Array<{
    name: string;
    description: string;
    promptTemplate?: string;
  }>;

  skillsDir?: string;

  configSchema?: {
    type: "json-schema";
    path: string;
  };
};
```

## 5.4 Plugin 生命周期
状态机建议为：

1. `Discovered`
2. `Installed`
3. `Validated`
4. `Enabled`
5. `Active`
6. `Error`
7. `Disabled`
8. `Uninstalled`

### 生命周期含义
- `Discovered`：目录被发现，但尚未纳管
- `Installed`：已登记到系统配置中
- `Validated`：manifest、权限、目录校验通过
- `Enabled`：用户或默认配置启用
- `Active`：已完成 hook/tool/command 注册
- `Error`：启动或注册失败
- `Disabled`：已安装但停用
- `Uninstalled`：解除系统登记

## 5.5 PluginHost 职责
Rust 侧 `PluginHost` 负责：

1. 发现本地插件目录
2. 解析并校验 `plugin.json`
3. 校验权限与目录边界
4. 安装 / 卸载 / 启用 / 禁用
5. 注册 hooks / tools / commands / skills
6. 维护运行态状态
7. 向前端暴露详情与错误信息

## 5.6 Plugin 执行规则
这是一期固定决策：

> Plugin 可以执行外部命令，但不得绕过宿主。

所有 plugin 的外部命令执行必须满足：

1. 由宿主发起，不允许插件自管常驻任意进程
2. 经 `ToolGateway`
3. 经 `PolicyEngine`
4. 高危操作可走 `Approval`
5. 全量进入 `AuditRepo`

### 这样做的价值
- 统一安全口径
- 与内建工具行为一致
- 线程 / run / workspace 上下文可追踪
- 为后续风控和日志排查打基础

## 5.7 Hook 机制
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

### 高危边界
- 直接修改 system prompt 的 hook 不纳入一期默认能力
- 若未来支持，需单独权限 `promptInjection`

## 5.8 Plugin 与现有运行时集成
Plugin 暴露的 tools 不直接塞进 UI 或 runtime，而是：

1. PluginHost 注册到 `ExtensionToolRegistry`
2. `executors::execute_tool` 先查内建，再查扩展
3. 所有调用仍经过 `ToolGateway`

这保证扩展工具与现有工具完全统一治理。

---

## 6. MCP 架构

## 6.1 MCP 定义
MCP 是扩展中心中的独立对象，代表“外部能力连接器”。

在产品层，它与 Plugin、Skills 并列。  
在实现层，它由 `McpHost` 单独管理，不走 Plugin 运行模型。

## 6.2 一期范围
一期支持：

1. `stdio`
2. 配置模型预留 `http` / `sse`
3. 工具与资源 discover
4. 连接状态治理
5. 降级与重连

一期不支持：
- 复杂认证流 UI
- marketplace 一键远程部署 server
- 多 transport 全量适配

## 6.3 MCP 配置模型
```ts
export type McpTransport = "stdio" | "http" | "sse";

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

## 6.4 MCP 运行态模型
```ts
export type McpServerStatus =
  | "disconnected"
  | "connecting"
  | "connected"
  | "degraded"
  | "error";

export type McpToolSummary = {
  name: string;
  qualifiedName: string;
  description?: string;
  inputSchema?: unknown;
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

这是一期必须保留的设计能力，不是锦上添花。

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
  tools: string[];
  priority?: "low" | "medium" | "high";
  source: SkillSource;
  path: string;
  enabled: boolean;
  content: string;
};
```

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
   - 建索引

2. **Select**
   - 根据用户请求、workspace、command、tools 需求挑选候选技能

3. **Assemble**
   - 注入摘要或片段，而非整篇原文

## 7.7 Prompt Budget
一期要内建预算限制：

- 单 skill 最大字符数
- 每次最多选 N 个 skills
- 总注入最大字符数
- 超限自动摘要或截断

## 7.8 产品组织
按已确认决策：

> Skills 一期纳入统一 `Extensions Center`

因此扩展中心有三个 tab：

- Plugins
- MCP
- Skills

不单独拆 `Skills Center` 一级入口。

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
- `extensions.mcp.allowed_transports`
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
  kind: "plugin" | "mcp" | "skill-pack";
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

## 12.1 路径安全
所有本地扩展对象都必须：

1. canonicalize
2. 检查目录存在
3. 拒绝非法 symlink 越界
4. 限制读取范围
5. 返回清晰错误

## 12.2 权限模型
权限采用三层：

1. **声明权限**
   - manifest / config 声明

2. **策略允许**
   - policy 判断当前宿主是否允许

3. **运行时审批**
   - 高危动作仍可触发 approval

### 建议权限项
- `workspace-read`
- `workspace-write`
- `shell-exec`
- `network-access`
- `prompt-injection`
- `terminal-control`

其中高危权限：
- `shell-exec`
- `prompt-injection`
- `terminal-control`

## 12.3 审计
扩展行为必须进入审计：
- 扩展 ID
- 类型
- run/thread/workspace
- 工具名
- 参数摘要
- 结果与错误

## 12.4 隔离原则
- 单个 plugin 失败不影响其他 plugin
- 单个 MCP server 失败不影响整体
- 单个 skill 解析失败不影响 skill host

---

## 13. 前端产品结构

## 13.1 Extensions Center
统一入口，包含三个页签：

1. **Plugins**
2. **MCP**
3. **Skills**

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

## 13.5 Marketplace 页
展示：
- listing 列表
- 本地安装说明
- 是否已安装
- 权限与风险说明
- 文档跳转

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

## 15. 与现有 Agent Runtime 的集成

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

## 16.1 Phase 1：扩展基础设施
交付：
- ExtensionRegistry
- Plugin manifest 发现与启停
- Extensions Center 基础 UI
- Marketplace listing 展示
- 本地导入 plugin

## 16.2 Phase 2：MCP Host
交付：
- stdio transport
- server registry
- discover
- status/phase/error UI
- MCP tool 注册

## 16.3 Phase 3：Skill Host
交付：
- builtin/workspace/plugin 扫描
- skill 索引
- skill 预览与启停
- prompt 预算控制

## 16.4 Phase 4：Hook 与扩展工具执行
交付：
- pre/post tool hooks
- plugin tool execution
- command-provider 接入

---

## 17. 风险与控制

### 主要风险
1. 插件执行能力带来宿主风险面扩大
2. MCP 连接不稳导致用户感知差
3. skills 注入不受控导致 prompt 过大
4. UI 做成“展示页”而非“可诊断控制台”

### 对应控制
- 严格要求所有扩展调用归一到 ToolGateway
- MCP phase/status/error 全量可见
- skills 强预算与选择器
- 所有扩展状态、权限、错误在 UI 可见

---

## 18. 推荐的下一步

我建议下一步直接进入 **实施规格文档**，把这份架构拆成可开发事项。  
最合理的输出顺序是：

1. **Phase 1 实施方案**
   - 后端模块
   - 前端页面
   - IPC 列表
   - 状态模型
2. **共享类型草案**
   - `extensions.ts / plugins.ts / mcp.ts / skills.ts`
3. **Rust 侧模块 skeleton**
4. **UI 信息架构与状态流**

如果继续推进，建议下一步产出：

- **《tiy-desktop 扩展系统实施方案 Phase 1》**
