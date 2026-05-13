# Thread 状态逻辑技术文档

> 版本: 1.1 | 日期: 2026-05-13 | 基于当前代码实现校准

---

## 1. 概览

Thread 是本项目的核心对话单元，贯穿前端 UI（侧边栏、Composer、运行时面板）与后端（Rust/Tauri IPC + SQLite 持久化）。本文档完整还原当前 Thread 状态逻辑的现实实现，涵盖：

- 后端数据模型与持久层
- 前端状态管理（threadStore / composerStore / RunLifecycleMachine）
- Thread 生命周期（创建 → 运行 → 暂停/恢复 → 完成/失败 → 删除）
- Composer 状态与 Thread 的关联
- Sidebar 线程列表的状态同步机制
- 崩溃恢复流程

---

## 2. 后端数据模型

### 2.1 ThreadStatus 枚举

**文件**: `src-tauri/src/model/thread.rs`

> **注意**：前端存在同名但含义不同的 `ThreadStatus` 类型（见 §4.1 侧边栏显示状态）。后端 `ThreadStatus` 是 7 值枚举（含 `Archived`），用于数据持久化与 IPC；前端侧边栏 `ThreadStatus` 是 5 值字符串联合类型（含 `needs-reply`，不含 `Idle`/`Archived`），仅用于 UI 显示。两者在 `api.ts` / `types.ts` 中分别定义。

```rust
pub enum ThreadStatus {
    Idle,             // 无活跃 run，可发起新对话
    Running,          // 有正在执行的 run
    WaitingApproval,  // 等待用户审批（plan 模式 / tool approval）
    NeedsReply,       // 等待用户输入（clarify / limit_reached）
    Interrupted,      // run 被中断（崩溃/手动终止）
    Failed,           // run 执行失败
    Archived,         // 已归档（当前未使用）
}
```

### 2.2 ThreadRecord（数据库主表）

当前数据库结构由 `src-tauri/migrations/20260316000001_initial_schema.sql` 加后续 migrations 共同形成；例如 `threads.profile_id` 由 `20260420000000_thread_profile_id.sql` 添加，不属于初始 schema。

| 字段 | 类型 | 说明 |
|---|---|---|
| id | String | UUID v7，时间有序 |
| workspace_id | String | 所属工作区 |
| profile_id | Option\<String\> | 绑定的 Agent Profile |
| title | String | 线程标题 |
| status | ThreadStatus | 当前运行状态 |
| summary | Option\<String\> | 上下文压缩摘要 |
| last_active_at | String | 最后活跃时间（排序用） |
| created_at / updated_at | String | 时间戳 |

### 2.3 DTO 层级

- **ThreadSummaryDto** — 轻量侧边栏 DTO（无 summary，无 messages），用于 `threadList` 接口
- **ThreadSnapshotDto** — 完整快照（含 `thread: ThreadSummaryDto`、messages、has_more_messages、active_run、latest_run、tool_calls、helpers、task_boards、active_task_board_id），用于 `threadLoad` 接口。注意：`active_run` 来自 `run_repo::find_active_by_thread`，会排除终态 run 和 `waiting_approval`，因此它不能代表所有可恢复/可订阅状态。审批态恢复需要结合 `snapshot.thread.status` 与 `latest_run`；`waiting_approval` 既可能来自 plan checkpoint，也可能来自普通 tool approval，其中只有 `RunCheckpointed` 会释放 active runtime session。`RuntimeThreadSurface` 在快照映射为 `running` / `waiting_approval` / `needs_reply` 且当前 stream 无 runId 时会尝试 `stream.subscribe(threadId)`，不是只在 `active_run` 存在时订阅。

### 2.4 RunSummaryDto

当前 `thread_runs` 表的 token usage 字段由 `20260320000100_thread_run_usage.sql` 补充添加；DTO 暴露的是当前迁移后的结构。

| 字段 | 类型 | 说明 |
|---|---|---|
| id | String | Run UUID |
| thread_id | String | 所属线程 |
| run_mode | String | `"default"` / `"plan"`；后端手动 `/compact` 会落库为 `"compact"` |
| status | String | 运行状态字符串 |
| model_id / model_display_name | Option\<String\> | 模型信息 |
| context_window | Option\<String\> | 上下文窗口规格 |
| error_message | Option\<String\> | 失败原因 |
| started_at | String | Run 启动时间 |
| usage | RunUsageDto | Token 用量（input_tokens/output_tokens/cache_read_tokens/cache_write_tokens/total_tokens） |

### 2.5 Run / Message 状态字符串

当前后端只有 `ThreadStatus` 是 Rust 枚举。Run 与 Message 相关状态在 SQLite 中以字符串字段保存，后端 repo 和事件处理逻辑按字符串读写；前端在 `src/shared/types/api.ts` 中用 TypeScript union 约束这些字符串。

- `RunStatus` 前端 union：`created` / `dispatching` / `running` / `waiting_approval` / `needs_reply` / `waiting_tool_result` / `cancelling` / `completed` / `limit_reached` / `failed` / `denied` / `interrupted` / `cancelled`。
- `MessageType` 前端 union：`plain_message` / `plan` / `reasoning` / `tool_request` / `tool_result` / `approval_prompt` / `sources` / `summary_marker`。
- `MessageStatus` 前端 union：`streaming` / `completed` / `failed` / `discarded`。
- 数据库字段来源于 `src-tauri/migrations/20260316000001_initial_schema.sql` 中的 `thread_runs.status`、`messages.message_type`、`messages.status`，后端主要通过 `run_repo.rs`、`message_repo.rs` 和 `agent_run_event_handler.rs` 更新。

---

## 3. 线程状态派生逻辑

### 3.1 derive_thread_status（后端）

**文件**: `src-tauri/src/core/thread_manager.rs`（`derive_thread_status`）

线程状态从其**最新 run** 的状态派生：

| Run Status | Thread Status |
|---|---|
| `created` / `dispatching` / `running` / `waiting_tool_result` / `cancelling` | `Running` |
| `waiting_approval` | `WaitingApproval` |
| `needs_reply` / `limit_reached` | `NeedsReply` |
| `interrupted` | `Interrupted` |
| `failed` / `denied` | `Failed` |
| `completed` / `cancelled`（及所有其他） | `Idle` |

**关键设计**: `limit_reached` → `NeedsReply`，因为用户需要继续对话来恢复上下文。

**例外路径**: 手动 `/compact` 不通过 `derive_thread_status()` 完成最终状态推导。`agent_run_compaction.rs::compact_thread_context()` 会创建 `run_mode = "compact"`、`status = "running"` 的 run，并把线程置为 `Running`；后台压缩结束后 `run_compact_background()` 根据结果把 run 更新为 `completed` / `cancelled` / `failed`，但都会直接把 `threads.status` 重置为 `Idle` 并移除 active run。

### 3.2 状态更新时机

正常流程中线程状态**不通过** `sync_status` 周期性推导，而是由 `agent_run_event_handler.rs` 的 `handle_runtime_event` 在处理**侧边栏可见的生命周期事件**时即时更新 DB + 广播前端，例如 run started、approval/clarify required 或 resolved、checkpoint、completed、failed、cancelled、interrupted、limit reached。工具调用、消息增量、usage 更新等事件通常只更新 run/message/tool 相关表，不一定更新 thread status。`RunRetrying` 会把 `thread_runs.status` 更新为 `running` 并广播 `thread-run-status-changed: running`，但不会直接更新 `threads.status`。手动 `/compact` 由 `agent_run_compaction.rs` 自己写入 run/thread 状态并通过 `ThreadStreamEvent::RunStarted` / `ContextCompressing` / 终态事件驱动前端，不走 `agent_run_event_handler.rs`。`sync_status` 主要用于崩溃恢复或显式状态重算。

---

## 4. 前端状态管理

### 4.1 threadStore（Zustand 风格 store）

**文件**: `src/modules/workbench-shell/model/thread-store.ts`

#### 核心状态

| 字段 | 类型 | 说明 |
|---|---|---|
| `workspaces` | `WorkspaceItem[]` | 工作区列表（含嵌套线程） |
| `threadStatuses` | `Record<threadId, ThreadStatusRecord>` | 实时/乐观 run 状态源 — 供侧边栏指示器和运行时面板消费 |
| `activeThreadId` | `string \| null` | 当前选中的线程 ID |
| `isNewThreadMode` | `boolean` | 是否处于"新线程"模式 |
| `pendingRuns` | `Record<threadId, PendingThreadRun>` | 提交但尚未 startRun 的待执行 run |
| `displayCounts` / `hasMore` / `loadMorePending` | `Record<wsId, number/boolean>` | 侧边栏分页控制 |
| `openWorkspaces` | `Record<wsId, boolean>` | 工作区展开/折叠 |
| `sidebarReady` | `boolean` | 初始同步是否完成 |
| `activeThreadProfileIdOverride` | `string \| null` | 线程级 Profile 覆盖 |
| `runtimeContextUsage` | `ThreadContextUsage \| null` | 实时 Token 用量 |
| `editingThreadId` | `string \| null` | 侧边栏正在内联重命名的线程 |
| `defaultWorkspaceId` | `string \| null` | 默认工作区 ID |

#### ThreadStatusRecord

```typescript
interface ThreadStatusRecord {
  status: ThreadRunStatus;    // 9 种状态之一
  runId: string | null;       // 关联的 run ID
  updatedAt: number;          // Date.now()，乱序防护用
  source: ThreadStatusSource; // "stream" | "tauri_event" | "snapshot" | "optimistic"
}
```

#### ThreadRunStatus（前端 9 种状态）

```typescript
type ThreadRunStatus =
  | "idle" | "running" | "waiting_approval" | "needs_reply"
  | "completed" | "failed" | "cancelled" | "interrupted" | "limit_reached";
```

#### 侧边栏显示映射

`threadRunStatusToDisplayStatus()` 将 9 种内部状态映射为 5 种侧边栏显示状态：

| ThreadRunStatus | 侧边栏 ThreadStatus |
|---|---|
| `idle` / `completed` / `cancelled` | `completed` |
| `waiting_approval` / `needs_reply` / `limit_reached` | `needs-reply` |
| `running` | `running` |
| `failed` | `failed` |
| `interrupted` | `interrupted` |

### 4.2 setThreadStatus 保护机制

**文件**: `src/modules/workbench-shell/model/thread-store.ts`（`setThreadStatus` / `batchSetThreadStatuses`）

前端有四类写入入口：stream 状态机订阅、Tauri 全局事件 fallback、快照恢复、乐观提交。实际 `ThreadStatusSource` 包含 `"stream" | "tauri_event" | "snapshot" | "optimistic"`；但当前快照恢复不是直接以 `source: "snapshot"` 调用 `setThreadStatus`，而是通过 `runMachine.reset()` 触发机器订阅回调，再以 `source: "stream"` 写入 store。`setThreadStatus` 有两个守卫：

#### Guard 1：乱序保护（同 runId 内的过时写入）

```
条件：existing.runId !== null
   ∧ meta.runId 不为 undefined/null
   ∧ existing.runId === meta.runId（同一 run）
   ∧ meta.updatedAt 存在
   ∧ existing.updatedAt > meta.updatedAt（传入时间戳更旧）
结果：静默忽略
```

- 仅当调用方显式提供 `updatedAt` 时才比较
- **runId 变更时始终放行**（新 run 开始是合法覆盖）

#### Guard 2：活跃状态降级保护

```
条件：existing.runId !== null
   ∧ existing.status ∈ {running, waiting_approval, needs_reply}
   ∧ 新 status === "idle"
   ∧ meta.runId 为 undefined/null
结果：拒绝
```

- 场景：快照恢复调用 `runMachine.reset("idle", { runId: null })` 时，不应覆盖由流事件或全局事件写入的带真实 runId 的活跃状态
- `batchSetThreadStatuses` 中有完全相同的两个 guard

#### runId 写入语义

`setThreadStatus` / `batchSetThreadStatuses` 当前使用 `meta.runId ?? existing?.runId ?? null` 写入 `runId`，因此传入 `runId: null` 不会主动清空已有 `runId`；只有没有既有 `runId` 时结果才为 `null`。这意味着终态或 `idle` 写入如果未携带新的 runId，会沿用该线程现有的 runId 记录。

---

## 5. RunLifecycleMachine 状态机

**文件**: `src/modules/workbench-shell/model/run-lifecycle-machine.ts`

每个 `RuntimeThreadSurface` 挂载时创建一个独立的状态机实例，卸载时销毁。

### 5.1 完整状态转换表

| 当前状态 | 事件 | 目标状态 | Context 更新 |
|---|---|---|---|
| `idle` | `RUN_STARTED` | `running` | runId=payload.runId, retryCount=0, errorMessage=null |
| `running` | `APPROVAL_REQUIRED` | `waiting_approval` | — |
| `running` | `CLARIFY_REQUIRED` | `needs_reply` | — |
| `running` | `RUN_RETRYING` | `running` | runId=payload.newRunId, retryCount+1 |
| `running` | `RUN_COMPLETED` | `completed` | — |
| `running` | `RUN_FAILED` | `failed` | errorMessage=payload.message |
| `running` | `RUN_CANCELLED` | `cancelled` | — |
| `running` | `RUN_INTERRUPTED` | `interrupted` | — |
| `running` | `LIMIT_REACHED` | `limit_reached` | — |
| `waiting_approval` | `RUN_STARTED` | `running` | runId, retryCount=0, errorMessage=null |
| `waiting_approval` | `APPROVAL_RESOLVED` | `running` | — |
| `waiting_approval` | `RUN_COMPLETED` / `RUN_FAILED` / `RUN_CANCELLED` / `RUN_INTERRUPTED` / `LIMIT_REACHED` | 对应终态（`RUN_FAILED` 设置 errorMessage） | — |
| `needs_reply` | `RUN_STARTED` | `running` | runId, retryCount=0, errorMessage=null |
| `needs_reply` | `CLARIFY_RESOLVED` | `running` | — |
| `needs_reply` | `RUN_COMPLETED` / `RUN_CANCELLED` / `RUN_INTERRUPTED` / `LIMIT_REACHED` | 对应终态 | — |
| `needs_reply` | `RUN_FAILED` | `failed` | errorMessage=payload.message |
| `completed` / `failed` / `cancelled` / `interrupted` / `limit_reached` | `RUN_STARTED` | `running` | runId, retryCount=0, errorMessage=null |

### 5.2 自动同步

机器在 `subscribe` 回调中把每次状态或 context 变化同步到 store；`runMachine.reset()` 也会触发该订阅回调：

```typescript
setThreadStatus(threadId, currentState, { runId, source: "stream" });
```

### 5.3 流事件映射

`mapStreamEventToMachineEvent()` 将 ThreadStream 事件映射为机器事件：

| 流事件 | 机器事件 |
|---|---|
| `run_started` | `RUN_STARTED` |
| `approval_required` / `run_checkpointed` | `APPROVAL_REQUIRED`（plan checkpoint ≡ approval） |
| `clarify_required` | `CLARIFY_REQUIRED` |
| `approval_resolved` | `APPROVAL_RESOLVED` |
| `clarify_resolved` | `CLARIFY_RESOLVED` |
| `run_retrying` | `RUN_RETRYING` |
| `run_completed` | `RUN_COMPLETED` |
| `run_failed` | `RUN_FAILED` |
| `run_cancelled` | `RUN_CANCELLED` |
| `run_interrupted` | `RUN_INTERRUPTED` |
| `run_limit_reached` | `LIMIT_REACHED` |

`run_checkpointed` 与 `approval_required` 都映射为 `APPROVAL_REQUIRED`。后端会把 plan run 停在 `waiting_approval`，并将 `RunCheckpointed` 视为 terminal runtime event，移除 active runtime session；后续 `executeApprovedPlan()` 会启动新的 implementation run。

**注意**: `ContextCompressing` 不是 RunLifecycleMachine 状态事件；它只用于运行面板把占位文案切换为“Compressing context…”。手动 `/compact` 仍依赖 `run_started` 进入 `running`，并依赖 `run_completed` / `run_failed` / `run_cancelled` 进入终态。

---

## 6. RunEventDispatcher（全局事件路由）

**文件**: `src/modules/workbench-shell/model/run-event-dispatcher.ts`

### 6.1 机器注册表

```typescript
const activeMachines = Map<threadId, Machine>();
```

- `RuntimeThreadSurface` 挂载时 `registerRunMachine(threadId, machine)`
- 卸载时 `unregisterRunMachine(threadId)`

### 6.2 路由逻辑

| 方法 | 有注册机器 | 无注册机器（后台线程） |
|---|---|---|
| `dispatchGlobalEvent` | `machine.send(event, payload)` | `setThreadStatus(threadId, status, { source: "tauri_event" })` |
| `dispatchRunFinishedEvent` | 映射 backendStatus → 机器事件 → `machine.send()` | `setThreadStatus(threadId, mappedStatus, { source: "tauri_event" })` |
| `dispatchRunStatusChangedEvent` | 映射 backendStatus → 机器事件 → `machine.send()` | `setThreadStatus(threadId, mappedStatus, { source: "tauri_event" })` |

**关键设计**：无 RuntimeThreadSurface 的后台线程，其状态通过直接写 threadStore 保持侧边栏同步。

### 6.3 后端状态字符串映射

| 后端 Status | 机器事件 | ThreadRunStatus |
|---|---|---|
| `running` | `RUN_STARTED` | `running` |
| `waiting_approval` | `APPROVAL_REQUIRED` | `waiting_approval` |
| `needs_reply` | `CLARIFY_REQUIRED` | `needs_reply` |
| `completed` | `RUN_COMPLETED` | `completed` |
| `failed` | `RUN_FAILED` | `failed` |
| `cancelled` | `RUN_CANCELLED` | `cancelled` |
| `interrupted` | `RUN_INTERRUPTED` | `interrupted` |
| `limit_reached` | `LIMIT_REACHED` | `limit_reached` |

---

## 7. Thread 生命周期流程

### 7.1 创建流程

```
用户输入 prompt → Composer submit
  → submitNewThread()（`src/modules/workbench-shell/model/workbench-actions.ts`）
    ├─ 1. 确定项目/工作区（workspaceEnsureDefault）
    ├─ 2. 确保后端工作区存在（workspaceList → findWorkspaceByPath → workspaceAdd）
    ├─ 3. getOrCreateNewThreadId(workspaceId)
    │     ├─ 检查 terminalThreadBindings 缓存
    │     ├─ 检查 pendingCreations 去重 Map
    │     └─ threadCreate(workspaceId, "", activeAgentProfileId) IPC
    ├─ 4. 构建乐观线程条目（buildThreadTitle 截取前30字符）
    ├─ 5. 写入 stores:
    │     ├─ threadStore.workspaces（插入新线程）
    │     ├─ threadStore.pendingRuns[threadId] = submission
    │     ├─ setThreadStatus(threadId, "running", { runId: pendingRunId, source: "optimistic" })
    │     ├─ activeThreadId = threadId
    │     └─ isNewThreadMode = false
    ├─ 6. 清除新线程终端绑定（__new_thread__ 键）
    └─ 7. 清空 newThreadValue / newThreadRunMode / error

RuntimeThreadSurface 挂载后:
  → 检测 pendingRuns[threadId]
  → 快照就绪 + 无阻塞 run + 未处理过此 ID
  → 自动调用 stream.startRun()
  → `.finally()` 按 `PendingThreadRun.id` 从 `pendingRuns` 中过滤删除对应 pending run（`pendingRuns` 存储仍以 `threadId` 为 key）
```

### 7.2 选择/切换线程流程

```
用户点击侧边栏线程
  → selectThread(threadId)（`src/modules/workbench-shell/model/workbench-actions.ts`）
    ├─ 1. 扁平查找线程对象
    ├─ 2. 解析 profileId（resolveThreadProfileId）
    ├─ 3. 清除新线程终端绑定
    ├─ 4. threadStore 更新:
    │     ├─ isNewThreadMode = false
    │     ├─ activeThreadId = threadId
    │     ├─ activeThreadProfileIdOverride = profileId
    │     └─ editingThreadId = null
    ├─ 5. activateThread() — 标记活动线程
    └─ 6. 对齐 selectedProject 到线程所属工作区

RuntimeThreadSurface 挂载:
  → threadId 变化触发 loadSnapshot()
    → threadLoad(threadId) IPC 加载 ThreadSnapshotDto
    → mapSnapshotToRunState() 结合 snapshot.active_run、snapshot.thread.status、latest_run 恢复状态
    → 如果恢复出的状态是 running / waiting_approval / needs_reply，且当前 stream 无 runId，则尝试 stream.subscribe(threadId)
```

**懒加载设计**: 切换线程时通过 `threadLoad` IPC 按需加载完整快照，不在内存中常驻所有线程数据。`waiting_approval` 不属于 `active_run`，审批态恢复依赖 `thread.status` + `latest_run`；其中只有 plan checkpoint 对应的 `RunCheckpointed` 会释放 active runtime session。`mapSnapshotToRunState()` 有两层映射：有 `activeRun` 时直接按 run status 映射，其中 `cancelling` 映射为 `cancelled`、`waiting_tool_result` 仍映射为 `running`；无 `activeRun` 时按 `snapshot.thread.status` 映射，`needs_reply` 且 `latestRun.status === "limit_reached"` 会恢复为 `limit_reached`，后端 `idle` / `archived` / 其他状态在前端运行面板中显示为 `completed`。

### 7.3 删除流程

**前端实际路径**:

当前 Dashboard 删除入口调用 `workbench-actions.deleteThread`，不是两层串联执行：
- 若未传 `skipIpc` 且处于 Tauri 环境，先调用 `threadDelete(threadId)` IPC
- IPC 成功后清理 terminal session
- 从 `workspaces` 中移除线程
- 清理 `pendingRuns`、`terminalCollapsedByThreadKey`、`terminalThreadBindings`
- 当前路径不会删除 `threadStatuses[threadId]`；被删线程已经从 `workspaces` 移除，因此侧边栏不可见，但该实时状态记录可能在内存中残留到 store 重建
- 如果删除的是活动线程 → `{ activeThreadId: null, isNewThreadMode: true, activeThreadProfileIdOverride: null }`，并清除 composer error

`threadStore.deleteThread` 另有一个独立的乐观/去重 helper：它使用 `syncToBackend`、`dedupe: { key: 'thread-delete:${threadId}', strategy: 'first' }`，失败时可 rollback，并会从 `threadStatuses` 中删除记录。但当前 Dashboard 删除路径未使用该 helper，因此文档不要把它描述为 UI 删除流程的第一层。

**后端命令层** (`thread_delete` IPC):
- 删除前尝试取消活跃 run
- 最多等待 5s 直到 run 变为 inactive
- 关闭该线程关联的 terminal session
- 调用 `thread_repo::delete` 执行持久化删除

**后端持久层** (`thread_repo::delete`):
- 开启 SQLite 事务，按依赖顺序级联删除 10 张关联表，最后删除 `threads` 主表：
  1. audit_events → 2. tool_calls → 3. run_subtasks → 4. run_helpers → 5. messages → 6. thread_summaries → 7. terminal_sessions → 8. thread_runs → 9. task_items（通过 task_boards） → 10. task_boards → 11. threads（主表）

---

## 8. isNewThreadMode 逻辑

### 8.1 设置为 `true` 的时机

| 触发点 | 场景 |
|---|---|
| 初始默认值 | 应用启动 |
| `setActiveThread(null)` | 无线程选中时 `isNewThread ?? (threadId === null)` 为 true |
| `enterNewThreadMode()` | 用户点击"新对话" |
| `activateWorkspace()` | 切换工作区 |
| `deleteThread()` 删除活动线程 | 删除后回到新线程模式 |
| `removeWorkspace()` 移除活动工作区 | 移除的 workspace 包含当前活动线程时，进入新线程模式并清除 workspace/thread 的 active 标记；当前 orchestration 路径不会立即清空 `activeThreadId`、`activeThreadProfileIdOverride` 或 `threadStatuses` |

### 8.2 设置为 `false` 的时机

| 触发点 | 场景 |
|---|---|
| `selectThread()` | 用户在侧边栏选中一个线程 |
| `submitNewThread()` | 提交新线程后立即切换到该线程 |

### 8.3 影响范围

| 消费方 | `true` 时的行为 | `false` 时的行为 |
|---|---|---|
| **dashboard-workbench** | 渲染 `NewThreadEmptyState` + 新线程 composer | 渲染 `RuntimeThreadSurface` + 线程标题栏 |
| **sidebar** | "New Thread" 按钮高亮 | 活动线程高亮 |
| **Profile 解析** | 使用全局 `activeAgentProfileId` | 使用线程级 override |
| **Terminal 绑定** | 解析 `__new_thread__` 绑定键 | 使用活动线程 ID |
| **Profile 切换** | `setActiveAgentProfile()` 修改全局默认 | `threadUpdateProfile()` 修改线程级 |
| **Composer 提交** | 走 `submitNewThread` | 交给 `RuntimeThreadSurface` |
| **Terminal 预热** | 触发 | 不触发 |

---

## 9. Composer 状态管理

**文件**: `src/modules/workbench-shell/model/composer-store.ts`

### 9.1 双轨设计

| 模式 | 状态字段 | 说明 |
|---|---|---|
| **新线程** | `newThreadValue` / `newThreadRunMode` / `newThreadReferencedFiles` / `newThreadAttachmentData` | 全局单一实例 |
| **已有线程** | `drafts: Record<threadId, ComposerDraftData>` | 按线程 ID 键值存储 |

### 9.2 ComposerDraftData

```typescript
interface ComposerDraftData {
  text: string;                           // 输入文本
  referencedFiles: ComposerReferencedFile[]; // @file 引用
}
```

### 9.3 Draft 生命周期

| 时机 | 操作 |
|---|---|
| **保存** — 每次输入变化 | `setDraft(threadId, { ...existing, text: value })` |
| **保存** — 文件引用变更 | `setDraft(threadId, { ...existing, referencedFiles: files })` |
| **加载** — 切换到线程 | `getDraft(threadId)` 自动恢复 |
| **清除** — `submitNewThread()` | `composerStore.setState({ newThreadValue: "", newThreadRunMode: "default", error: null })` |
| **清除** — `clearNewThreadComposer()` | `newThreadValue: ""` + `newThreadReferencedFiles: []` + `newThreadAttachmentData: []` + `error: null`（保留 `newThreadRunMode`） |
| **不清除** — 切换线程 | 保留 draft，切回时自动恢复 |

**向后兼容**: `getDraft()` 处理旧版纯字符串 draft，自动包装为 `{text, referencedFiles:[]}`。

### 9.4 RunMode

```typescript
// src/shared/types/api.ts 当前前端类型
export type RunMode = "default" | "plan";
```

| 模式 | 工具配置 | 行为 |
|---|---|---|
| `default` | 默认 `"default_full"` — 允许所有工具 | 直接执行 |
| `plan` | 默认 `"plan_read_only"` — 只允许只读工具 | 先产出计划，等待用户审批后执行第二阶段 |

`default` / `plan` 是 Composer 可选择并由 `PendingThreadRun.runMode` 承载的交互模式。后端解析工具配置时会优先使用 `modelPlan.toolProfileByMode[runMode]` 中的显式配置；只有缺省时才 fallback 到 `default_full` / `plan_read_only`。

**实现差异**: 后端 `RunSummaryDto.run_mode` 是普通 `String`，手动 `/compact` 会在 `agent_run_compaction.rs` 中插入 `run_mode = "compact"`，并通过 `ThreadStreamEvent::RunStarted { run_mode: "compact" }` 通知前端；当前 `src/shared/types/api.ts` 的 `RunMode` union 尚未覆盖 `"compact"`。因此文档中凡描述 API/数据库 run_mode 时应按字符串处理，凡描述 Composer/PendingRun 时仍是 `"default" | "plan"`。

**Plan 模式流程**: Run 以 `plan` 启动 → Agent 产出计划 → `waiting_approval` → 用户选择 `apply_plan` 或 `apply_plan_with_context_reset` → `stream.executeApprovedPlan()` → 后端启动新的 implementation run，并把原 planning run 从 `waiting_approval` 更新为 `completed`。如果用户未审批而直接启动新 run，`expire_pending_plan_approval()` 会把待审批计划标记为 superseded，并取消该线程中停在 `waiting_approval` 的 run。

**手动 `/compact` 流程**: `thread_compact_context` IPC 调用 `AgentRunManager::compact_thread_context()`；若当前线程有 active run，会先尝试取消。随后后端同步持久化 `/compact` 用户命令消息和 `context_reset` summary marker，创建 `run_mode = "compact"` / `status = "running"` 的 run，把 thread 置为 `Running`，并发送 `RunStarted` + `ContextCompressing`。后台 LLM 摘要完成后写入 `context_summary` marker，成功发送 `RunCompleted`，取消发送 `RunCancelled`，失败则尽量写入 `compact_fallback` 摘要后发送 `RunFailed`；三种结果都会更新 run 终态、把 thread 重置为 `Idle` 并移除 active run。

---

## 10. pendingRuns 机制

### 10.1 问题

新线程创建和 `RuntimeThreadSurface` 挂载之间存在时间窗口，需要将 prompt 从 `submitNewThread` 传递给后挂载的 surface 组件。

### 10.2 解决方案

```typescript
type PendingThreadRun = {
  id: string;                    // 临时唯一 ID
  displayText: string;           // 用户看到的文本
  effectivePrompt: string;       // 实际发送的 prompt
  attachments: MessageAttachmentDto[];
  metadata: Record<string, unknown> | null;
  runMode: RunMode;
  threadId: string;
};
```

### 10.3 流程

1. `submitNewThread()` → 写入 `threadStore.pendingRuns[threadId]`
2. `RuntimeThreadSurface` 挂载 → 检测 `pendingRuns[threadId]`
3. 条件满足（快照就绪 + 无阻塞 run + 未处理过此 ID）→ 自动调用 `submitPrompt`
4. `submitPrompt` 完成 → `.finally()` 按 `PendingThreadRun.id` 从 `pendingRuns` 中过滤删除对应 pending run（`pendingRuns` 存储仍以 `threadId` 为 key）
5. `deleteThread` / `removeWorkspace` 中清理相关 `pendingRuns`

---

## 11. Sidebar 状态同步

### 11.1 数据结构

```typescript
type WorkspaceItem = {
  id: string;
  name: string;
  defaultOpen: boolean;
  threads: Array<WorkspaceThreadItem>;
  path?: string;
  kind?: "standalone" | "repo" | "worktree";
  parentWorkspaceId?: string | null;
  worktreeHash?: string | null;
  branch?: string | null;
  createdAt?: string;
};

type WorkspaceThreadItem = {
  id: string;
  profileId?: string | null;
  name: string;
  time: string;
  active: boolean;
  status: ThreadStatus;  // 5 种侧边栏显示状态
};
```

### 11.2 同步机制

**三条通道并行**:

| 通道 | 机制 | 频率 |
|---|---|---|
| **Tauri 全局事件** | `useGlobalThreadEvents` 监听 4 种事件，并通过 dispatcher 写入 `threadStatuses` 或活动机器 | 实时 |
| **自动轮询** | `use-sidebar-auto-poll.ts` 根据 `WorkspaceThreadItem.status` 判断是否轮询 | 有 running/needs-reply 线程或处于事件宽限期时 2s 间隔 |
| **初始/列表同步** | `performSidebarSync()` 调用 `workspaceList` + `threadList` 刷新 `workspaces` | 应用启动 + 工作区变更时 |

`threadStatuses` 是实时/乐观 run 状态源，侧边栏指示器当前按 `threadStatuses[threadId]?.status ?? "idle"` 渲染；`performSidebarSync()` 只刷新 `WorkspaceThreadItem.status` 快照，不会批量灌入 `threadStatuses`。自动轮询是否启动则读取 `WorkspaceThreadItem.status` 中的 `running` / `needs-reply`。

### 11.3 全局事件监听

`useGlobalThreadEvents` 监听 4 种 Tauri 全局事件：

| 事件 | 处理 |
|---|---|
| `thread-run-started` | `dispatchGlobalEvent(threadId, "RUN_STARTED")` + 延长自动刷新 20s |
| `thread-run-finished` | `dispatchRunFinishedEvent()` + `syncWorkspaceSidebar()` |
| `thread-run-status-changed` | `dispatchRunStatusChangedEvent()` + 延长自动刷新 20s |
| `thread-title-updated` | `setStoreThreadTitle()`（跳过正在编辑的线程） |

此外，`RuntimeThreadSurface` 自身还单独监听 `thread-run-finished` 作为兜底：如果当前线程的 `threadStatuses` 仍是 `running` / `waiting_approval` / `needs_reply`，则触发 `loadSnapshot()`，避免 per-stream 回调漏掉终态事件后运行面板卡在活跃状态。

### 11.4 SidebarAutoPoll

- 当 `WorkspaceThreadItem.status` 中存在 running / needs-reply 线程，或当前仍处于全局事件延长出的宽限期时启动定时器
- 间隔 2s，调用 `performSidebarSync()`
- 与 `useGlobalThreadEvents` 共享 `sidebarAutoRefreshUntilRef` 引用
- 事件到达后延长 20s 宽限期，避免事件和轮询同时消失

### 11.5 SidebarSyncRunner

`CoalescedAsyncRunner` 提供：
- **合并并发请求** — 同一 runner 实例内的并发/排队请求会合并 options
- **单飞行 + trailing run** — 同一时间只有一个 IPC 调用在飞行，飞行期间新增请求合并为一次后续执行
- **最小间隔节流** — 300ms，防止 effect 反馈循环导致 IPC 队列饱和

### 11.6 分页

- `WORKSPACE_THREAD_PAGE_SIZE = 10`
- 每个工作区独立分页（`displayCounts` / `hasMore` / `loadMorePending`）
- "Show More" 按钮加载下一页

### 11.7 排序

`sortWorkspacesWithWorktrees()`:
1. 默认工作区优先（`defaultOpen`）
2. 名称升序（locale-aware）
3. repo/standalone 排在 worktree 前
4. 创建时间降序（最新优先）
5. ID tiebreaker — 确保完全确定性排序
6. worktree 归组紧跟其父 repo（`sortWorkspacesWithWorktrees` 第二阶段）

---

## 12. 崩溃恢复流程

### 12.1 后端（应用启动时）

**文件**: `src-tauri/src/core/thread_manager.rs`（`recover_interrupted_runs`）

`recover_interrupted_runs()` 在 `tauri::Builder::setup` 中异步执行：

1. 收集所有有活跃 run 的线程 ID（`run_repo::list_thread_ids_with_active_runs` + `thread_repo::list_ids_with_active_status`）
2. `run_repo::interrupt_active_runs()` — 将非终态且非 `waiting_approval` 的 dangling run 状态改为 `interrupted`；`waiting_approval` 线程只参与后续 `sync_status()` 重算
3. `tool_call_repo::interrupt_active_tool_calls()` — 将活跃工具调用标记为中断
4. `run_helper_repo::interrupt_active_helpers()` — 将活跃 helper 标记为中断
5. `message_repo::discard_dangling_reasoning()` — 丢弃未完成的 reasoning 消息
6. 对每个受影响线程调用 `sync_status()` — `derive_thread_status()` 重新推导线程状态

### 12.2 前端

`RuntimeThreadSurface` 挂载时：
- `threadLoad()` 获取完整快照
- `mapSnapshotToRunState()` 先按 `snapshot.activeRun` 映射；无 active run 时再结合 `snapshot.thread.status` 与 `latest_run` 映射快照状态 → `RunState`，其中后端 `idle` / `archived` / 其他状态在前端显示为 `completed`
- `runMachine.reset()` 恢复机器状态，并通过机器订阅回调同步 `threadStore.threadStatuses`
- 中断线程优先显示 run.errorMessage；崩溃恢复写入默认错误：`The app closed or the run was terminated before completion. Restarted in interrupted state.`；若无 errorMessage，前端 fallback 为 `The app closed or the run was terminated before completion. This thread was restored as interrupted.`

---

## 13. Thread 标题生成

### 13.1 两层机制

**前端乐观标题** — `buildThreadTitle(prompt)`: 截取 prompt 前 30 字符 + "..."，在 `submitNewThread` 中立即设置。

**后端 AI 生成** — `maybe_generate_thread_title()`:
1. 检查 `has_title()` — 已有标题则跳过
2. 加载最近 reset 边界后的 user/assistant 消息
3. 按优先级构建候选模型（lightweight → auxiliary → primary 去重）
4. 依次尝试 LLM 生成标题（规则：最多 18 个中文字 / 7 个英文词）
5. `normalize_generated_title()` — 去除前缀/引号/括号/尾部标点，截断 40 字符
6. 成功后 `update_title()` + 发送两个事件：
   - `ThreadStreamEvent::ThreadTitleUpdated` — 流内事件
   - `app_handle.emit("thread-title-updated")` — 全局 Tauri 事件

### 13.2 手动重新生成

`thread_regenerate_title` 命令：用户触发，限制最近 128 条消息中最后 24 条，逻辑与自动生成相同但不受 `has_title` 限制。该命令只返回候选标题，不会直接调用 `update_title()` 持久化，也不会像自动生成那样直接广播 `thread-title-updated`；前端会把候选标题填入重命名输入框，用户确认保存后才通过标题更新路径持久化。

### 13.3 防冲突

全局事件处理中检查 `editingThreadId` — 用户正在内联重命名时跳过自动标题更新。

---

## 14. 数据流全景图

```
┌─────────────────────────────────────────────────────────────────┐
│                        后端 (Rust/Tauri)                         │
│                                                                   │
│  ThreadManager ── ThreadRepo (SQLite)                            │
│       │                                                           │
│  AgentSession ── AgentRunEventHandler                            │
│       │              │                                            │
│       │         handle_runtime_event()                            │
│       │              ├─ 对侧边栏可见生命周期事件更新 DB           │
│       │              │  (run_status + thread_status)              │
│       │              ├─ emit Tauri 全局事件 ×3                    │
│       │              │  (thread-run-started / finished /          │
│       │              │   status-changed)                          │
│       │              └─ maybe_generate_thread_title()            │
│       │                                                           │
│  ThreadStream ── ThreadStreamEvent → 前端 IPC callback/channel     │
└─────────────────────────────────────────────────────────────────┘
                            │
                ┌───────────┴───────────┐
                ▼                       ▼
┌──────────────────────┐  ┌──────────────────────────────────┐
│  Tauri 全局事件通道  │  │  ThreadStream 通道              │
│  (listen)            │  │  (invoke + callback/broadcast)  │
└──────────┬───────────┘  └──────────────┬───────────────────┘
           │                             │
           ▼                             ▼
┌──────────────────────┐  ┌──────────────────────────────────┐
│ useGlobalThreadEvents│  │ ThreadStream                     │
│   ├─ thread-run-     │  │   ├─ startRun()                  │
│   │  started         │  │   ├─ subscribe() callbacks       │
│   ├─ thread-run-     │  │   ├─ cancelRun()                 │
│   │  finished        │  │   ├─ executeApprovedPlan()        │
│   ├─ thread-run-     │  │   └─ ...                         │
│   │  status-changed  │  │                                   │
│   └─ thread-title-  │  │                                   │
│      updated         │  │                                   │
└──────────┬───────────┘  └──────────────┬───────────────────┘
           │                             │
           ▼                             ▼
┌──────────────────────────────────────────────────────────────┐
│                 RunEventDispatcher                             │
│  ┌────────────────────────────────────────────────────────┐   │
│  │ activeMachines: Map<threadId, Machine>                  │   │
│  │                                                         │   │
│  │ dispatchGlobalEvent():                                  │   │
│  │   有 machine → machine.send(event)                     │   │
│  │   无 machine → setThreadStatus() 直接写                 │   │
│  │                                                         │   │
│  │ dispatchRunFinishedEvent():                             │   │
│  │   有 machine → mapBackendStatus → machine.send()       │   │
│  │   无 machine → setThreadStatus()                       │   │
│  │                                                         │   │
│  │ dispatchRunStatusChangedEvent():                        │   │
│  │   有 machine → mapBackendStatus → machine.send()       │   │
│  │   无 machine → setThreadStatus()                       │   │
│  └────────────────────────────────────────────────────────┘   │
└──────────────────────────┬───────────────────────────────────┘
                           │
          ┌────────────────┼────────────────┐
          ▼                ▼                ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────────┐
│  Machine     │  │  threadStore │  │  SidebarAutoPoll  │
│  subscribe   │──▶│              │◀──│  (2s 定时 IPC)   │
│  callback    │  │ threadStatuses│  │                  │
└──────────────┘  │ activeThreadId│  └──────────────────┘
                  │ isNewThreadMode│
                  │ workspaces    │
                  │ pendingRuns   │
                  └───────┬───────┘
                          │
           ┌──────────────┼──────────────┐
           ▼              ▼              ▼
    ┌────────────┐  ┌──────────┐  ┌──────────────┐
    │  Sidebar   │  │ Composer │  │ Runtime      │
    │  列表渲染  │  │ draft    │  │ Thread       │
    │  状态指示器│  │ 管理     │  │ Surface      │
    └────────────┘  └──────────┘  └──────────────┘
```

---

## 15. 关键文件索引

| 文件路径 | 职责 |
|---|---|
| `src/modules/workbench-shell/model/thread-store.ts` | threadStore — 实时/乐观线程状态与 workbench 线程列表状态 |
| `src/modules/workbench-shell/model/run-lifecycle-machine.ts` | RunLifecycleMachine — per-thread 状态机 |
| `src/modules/workbench-shell/model/run-event-dispatcher.ts` | RunEventDispatcher — 全局事件路由 |
| `src/modules/workbench-shell/model/composer-store.ts` | composerStore — Composer 双轨状态 |
| `src/modules/workbench-shell/model/types.ts` | ThreadRunStatus / ThreadStatus / WorkspaceItem 类型 |
| `src/modules/workbench-shell/ui/dashboard-sidebar.tsx` | 侧边栏渲染 |
| `src/modules/workbench-shell/hooks/use-global-thread-events.ts` | Tauri 全局事件监听 |
| `src/modules/workbench-shell/hooks/use-sidebar-auto-poll.ts` | 侧边栏自动轮询 |
| `src/modules/workbench-shell/model/sidebar-sync-runner.ts` | 侧边栏同步节流/合并 |
| `src/modules/workbench-shell/ui/dashboard-workbench-logic.ts` | Workbench 纯 helper/constants/PendingThreadRun 类型 |
| `src/modules/workbench-shell/model/workbench-actions.ts` | select/create/delete/sync 等跨 store orchestration actions |
| `src/services/thread-stream/thread-stream.ts` | ThreadStream — 实时事件适配 |
| `src/services/bridge/thread-commands.ts` | IPC 命令桥接 |
| `src/shared/types/api.ts` | 前端 API DTO 与 Run/Message/Thread 状态 union 类型 |
| `src-tauri/src/model/thread.rs` | ThreadRecord / ThreadStatus / ThreadSnapshotDto |
| `src-tauri/src/core/thread_manager.rs` | ThreadManager — create / load / sync_status / recover |
| `src-tauri/src/core/agent_run_event_handler.rs` | handle_runtime_event — 运行时事件处理 |
| `src-tauri/src/core/agent_run_compaction.rs` | 手动 `/compact` 上下文压缩 — 创建 `compact` run、发送压缩流事件、完成后重置 thread 为 Idle |
| `src-tauri/src/ipc/frontend_channels.rs` | ThreadStreamEvent — Rust 到前端的流事件定义与通道 |
| `src-tauri/src/persistence/repo/thread_repo.rs` | 线程 SQLite CRUD 与删除级联 |
| `src-tauri/src/persistence/repo/run_repo.rs` | Run SQLite CRUD、active/latest 查询与状态更新 |
| `src-tauri/src/persistence/repo/message_repo.rs` | Message SQLite CRUD 与 dangling message 恢复处理 |
| `src-tauri/src/commands/thread.rs` | Tauri IPC 命令 |
| `src-tauri/migrations/20260316000001_initial_schema.sql` | 初始 SQLite schema，定义基础 thread/run/message 表与状态字段；当前完整 schema 还依赖后续 migrations（例如 `threads.profile_id`、run usage、`messages.parts_json`） |
