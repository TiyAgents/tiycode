# Tiy Agent 实施落地计划

## Context

Tiy Agent 是一款面向开发者的 AI 桌面工作台，当前处于 Alpha 原型向正式产品
收敛阶段。

本计划基于 2026-03-19 的架构修订：

- 不再建设 `TS Agent Sidecar`
- 改为 `Frontend -> Rust Core -> BuiltInAgentRuntime -> tiy-core -> LLM`
- helper-agent 作为 Rust 内部编排能力存在，并在主线程中折叠展示

**当前状态**：

- Rust 后端已具备基础 manager / repo / settings / provider 集成雏形
- Provider Settings 与 `tiy-core` 对齐已启动
- 前端仍有原型态渲染与 mock 组件残留
- Agent Run 相关实现仍带有 sidecar 过渡结构，需要整体转向

**目标**：

按三阶段将原型态产品演进为真实执行态，建立
`Frontend -> Rust Core -> BuiltInAgentRuntime -> tiy-core -> LLM`
的完整链路。

---

## Phase 1: 线程与 Agent 真链路（~8 周）

### M1.1 项目基础设施与数据库层（1-2 周）

**目标**：建立 Rust 异步运行时、SQLite 连接池、日志系统和
`$HOME/.tiy/` 目录初始化。

**Rust 变更**：

- 完善 `tokio`、`sqlx`、`tracing`、`uuid`、`chrono` 等依赖接入
- 初始化 `$HOME/.tiy/` 目录树、数据库和系统日志路径
- 统一 `AppState` 与 `AppError`

**验收标准**：

- `cargo build` 通过
- app 启动后 `$HOME/.tiy/db/tiy-agent.db` 正常存在
- 日志写入系统惯例路径

---

### M1.2 Workspace 管理（1-2 周）

**目标**：实现 WorkspaceManager 作为全局上下文边界。

**Rust 新建/扩展**：

- `core/workspace_manager.rs`
- `commands/workspace.rs`
- `persistence/repo/workspace_repo.rs`
- `model/workspace.rs`

**前端变更**：

- Workspace invoke 封装
- Settings/Workbench 使用真实 Workspace 数据

**验收标准**：

- 工作区路径 canonicalize 后入库
- 启动时自动重验证路径状态

---

### M1.3 设置与配置系统（1 周）

**目标**：Settings、Provider、Agent Profile、Policy 持久化迁移到 Rust。

**Rust 新建/扩展**：

- `core/settings_manager.rs`
- `commands/settings.rs`
- `persistence/repo/settings_repo.rs`
- `persistence/repo/provider_repo.rs`
- `persistence/repo/profile_repo.rs`

**关键实现**：

- Provider 设置完全对齐 `tiy-core`
- Agent Profile 保留三层模型映射：
  - `primary`
  - `assistant`
  - `lite`
- 为后续 runtime 冻结 `effective_model_plan_json` 做准备

**验收标准**：

- 设置跨重启持久化
- Provider API Key 加密存储
- Profile 三层模型映射可配置

---

### M1.4 Thread 核心（1-2 周）

**目标**：线程 CRUD、消息 append-only 持久化、快照构建。

**Rust 新建/扩展**：

- `core/thread_manager.rs`
- `commands/thread.rs`
- `persistence/repo/thread_repo.rs`
- `persistence/repo/message_repo.rs`
- `persistence/repo/run_repo.rs`
- `model/thread.rs`

**关键实现**：

- 长线程分页
- ThreadStatus 从最新 run 状态推导
- ThreadSnapshot 组装 recent messages、summary、active run、pending approvals

**验收标准**：

- 新建线程归属工作区
- 消息重启后可恢复
- 长线程支持分页加载

---

### M1.5 Built-In Agent Runtime（1-2 周）

**目标**：建立 `AgentRunManager + BuiltInAgentRuntime + AgentSession`，
打通 Rust 与 `tiy-core` 的真实运行链路，移除 sidecar 方案。

**Rust 新建/重构文件**：

- `core/agent_run_manager.rs` — parent run 生命周期、事件聚合、
  `effective_model_plan_json` 冻结
- `core/built_in_agent_runtime.rs` — runtime 创建、session 管理、统一事件翻译
- `core/agent_session.rs` — 单个 parent run 的 in-memory runtime session
- `core/helper_agent_orchestrator.rs` — helper task 编排、取消传播、结果折叠
- `commands/agent.rs` — `thread_start_run`、`thread_cancel_run`、`tool_approval_respond`
- `ipc/frontend_channels.rs` — ThreadStreamEvent 定义
- `persistence/repo/run_helper_repo.rs` — helper 摘要持久化
- `src-tauri/migrations/<timestamp>_run_helpers.sql` — `run_subtasks -> run_helpers`
  迁移与兼容处理

**必须删除/替换的 sidecar 残留**：

- 删除 `core/sidecar_manager.rs`
- 删除 `ipc/sidecar_protocol.rs`
- 删除 `commands/agent.rs` 中的 `sidecar_status`
- 删除 `services/bridge/agent-commands.ts` 中的 `sidecarStatus` 调用
- 删除 `shared/types/api.ts` 中的 `SidecarStatusDto`
- 重构 `core/app_state.rs`，移除 `sidecar_manager`
- 重构 `core/agent_run_manager.rs`，移除 sidecar 发送/接收路径
- 重构 `src-tauri/src/lib.rs`，移除 sidecar 启动逻辑与环境变量解析
- 收敛 `model/errors.rs` 中 sidecar 专属错误来源
- 清理所有 sidecar 过渡测试、注释和 bridge 类型

**关键实现**：

- `tiy-core::agent::Agent` 作为主 agent kernel
- `AgentSession` 通过 `set_tool_executor(...)` 将 `ToolGateway` 注册为
  `tiy-core` tool executor bridge
- approval 等待通过挂起 tool executor future 实现，parent run 在等待期间
  进入 `WaitingApproval`
- run 启动时冻结 effective model plan：
  - `primary_model`
  - `helper_default_model`
  - `lite_model`
  - `thinking_level`
  - `transport`
  - `tool_profile_by_mode`
- helper-agent 通过 Rust orchestration tool 调起，不暴露成独立 thread run
- helper 不开启独立 approval UI；遇到需要审批或 mutating 的 helper tool
  路径时，折叠回 parent run 做 escalation
- parent run 仍是一线程唯一 active run
- parent run 保留 `Cancelling` 中间态，用于等待 tool cleanup、helper 停止和超时收尾
- 迁移 `run_subtasks -> run_helpers`，为 helper 摘要持久化提供新 repo 和新 schema

**前端变更**：

- PromptInput 提交触发 `thread_start_run`
- ThreadStreamEvent 直接消费 Rust runtime 事件
- 主线程支持 helper 折叠块渲染

**验收标准**：

- 用户发送 prompt -> Rust runtime -> `tiy-core` -> LLM -> 流式回显线程
- 无 sidecar 依赖
- helper 状态可折叠展示在主线程
- 取消中的 run 会先进入 `Cancelling`，再稳定落到 `Cancelled` 或 `Interrupted`
- `run_helpers` 持久化生效，旧 `run_subtasks` 不再作为新链路写入目标

---

### M1.6 Tool Gateway 与 Policy Engine（1-2 周）

**目标**：实现统一工具执行网关和权限策略引擎。

**Rust 新建/扩展**：

- `core/tool_gateway.rs`
- `core/policy_engine.rs`
- `core/executors/filesystem.rs`
- `core/executors/search.rs`
- `core/executors/process.rs`
- `persistence/repo/audit_repo.rs`

**关键实现**：

- schema 校验 -> PolicyEngine -> 审批 -> 执行 -> 审计
- `plan` 模式工具画像与 policy ceiling 协同工作
- helper 继承 parent `run_mode` 上限，不能绕过只读限制
- tool executor、ToolGateway、approval resolution 与 cancellation token 协同工作

**验收标准**：

- mutating 工具需要审批或被拒绝
- `plan` 模式下 mutating 工具被拒绝或显式升级
- audit_events 记录完整

---

### M1.7 前端完整集成（1 周）

**目标**：移除 mock 数据和 localStorage fallback，前端完全对接 Rust。

**前端变更**：

- ThreadStreamEvent adapter：
  - `message_*` -> Conversation
  - `tool_*` -> Tool
  - `approval_required` -> Confirmation
  - `helper_*` -> 折叠 helper block
  - `plan_artifact_updated` -> Plan
- 错误态映射与断线重拉 snapshot
- 清理 mock fixtures 与 localStorage fallback

**验收标准**：

- 全链路可用：工作区 -> 线程 -> 对话 -> 工具调用 -> 审批 -> helper 折叠结果
- 无 mock 数据残留

---

### M1.8 Index 基础（1 周）

**目标**：文件树缓存 + ripgrep 文本检索。

**Rust 新建/扩展**：

- `core/index_manager.rs`
- `commands/index.rs`

**验收标准**：

- 中型仓库文件树加载稳定
- `search_repo` 工具使用 IndexManager

---

## Phase 2: 本地能力真实化（~4 周）

### M2.1 Terminal Manager（1-2 周）

**目标**：真实 PTY 会话与 thread 级终端绑定。

**Rust 新建/扩展**：

- `core/terminal_manager.rs`
- `commands/terminal.rs`
- `core/executors/terminal.rs`

**验收标准**：

- 终端可交互
- Agent 可通过 terminal 工具族操作
- 切线程不中断后台 PTY

### M2.2 Git 能力（2-3 周）

拆分为：

- `M2.2a Git Read Backend + TreeView`
- `M2.2b Git Panel Read Experience`
- `M2.2c Git CLI Mutations`

保持原方向：

- 只读能力优先基于 `git2-rs`
- 写操作与远端操作走 Git CLI + PolicyEngine + Audit

### M2.3 Index 增强（1 周）

- FTS5 虚拟表 + 增量索引
- 文件树持久化到 `$HOME/.tiy/cache/index/`

---

## Phase 3: 扩展生态（方向性规划）

### M3.1 MarketplaceHost

- Skills / MCP / Plugins / Automations 统一 install/enable/disable

### M3.2 MCP 生命周期

- 从 `config.json` 读取 MCP Server 配置
- Rust 宿主管理 MCP 进程（spawn/health/restart）
- 将 MCP 工具暴露给 `AgentSession` 的 runtime tool registry

### M3.3 Plugin 系统

- `$HOME/.tiy/plugins/` 加载插件
- 插件工具统一经 `ToolGateway`

### M3.4 Automation Scheduler

- 复用 thread/run 模型执行自动化任务
- `automation_runs` 表记录执行历史

---

## 关键路径依赖图

```text
M1.1 基础设施
 ├── M1.2 Workspace ──── M1.8 Index 基础
 │    └── M1.3 Settings
 │         └── M1.4 Thread
 │              └── M1.5 Built-In Agent Runtime
 │                   └── M1.6 ToolGateway + Policy
 │                        └── M1.7 前端集成
 │                             ├── M2.1 Terminal
 │                             ├── M2.2 Git
 │                             └── M2.3 Index 增强
 │                                  └── M3.x 扩展生态
```

## 并行开发策略

- **前端与 Rust 并行**：
  前端维持 `isTauri()` 检测与过渡层，直到对应 Rust 里程碑完成
- **Runtime 与工具层并行**：
  M1.5 期间可以先用 mocked ToolGateway 事件打通 `tiy-core` streaming 渲染
- **M1.2 与 M1.8 可并行**：
  Index 仅依赖 M1.1
- **M2.1 与 M2.2 可并行**：
  Terminal 与 Git 共用 M1.6 基础，但业务可分开推进

不再存在“Sidecar 独立开发”分支。

## 验证方式

| 阶段 | 验证方法 |
|------|----------|
| 每个里程碑 | `cargo test` 单元测试 + Rust 集成测试 |
| M1.5 完成后 | 端到端测试：prompt -> `tiy-core` -> streaming response |
| M1.6 完成后 | plan mode / approval / helper inheritance 测试 |
| M1.7 完成后 | 全链路验收：工作区 -> 线程 -> 对话 -> 工具调用 -> helper -> 审批 |
| M2.2 完成后 | 本地闭环：对话 + 项目树 + Git + 终端 |

## 输出文档

实施过程中需同步更新的文档：

- `docs/technical-architecture-20260316.md`
- `docs/database-design-20260316.md`
- `docs/module/agent-run-design-20260316.md`
- `docs/module/agent-tools-design-20260316.md`
- `docs/superpowers/specs/2026-03-19-built-in-agent-runtime-tiy-core-design.md`
