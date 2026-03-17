# Tiy Agent 实施落地计划

## Context

Tiy Agent 是一款面向开发者的 AI 桌面工作台，当前处于 Alpha 原型向正式产品收敛阶段。产品需求、技术架构、10 个模块设计文档和数据库设计均已完成审查并批准进入实施阶段。

**当前状态**：Rust 后端仅有 3 个系统命令（app 元数据、工作区应用列表、外部打开）；前端使用 localStorage + mock 数据完整表达了产品形态，但无真实执行能力。数据库迁移 SQL 已就绪但未接入。

**目标**：按三阶段将原型态产品演进为真实执行态，建立"Frontend → Rust Core → TS Sidecar → LLM"的完整链路。

---

## Phase 1: 线程与 Agent 真链路（~8 周）

### M1.1 项目基础设施与数据库层（1-2 周）

**目标**：建立 Rust 异步运行时、SQLite 连接池、日志系统和 `$HOME/.tiy/` 目录初始化。

**Rust 变更**：

- `Cargo.toml` 新增依赖：`tokio`、`sqlx`（sqlite+chrono+uuid）、`tracing`+`tracing-subscriber`+`tracing-appender`、`dirs`、`thiserror`、`anyhow`、`uuid`（v7）、`chrono`
- 新建 `src-tauri/src/persistence/mod.rs` — 连接池初始化（WAL、foreign_keys、busy_timeout）
- 新建 `src-tauri/src/persistence/sqlite/mod.rs` — SQLite 配置
- 新建 `src-tauri/src/persistence/sqlite/migrations.rs` — `sqlx::migrate!` 执行器
- 新建 `src-tauri/src/core/mod.rs` + `core/app_state.rs` — 全局 AppState（持有 pool + managers）
- 新建 `src-tauri/src/model/errors.rs` — 统一 `AppError` 类型（errorCode、category、source、userMessage、retryable）
- 修改 `src-tauri/src/lib.rs` — setup 阶段：初始化 `$HOME/.tiy/` 目录树 → 初始化日志 → 创建连接池 → 运行迁移 → 构造 AppState 并 manage

**前端变更**：无。继续使用 localStorage mock。

**验收标准**：
- `cargo build` 通过，app 启动后 `$HOME/.tiy/db/tiy-agent.db` 存在且含 17 张表
- 日志写入系统惯例路径（macOS `~/Library/Logs/TiyAgent/`）
- `cargo test` persistence 模块通过

---

### M1.2 Workspace 管理（1-2 周）

**目标**：实现 WorkspaceManager 作为全局上下文边界。

**Rust 新建文件**：
- `core/workspace_manager.rs` — 路径规范化、校验、状态管理、重验证
- `commands/workspace.rs` — `workspace_list`、`workspace_add`、`workspace_remove`、`workspace_set_default`、`workspace_validate`
- `persistence/repo/workspace_repo.rs` — CRUD
- `model/workspace.rs` — WorkspaceRecord、WorkspaceStatus、WorkspaceDto

**前端变更**：
- 新建 `src/features/workspace/api/workspace-commands.ts` — invoke 封装
- 修改 `modules/workbench-shell/model/` — 控制器优先调 Rust invoke，fallback localStorage
- 修改 `modules/settings-center/` — Workspace 标签页对接真实数据

**验收标准**：
- 通过文件夹选择器添加工作区，路径经过 canonicalize 后入库
- 重复路径被拒绝，侧边栏工作区列表从 SQLite 加载
- 启动时自动重验证路径状态（Missing/Ready）

---

### M1.3 设置与配置系统（1 周）

**目标**：Settings、Provider、Agent Profile、Policy 持久化迁移到 Rust。

**Rust 新建文件**：
- `core/settings_manager.rs` — 统一 settings/policies/providers/profiles 管理
- `commands/settings.rs` — settings_get/set/get_all、provider_list/create/update/delete、profile_list/create/update/delete、policy_get/set
- `persistence/repo/settings_repo.rs`、`provider_repo.rs`、`profile_repo.rs`
- `model/settings.rs`、`model/provider.rs`

**前端变更**：
- 修改 `modules/settings-center/model/use-settings-controller.ts` — 从 localStorage 迁移到 invoke
- 修改 `modules/settings-center/model/settings-storage.ts` — 适配双源（Rust 优先）
- Provider 管理 UI、Profile 编辑 UI、Permissions 标签页对接真实数据

**验收标准**：
- 设置跨重启持久化（不再依赖 localStorage）
- Provider API Key 加密存储
- Profile 三层模型映射（primary/auxiliary/lightweight）可配置

---

### M1.4 Thread 核心（1-2 周）

**目标**：线程 CRUD、消息 append-only 持久化、快照构建。

**Rust 新建文件**：
- `core/thread_manager.rs` — 创建/加载/删除线程，消息追加，快照构建，ThreadStatus 派生
- `commands/thread.rs` — thread_create/list/load/update_title/delete/add_message
- `persistence/repo/thread_repo.rs`、`message_repo.rs`、`run_repo.rs`
- `model/thread.rs` — ThreadRecord、ThreadSnapshot、MessageRecord、ThreadStatus 枚举

**关键实现**：
- 消息分页：基于 UUID v7 游标 `WHERE id < ? ORDER BY id DESC LIMIT ?`
- ThreadStatus 由最新 run 状态推导（参见 thread-design §ThreadStatus Derivation）
- ThreadSnapshot 组装：base metadata + recent messages + summary + active run + pending approvals

**前端变更**：
- 新建 `src/services/thread-stream/types.ts` — ThreadStreamEvent 类型定义
- 修改 `modules/workbench-shell/` — 侧边栏线程列表从 Rust 加载，消息列表分页
- 移除 `fixtures.ts` 中的 WORKSPACE_ITEMS/THREAD mock 数据依赖

**验收标准**：
- 新建线程归属工作区，侧边栏按 last_active_at 排序
- 消息追加后持久化，重启后恢复
- 长线程支持分页加载

---

### M1.5 Agent Run 与 Sidecar 连接（1-2 周）

**目标**：建立 AgentRunManager + SidecarManager，实现 JSON-RPC/NDJSON 协议，完成 Rust ↔ Sidecar 双向通信。

**Rust 新建文件**：
- `core/agent_run_manager.rs` — run 生命周期、事件聚合、effective model plan 冻结
- `core/sidecar_manager.rs` — 进程启动/健康监控/graceful restart/crash recovery
- `commands/agent.rs` — thread_start_run、thread_cancel_run、tool_approval_respond
- `ipc/sidecar_protocol.rs` — JSON-RPC request/response/event 类型定义
- `ipc/frontend_channels.rs` — ThreadStreamEvent channel 定义
- `persistence/repo/tool_call_repo.rs`

**Sidecar 项目**（新建 `agent-sidecar/`）：
- `src/main.ts` — 入口，stdio server
- `src/transport/stdio-server.ts` + `protocol.ts` — NDJSON 读写
- `src/runtime/session-registry.ts` + `agent-runner.ts` — pi-agent 会话管理
- `src/providers/provider-registry.ts` + `model-router.ts` — 多 Provider 路由
- `src/tools/tool-registry.ts` + `tool-proxies.ts` — 工具描述注册
- `src/output/event-stream.ts` — 结构化事件发射

**Run 状态机**：Created → Dispatching → Running ⇄ WaitingApproval/WaitingToolResult → Completed/Failed/Cancelled/Interrupted

**前端变更**：
- 新建 `src/services/thread-stream/thread-stream.ts` — Tauri channel 消费者
- PromptInput 提交触发 `thread_start_run`
- 流式渲染 assistant 消息（message_delta → Conversation 组件）

**验收标准**：
- 用户发送 prompt → Sidecar Agent Loop 调用 LLM → 流式响应回显到线程
- Run 状态机转换正确，crash recovery 标记 interrupted
- Sidecar 健康指标可查询（RSS、event_loop_lag、active_run_count）

---

### M1.6 Tool Gateway 与 Policy Engine（1-2 周）

**目标**：实现统一工具执行网关和权限策略引擎。

**Rust 新建文件**：
- `core/tool_gateway.rs` — 请求编排：schema 校验 → PolicyEngine → 审批 → 执行 → 审计
- `core/policy_engine.rs` — 策略评估（deny/allow list、workspace boundary、run_mode 限制、dangerous pattern）
- `core/executors/mod.rs`、`executors/filesystem.rs`（read_file/write_file/list_dir/apply_patch）
- `core/executors/search.rs`（search_repo，ripgrep 封装）
- `core/executors/process.rs`（run_command，非交互一次性执行）
- `persistence/repo/audit_repo.rs` — audit_events 写入

**前端变更**：
- 完善 `components/ai-elements/confirmation.tsx` — 审批 UI（工具名、参数、策略原因）
- 完善 `components/ai-elements/tool.tsx` — 工具执行状态展示

**验收标准**：
- Sidecar 工具请求经 PolicyEngine 裁决后执行
- require-approval 工具弹出确认 UI，用户批准/拒绝后继续
- `rm -rf /`、`sudo` 等危险命令被硬拒绝
- Plan 模式下 mutating 工具被拒绝
- audit_events 表记录所有工具调用

---

### M1.7 前端完整集成（1 周）

**目标**：移除所有 mock 数据和 localStorage fallback，前端完全对接 Rust。

**前端变更**：
- ThreadStreamEvent adapter 层：event → AI Elements 组件映射
  - `plan_updated` → Plan、`tool_*` → Tool、`approval_required` → Confirmation、`message_*` → Conversation
- 错误态映射：`tool_failed` → Tool 错误态 + 系统消息、`run_failed` → 系统消息
- 移除 `fixtures.ts` 中的剩余 mock 数据
- 移除各 controller 中的 localStorage fallback
- 线程切换时正确 unsubscribe/resubscribe stream
- 断线重连后从 Rust 重新拉取 snapshot

**验收标准**：
- 全链路可用：选择工作区 → 发起线程 → AI 流式回复 → 工具执行 → 审批 → 结果回传
- 无 mock 数据残留
- App 重启后完整恢复线程状态

---

### M1.8 Index 基础（1 周）

**目标**：文件树缓存 + ripgrep 文本检索。

**Rust 新建文件**：
- `core/index_manager.rs` — 工作区文件树扫描、内存缓存、ripgrep 子进程封装
- `commands/index.rs` — index_get_tree、index_search

**前端变更**：
- Project Drawer 从 `index_get_tree` 加载，支持文件过滤
- search_repo 工具通过 IndexManager 执行

**验收标准**：
- 中型仓库文件树加载 < 300ms
- ripgrep 搜索返回文件路径 + 行号 + 上下文

---

## Phase 2: 本地能力真实化（~4 周）

### M2.1 Terminal Manager（1-2 周）

**Rust 新建**：`core/terminal_manager.rs`、`commands/terminal.rs`、`core/executors/terminal.rs`

**关键实现**：
- PTY 分配：portable-pty 或 tokio-pty-process
- 1 thread = 0..1 terminal session（partial unique index 保证）
- TerminalStreamEvent channel（stdout_chunk、stderr_chunk、session_exited）
- 切线程不销毁后台 PTY，Rust 保持 ring buffer

**前端**：集成 xterm.js 终端渲染器，消费 TerminalStreamEvent

**验收标准**：终端可交互、Agent 可通过 terminal 工具族操作、切换线程终端不中断

---

### M2.2a Git Read Backend + TreeView（1 周）

**目标**：搭建基于 `git2-rs` 的只读 Git backend，并完成 TreeView 所需的全部 Git-aware 能力。

**Rust 新建/扩展**：`core/git_manager.rs`、`commands/git.rs`、`core/executors/git.rs`

**关键实现**：
- 定义统一 `GitBackend` 抽象，`git2-rs` 作为只读能力实现
- 工作区级 capability 探测：`repo_available`
- TreeView 文件树继续来自 `IndexManager` 或 filesystem scan
- `git2-rs` 为 TreeView 叠加 tracked/untracked/ignored 元数据
- Git ignore 结果可供 TreeView 样式层直接消费

**前端**：
- TreeView 对接真实文件树数据
- TreeView 基于 Git overlay 渲染 `.gitignore` 命中的 ignored 文件样式
- TreeView 区分 tracked/untracked/ignored 状态

**验收标准**：
- TreeView 文件枚举不依赖 Git CLI
- Git 仓库内 ignored/untracked/tracked 文件在 TreeView 中样式区分正确
- 非 Git 工作区下 TreeView 仍可正常工作，仅无 Git overlay

### M2.2b Git Panel Read Experience（1 周）

**目标**：完成 Git Panel 的全部只读能力，统一复用 `git2-rs` backend。

**Rust 新建/扩展**：`core/git_manager.rs`、`commands/git.rs`、`core/executors/git.rs`

**关键实现**：
- `git_get_snapshot`、`git_get_diff`、`git_get_history`、`git_get_file_status`
- 读操作（status/diff/history）优化响应速度
- GitStreamEvent（`refresh_started`、`snapshot_updated`、`refresh_completed`）
- Git Panel 与 TreeView 共享统一 typed Git models，避免状态分裂

**前端**：
- Git Drawer 对接真实只读数据
- 完成 status 分组、Diff 预览、history 浏览
- 根据 `repo_available` 控制 Git Panel 空态与可见性

**验收标准**：
- Git Panel 可展示真实仓库状态、Diff 和 history
- Git 只读能力在未安装 Git CLI 的设备上仍可工作
- TreeView 与 Git Panel 的只读状态保持一致

### M2.2c Git CLI Mutations（1 周）

**目标**：完成依赖本地 Git CLI 的 Git 写操作与远端操作，并在缺失 Git CLI 时优雅降级。

**Rust 新建/扩展**：`core/git_manager.rs`、`commands/git.rs`、`core/executors/git.rs`

**关键实现**：
- 增加工作区级 capability 探测：`git_cli_available`
- `git_commit`、`git_fetch`、`git_pull`、`git_push` 走本地 Git CLI
- CLI-backed 写操作经 PolicyEngine + AuditRecord
- 缺失 Git CLI 时返回结构化错误而非原始 shell stderr

**前端**：
- Commit 表单、Fetch/Pull/Push 操作对接真实命令
- 根据 `git_cli_available` 将 `git_commit`、`git_fetch`、`git_pull`、`git_push` 置为 disabled
- disabled 状态展示清晰安装提示

**验收标准**：
- 已安装 Git CLI 时，commit/fetch/pull/push 可正常执行
- 未安装 Git CLI 时，Git Panel 只读能力仍可用，CLI-backed 操作全部 disabled
- commit/push/pull/fetch 经策略审批，audit 记录完整

---

### M2.3 Index 增强（1 周）

- FTS5 虚拟表 + 触发器同步
- 文件树持久化到 `$HOME/.tiy/cache/index/`
- 后台增量索引

---

## Phase 3: 扩展生态（方向性规划）

### M3.1 MarketplaceHost

- `core/marketplace_host.rs` — 扩展目录管理 + 运行时状态分离
- Skills/MCP/Plugins/Automations 四类 install/enable/disable
- 前端 Marketplace 对接真实数据

### M3.2 MCP 生命周期

- 从 `config.json` 读取 MCP Server 配置
- Rust 宿主管理 MCP 进程（spawn/health/restart）
- 将 MCP 工具暴露给 Sidecar tool registry

### M3.3 Plugin 系统

- `$HOME/.tiy/plugins/` 加载插件
- 插件工具统一经 ToolGateway

### M3.4 Automation Scheduler

- `core/automation_scheduler.rs` — 周期调度
- 复用 thread/run 模型执行自动化任务
- `automation_runs` 表记录执行历史

---

## 关键路径依赖图

```
M1.1 基础设施
 ├── M1.2 Workspace ──── M1.8 Index 基础
 │    └── M1.3 Settings
 │         └── M1.4 Thread
 │              └── M1.5 AgentRun + Sidecar
 │                   └── M1.6 ToolGateway + Policy
 │                        └── M1.7 前端集成
 │                             ├── M2.1 Terminal
 │                             ├── M2.2 Git
 │                             └── M2.3 Index 增强
 │                                  └── M3.x 扩展生态
```

## 并行开发策略

- **前端与 Rust 并行**：前端维护 `isTauri()` 检测 + localStorage fallback，直到对应 Rust 里程碑完成
- **Sidecar 独立开发**：M1.5 之前可用 mock sidecar 测试前端流式渲染
- **M1.2 与 M1.8 可并行**：Index 仅依赖 M1.1，不依赖 M1.3/M1.4
- **M2.1 与 M2.2 可并行**：Terminal 和 Git 互不依赖，仅共享 M1.6 基础

## 验证方式

| 阶段 | 验证方法 |
|------|----------|
| 每个里程碑 | `cargo test` 单元测试 + Rust 集成测试 |
| M1.5 完成后 | 端到端测试：prompt → LLM → streaming response |
| M1.7 完成后 | 全链路验收：工作区 → 线程 → 对话 → 工具调用 → 审批 → 设置 |
| M2.2 完成后 | 本地闭环：对话 + 项目树 + Git + 终端 |
| Phase 3 | Marketplace 扩展安装/启用/MCP 连接验证 |

## 输出文档

实施过程中需同步更新的文档：
- `docs/technical-architecture-20260316.md` — 架构变更同步
- `docs/database-design-20260316.md` — schema 变更记录
- `CLAUDE.md`（待创建）— 项目约定、构建命令、代码规范
