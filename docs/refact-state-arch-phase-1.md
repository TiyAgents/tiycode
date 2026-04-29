# Phase 1：threadStore — 统一线程状态事实源

## Summary

创建 `threadStore` 作为线程与工作区状态的唯一事实源（Single Source of Truth），消除当前线程状态在 Sidebar（`workspaces[].threads[].status`）、Surface（`runState`）、Workbench（`pendingThreadRuns`）三处独立维护导致的不一致问题。这是整个重构中收益最高的单步改进，直接解决最严重的架构痛点。

## Context

### 前置依赖

本阶段依赖 Phase 0 产出的 `createStore` 工厂函数（`src/shared/lib/create-store.ts`）。

### 三源不一致问题详解

当前同一个"线程运行状态"概念存在于三个独立位置，由不同事件源驱动：

**源 ①：Sidebar 层**（`DashboardWorkbench` 的 `workspaces` state）
- 类型：`WorkbenchThreadStatus`（`idle | running | needs-reply | waiting-approval | failed | interrupted | limit-reached`）
- 驱动源：Tauri 全局 `emit` 事件（`thread-run-started` → `running`，`thread-run-finished` → 映射到具体状态）
- 位置：`dashboard-workbench.tsx` L~1490，`listen("thread-run-started")` 直接 `setWorkspaces` 修改嵌套的 `threads[].status`

**源 ②：Surface 层**（`RuntimeThreadSurface` 的 `runState` state）
- 类型：`RunState`（9 个字面量联合，比 Sidebar 多 `completed`、`cancelled` 两个状态）
- 驱动源：`ThreadStream` 的 Channel 流事件（精确、实时）
- 位置：`runtime-thread-surface.tsx` 中 `stream.onRunStateChange` 回调

**源 ③：乐观写入层**（`DashboardWorkbench` 的 `pendingThreadRuns` state）
- 用途：新线程提交后，在 `run_started` 事件到达前，本地立即标记为 `running`
- 位置：`dashboard-workbench.tsx` 中 `handleNewThreadSubmit` 函数

三者通过 `mapRunStateToWorkbenchThreadStatus`（`dashboard-workbench-logic.ts`）和 `mapSnapshotToRunState`（`runtime-thread-surface-logic.ts`）互相映射。当 Surface 的 `runState` 变化时，通过 `onRunStateChange` 回调上报给 `DashboardWorkbench`，后者再 `setWorkspaces` 更新 Sidebar 状态。这条链路中任何一环延迟或遗漏，都会导致 Sidebar 和 Surface 显示不一致。

### 相关状态在 DashboardWorkbench 中的分布

以下 `useState` 将被迁移到 `threadStore`：

| 状态变量 | 用途 | 当前位置 |
|---------|------|---------|
| `workspaces` | 工作区列表（含嵌套的 threads） | `dashboard-workbench.tsx` |
| `isNewThreadMode` | 新线程/存量线程模式切换 | `dashboard-workbench.tsx` |
| `isSidebarReady` | Sidebar 初始化完成标志 | `dashboard-workbench.tsx` |
| `pendingThreadRuns` | 待执行的 Run（乐观状态） | `dashboard-workbench.tsx` |
| `workspaceThreadDisplayCounts` | 每工作区显示线程数 | `dashboard-workbench.tsx` |
| `workspaceThreadHasMore` | 每工作区是否有更多线程 | `dashboard-workbench.tsx` |
| `workspaceThreadLoadMorePending` | 加载更多状态 | `dashboard-workbench.tsx` |
| `openWorkspaces` | 每工作区展开/折叠状态 | `dashboard-workbench.tsx` |
| `_defaultWorkspaceId` | 默认工作区 ID | `dashboard-workbench.tsx` |

以及相关的 `useRef` 镜像：`workspaceThreadDisplayCountsRef`、`newThreadCreationRef`、`removedWorkspacePathsRef`。

### 现有纯函数逻辑层

以下已提取的纯函数将被 `threadStore` 的 action 直接复用：
- `model/helpers.ts`：`buildWorkspaceItemsFromDtos`、`sortWorkspaceThreads`、`findThreadInWorkspaces`
- `dashboard-workbench-logic.ts`：`mapRunStateToWorkbenchThreadStatus`
- `runtime-thread-surface-state.ts`：`mapSnapshotToRunState`

## Design

### WorkspaceItem 类型变更（破坏性）

当前 `WorkspaceItem` 嵌套持有 `threads: { ...; status: WorkbenchThreadStatus }[]`，`status` 是内嵌字段。迁移后 `threads` 数组中的 `status` 字段**不再内嵌**，改为从 `threadStore.threadStatuses[threadId]` 派生。

**迁移策略**：
1. 在 `WorkspaceItem` 类型中将 `threads[].status` 标记为 `@deprecated`，新增可选的 `status` 字段（兼容期）
2. 所有读取 `thread.status` 的代码改为 `threadStore.getState().threadStatuses[thread.id] ?? 'idle'`
3. 所有写入 `thread.status` 的代码（`setWorkspaces` 中的嵌套更新）改为 `setThreadStatus(threadId, status)`
4. 在 Phase 1 结束时移除 `WorkspaceItem.threads[].status` 字段

需要迁移的消费者清单：
- `DashboardSidebar`：渲染线程状态标签
- `dashboard-workbench-logic.ts`：`findThreadInWorkspaces`、`mergeLocalFallbackThreads`
- `dashboard-workbench.tsx`：`handleRuntimeThreadRunStateChange`、全局事件监听中的 `setWorkspaces`
- `runtime-thread-surface.tsx`：从 props 获取状态（改为直接订阅 Store）

### 核心设计：统一的 `threadStatuses` Map（被动记录，由状态机驱动）

引入一个扁平的 `Record<string, ThreadStatusRecord>` 作为所有线程状态的集中存储：

```typescript
interface ThreadStatusRecord {
  status: ThreadRunStatus;
  runId: string | null;
  updatedAt: number;
  source: ThreadStatusSource;
}

threadStatuses: Record<string, ThreadStatusRecord>
```

**Phase 1 的 `threadStatuses` 是被动记录层**：它不包含防乱序 reducer 逻辑（如 `shouldAcceptThreadStatus`），而是由 Phase 3 引入的 `runLifecycleMachine`（每线程一个状态机实例）作为权威来源。状态机负责所有合法转换检查和非法事件忽略，通过 `subscribe` 单向同步到 `threadStatuses`。Tauri 全局事件也经过状态机后再写入 Store，避免双重管理冲突。

> **Phase 1 过渡期**：在 Phase 3 之前，`setThreadStatus` 直接写入（无防乱序逻辑），因为此时还没有状态机层。Phase 3 引入状态机后，`setThreadStatus` 成为仅由状态机 subscribe 回调调用的内部函数。

**状态类型统一**：当前 `WorkbenchThreadStatus` 和 `RunState` 是两个不同的类型，有部分重叠。统一后使用一个超集类型 `ThreadRunStatus`：

```typescript
type ThreadRunStatus =
  | 'idle' | 'running' | 'waiting_approval' | 'needs_reply'
  | 'completed' | 'failed' | 'cancelled' | 'interrupted' | 'limit_reached';
```

Sidebar 的渲染逻辑通过一个纯函数 `threadRunStatusToDisplayStatus` 将超集映射为 UI 展示所需的子集（如 `completed` → 不显示状态标记）。

### Store 形状

```typescript
interface ThreadStoreState {
  // 工作区与线程列表
  workspaces: WorkspaceItem[];
  defaultWorkspaceId: string | null;

  // 线程状态（唯一事实源）
  threadStatuses: Record<string, ThreadStatusRecord>;
  activeThreadId: string | null;
  isNewThreadMode: boolean;

  // 乐观 Run 状态
  pendingRuns: Record<string, PendingThreadRun>;

  // Sidebar 分页
  displayCounts: Record<string, number>;
  hasMore: Record<string, boolean>;
  loadMorePending: Record<string, boolean>;
  openWorkspaces: Record<string, boolean>;

  // 初始化
  sidebarReady: boolean;
}
```

### Action 设计

```typescript
// 线程状态写入（Phase 1 过渡期直接写入；Phase 3 后仅由状态机 subscribe 调用）
function setThreadStatus(threadId: string, status: ThreadRunStatus, meta?: { runId?: string | null; source?: ThreadStatusSource; updatedAt?: number }): void;
function batchSetThreadStatuses(updates: Record<string, { status: ThreadRunStatus; meta?: Partial<ThreadStatusEventMeta> }>): void;

// 工作区管理
function setWorkspaces(workspaces: WorkspaceItem[]): void;
function updateWorkspace(workspaceId: string, updater: (ws: WorkspaceItem) => WorkspaceItem): void;
function removeWorkspace(workspaceId: string): void;  // 同时清理该工作区下所有 threadStatuses 条目

// 线程管理
function setActiveThread(threadId: string | null, isNewThread?: boolean): void;
function removeThread(threadId: string): void;
function updateThreadTitle(threadId: string, title: string): void;

// 乐观 Run
function addPendingRun(threadId: string, run: PendingThreadRun): void;
function removePendingRun(threadId: string): void;

// 分页
function setDisplayCount(workspaceId: string, count: number): void;
function setHasMore(workspaceId: string, hasMore: boolean): void;
```

### 事件源接入点改造

> **Phase 1 过渡期**：Tauri 全局事件和 ThreadStream 事件直接调用 `threadStore.setThreadStatus`。Phase 3 引入状态机后，这些事件改为先发送到 `runLifecycleMachine`，状态机通过 `subscribe` 自动同步到 `threadStore`。

**Tauri 全局事件**（当前在 `dashboard-workbench.tsx` 的 `useEffect` 中）：
```
listen("thread-run-started")  → threadStore.setThreadStatus(threadId, 'running', { runId, source: 'tauri_event', updatedAt })
listen("thread-run-finished") → threadStore.setThreadStatus(threadId, mapFinishStatus(status), { runId, source: 'tauri_event', updatedAt })
listen("thread-title-updated") → threadStore.updateThreadTitle(threadId, title)
```

**ThreadStream Channel 事件**（当前在 `runtime-thread-surface.tsx` 的 `onRunStateChange`）：
```
stream.onRunStateChange → threadStore.setThreadStatus(threadId, newStatus, { runId, source: 'stream' })
// RuntimeThreadSurface 不再需要自己的 runState useState
// 改为 useStore(threadStore, s => s.threadStatuses[threadId])
```

**本地乐观写入**（当前在 `handleNewThreadSubmit`）：
```
handleNewThreadSubmit → threadStore.setThreadStatus(newThreadId, 'running', { runId, source: 'optimistic' })
                      → threadStore.addPendingRun(newThreadId, run)
```

### 消除的 Ref-Mirror

迁移后以下 ref 镜像可以删除：
- `workspaceThreadDisplayCountsRef` → 改为 `threadStore.getState().displayCounts`
- `newThreadCreationRef` → 改为 `threadStore.getState().pendingRuns`
- `removedWorkspacePathsRef` → 纳入 Store 或改为 action 内部逻辑

## Key Implementation

### 文件结构

```
src/modules/workbench-shell/model/
├── thread-store.ts              ← Store 定义 + actions（~250 行）
├── thread-store.test.ts         ← 单元测试
└── types.ts                     ← ThreadRunStatus 类型（修改现有）

src/modules/workbench-shell/ui/
├── dashboard-workbench.tsx      ← 删除 9 个 useState + 3 个 useRef，改为 threadStore 订阅
├── dashboard-sidebar.tsx        ← 从 props 改为直接订阅 threadStore
└── runtime-thread-surface.tsx   ← runState 改为从 threadStore 读取
```

### threadStore 核心实现

```typescript
// src/modules/workbench-shell/model/thread-store.ts
import { createStore } from '@/shared/lib/create-store';
import type { WorkspaceItem, PendingThreadRun } from './types';

export type ThreadRunStatus =
  | 'idle' | 'running' | 'waiting_approval' | 'needs_reply'
  | 'completed' | 'failed' | 'cancelled' | 'interrupted' | 'limit_reached';

export type ThreadStatusSource = 'stream' | 'tauri_event' | 'snapshot' | 'optimistic';

interface ThreadStoreState {
  workspaces: WorkspaceItem[];
  defaultWorkspaceId: string | null;
  threadStatuses: Record<string, ThreadStatusRecord>;
  activeThreadId: string | null;
  isNewThreadMode: boolean;
  pendingRuns: Record<string, PendingThreadRun>;
  displayCounts: Record<string, number>;
  hasMore: Record<string, boolean>;
  loadMorePending: Record<string, boolean>;
  openWorkspaces: Record<string, boolean>;
  sidebarReady: boolean;
}

export const threadStore = createStore<ThreadStoreState>({
  workspaces: [],
  defaultWorkspaceId: null,
  threadStatuses: {},
  activeThreadId: null,
  isNewThreadMode: true,
  pendingRuns: {},
  displayCounts: {},
  hasMore: {},
  loadMorePending: {},
  openWorkspaces: {},
  sidebarReady: false,
});

interface ThreadStatusRecord {
  status: ThreadRunStatus;
  runId: string | null;
  updatedAt: number;
  source: ThreadStatusSource;
}

/**
 * 线程状态写入。
 *
 * Phase 1 过渡期：由 Tauri 全局事件、ThreadStream 和乐观写入直接调用。
 * 包含最小 runId 防乱序守卫：如果当前记录的 runId 已更新到新 run，
 * 忽略旧 runId 的写入，防止旧 run 的晚到事件覆盖新 run 的状态。
 * Phase 3 之后：仅由 runLifecycleMachine 的 subscribe 回调调用，
 * 外部事件源先发送到状态机，由状态机做合法转换检查后同步到此处。
 * 届时此处的 runId 守卫可简化或删除。
 */
export function setThreadStatus(
  threadId: string,
  status: ThreadRunStatus,
  meta: { runId?: string | null; source?: ThreadStatusSource; updatedAt?: number } = {},
) {
  threadStore.setState((prev) => {
    const existing = prev.threadStatuses[threadId];
    // 最小防乱序：如果已有更新的 runId，忽略旧 runId 的事件
    if (
      existing &&
      existing.runId !== null &&
      meta.runId !== undefined &&
      meta.runId !== null &&
      existing.runId !== meta.runId &&
      existing.updatedAt > (meta.updatedAt ?? 0)
    ) {
      return {}; // 忽略旧事件，不更新
    }
    return {
      threadStatuses: {
        ...prev.threadStatuses,
        [threadId]: {
          status,
          runId: meta.runId ?? existing?.runId ?? null,
          source: meta.source ?? 'tauri_event',
          updatedAt: meta.updatedAt ?? Date.now(),
        },
      },
    };
  });
}
```

### DashboardWorkbench 改造要点

```typescript
// 之前（9 个 useState）
const [workspaces, setWorkspaces] = useState<WorkspaceItem[]>([]);
const [isNewThreadMode, setIsNewThreadMode] = useState(true);
// ... 7 个更多

// 之后（selector 订阅）
const workspaces = useStore(threadStore, s => s.workspaces);
const isNewThreadMode = useStore(threadStore, s => s.isNewThreadMode);
const activeThreadId = useStore(threadStore, s => s.activeThreadId);
```

### DashboardSidebar 改造要点

```typescript
// 之前（从 props 接收）
interface DashboardSidebarProps {
  workspaces: WorkspaceItem[];
  onThreadSelect: (id: string) => void;
  // ... 34 个 props
}

// 之后（直接订阅 + 精简 props）
function DashboardSidebar({ onThreadSelect, onThreadDelete, ... }: SlimSidebarProps) {
  const workspaces = useStore(threadStore, s => s.workspaces);
  const threadStatuses = useStore(threadStore, s => s.threadStatuses);
  const openWorkspaces = useStore(threadStore, s => s.openWorkspaces);
  // 不再需要从 props 接收这些数据
}
```

### RuntimeThreadSurface 改造要点

```typescript
// 之前
const [runState, setRunState] = useState<RunState>('idle');
// stream.onRunStateChange = (state) => { setRunState(state); onRunStateChange?.(state); };

// 之后
const runStatus = useStore(threadStore, s => s.threadStatuses[threadId]?.status ?? 'idle');
// stream.onRunStateChange = (state, meta) => { setThreadStatus(threadId, state, { ...meta, source: 'stream' }); };
// 不再需要本地 runState，也不再需要 onRunStateChange 回调上报给 parent
```

## Steps

1. **定义 `ThreadRunStatus` 与兼容映射**
   - 在 `src/modules/workbench-shell/model/types.ts` 中新增 `ThreadRunStatus` 类型
   - 明确旧状态命名兼容：`waiting-approval → waiting_approval`、`needs-reply → needs_reply`、`limit-reached → limit_reached`
   - 添加 `threadRunStatusToDisplayStatus` 纯函数（超集 → UI 展示子集映射）
   - 文件：`src/modules/workbench-shell/model/types.ts`

2. **创建 `thread-store.ts`**
   - 使用 `createStore` 创建 `threadStore` 实例
   - 定义 `ThreadStatusRecord`、`ThreadStatusEvent`、`ThreadStatusSource`
   - 实现 `applyThreadStatusEvent` reducer，基于 `runId`、`updatedAt`、`sequence` 和 `source` 忽略旧事件
   - 实现所有 action 函数（`setThreadStatus`、`setWorkspaces`、`setActiveThread`、`removeThread`、`updateThreadTitle`、`addPendingRun`、`removePendingRun`、`setDisplayCount`、`setHasMore`）
   - 文件：`src/modules/workbench-shell/model/thread-store.ts`

3. **迁移 DashboardWorkbench 中的线程相关状态**
   - 删除 9 个 `useState`（`workspaces`、`isNewThreadMode`、`isSidebarReady`、`pendingThreadRuns`、`_defaultWorkspaceId`、`workspaceThreadDisplayCounts`、`workspaceThreadHasMore`、`workspaceThreadLoadMorePending`、`openWorkspaces`）
   - 删除 3 个 `useRef` 镜像（`workspaceThreadDisplayCountsRef`、`newThreadCreationRef`、`removedWorkspacePathsRef`）
   - 替换为 `useStore(threadStore, selector)` 订阅
   - 将 Tauri 全局事件监听（`thread-run-started`、`thread-run-finished`、`thread-title-updated`）改为写入 `threadStore`
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

3.5. **Tauri 事件监听生命周期管理**
   - 保持事件监听在 `dashboard-workbench.tsx` 的 `useEffect` 中注册（应用生命周期内只需注册一次）
   - `useEffect` 返回的 cleanup 函数中调用 `unlisten()` 注销，防止内存泄漏
   - 事件回调直接调用 `threadStore` action，不再经过 `setWorkspaces`
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

4. **迁移 RuntimeThreadSurface 的 runState**
   - 删除 `runState` useState
   - 改为 `useStore(threadStore, s => s.threadStatuses[threadId])`
   - `stream.onRunStateChange` 改为调用 `setThreadStatus(threadId, state)`
   - 删除 `onRunStateChange` props 回调（不再需要上报给 parent）
   - 文件：`src/modules/workbench-shell/ui/runtime-thread-surface.tsx`

5. **精简 DashboardSidebar props**
   - 将 `workspaces`、`openWorkspaces`、`displayCounts`、`hasMore` 等数据从 props 改为直接订阅 `threadStore`
   - 预计可减少 10-15 个 props
   - 文件：`src/modules/workbench-shell/ui/dashboard-sidebar.tsx`

6. **迁移 syncWorkspaceSidebar 函数**
   - 将 `syncWorkspaceSidebar` 内部的 `setWorkspaces` 调用改为 `threadStore.setWorkspaces`
   - `setWorkspaces` 合并后端刷新结果时，只为新增线程初始化缺省状态，不因分页刷新删除未展示线程的 `threadStatuses`
   - 删除线程或删除工作区时才裁剪对应 `threadStatuses` 条目
   - 删除 `runSyncWorkspaceSidebarRef` 镜像模式（Store 的 `getState()` 替代 ref 读取）
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

7. **编写单元测试**
   - 测试 `setThreadStatus` 的正常写入和记录结构
   - 测试多次写入同一 threadId 的覆盖行为（相同 runId 允许覆盖）
   - 测试 runId 防乱序守卫：旧 runId + 旧 updatedAt 被忽略；新 runId 正常写入
   - 测试 `batchSetThreadStatuses` 的批量更新
   - 测试 `removeThread` 清理 `threadStatuses` 和 `workspaces` 中的对应条目
   - 测试 `removeWorkspace` 清理该工作区下所有线程的 `threadStatuses` 条目
   - 测试 `updateThreadTitle` 更新嵌套的 workspace.threads
   - 测试旧命名状态到新命名状态的兼容映射
   - 文件：`src/modules/workbench-shell/model/thread-store.test.ts`

## Verification

1. **单元测试**：`npm run test:unit -- --run src/modules/workbench-shell/model/thread-store.test.ts`，所有用例通过。
2. **类型检查**：`npm run typecheck` 通过。重点关注 `DashboardSidebar` 的 props 类型变更是否有遗漏的调用点。
3. **一致性验证**（手动）：
   - 启动应用，创建新线程并提交 prompt → Sidebar 和 Surface 同时显示 "running"
   - 等待 run 完成 → Sidebar 和 Surface 同时回到 idle
   - 在 run 进行中切换到另一个线程再切回 → 状态不丢失
   - 触发 `stream_resync_required`（模拟事件丢失）→ 快照重载后状态一致
4. **性能验证**：React DevTools Profiler 检查 `DashboardSidebar` 的重渲染频率是否因 selector 粒度而降低。

## Risks

1. **迁移过程中的双写窗口**：在逐步迁移过程中，可能出现部分代码写 `threadStore`、部分代码仍写 `setWorkspaces` 的中间状态。缓解措施：在 `setWorkspaces` 的旧调用点添加 `// TODO: migrate to threadStore` 注释，并在 PR 中确保所有写入点都已迁移。
2. **Selector 粒度与渲染性能**：如果 `useStore(threadStore, s => s.workspaces)` 的 selector 返回整个 `workspaces` 数组，任何线程列表变化都会触发 Sidebar 重渲染。缓解措施：对高频变化的 `threadStatuses` 使用独立 selector（`s => s.threadStatuses`），与 `workspaces` 列表分开订阅。
3. **`syncWorkspaceSidebar` 的复杂度**：这个函数是当前最复杂的副作用（含 Promise 合并、节流、ref guard），迁移时需要特别小心保持其流控语义不变。建议先迁移数据写入点（`setWorkspaces` → `threadStore.setWorkspaces`），暂不改变流控逻辑本身（留给 Phase 3 的状态机改造）。
4. **`RuntimeThreadSurface` 的 `runState` 消除**：当前 `runState` 不仅用于 UI 渲染，还用于控制 `initialPromptRequest` effect 的触发条件。迁移时需要确保 effect 的 deps 从 `runState` 变为 `threadStore.use(...)` 的返回值，语义等价。
5. **Tauri 事件监听泄漏**：如果 `listen()` 返回的 `unlisten` 未被调用，会导致内存泄漏和僵尸更新。确保 `useEffect` cleanup 中正确注销。如果未来应用支持多窗口，需要将事件监听提升到应用级别（如 `src/app/` 的初始化逻辑中）。

## Assumptions

- Phase 0 的 `createStore` 已实现并通过测试。
- `WorkspaceItem` 和 `PendingThreadRun` 类型定义不需要大幅修改，仅需将 `status` 字段的来源从内嵌改为引用 `threadStatuses` map。
- `mapRunStateToWorkbenchThreadStatus` 和 `mapSnapshotToRunState` 纯函数保持不变，仅调用位置从组件内移到 Store action 内。


### 补充约束：后端刷新与状态记录合并

`threadStore.setWorkspaces` 只负责更新工作区与线程列表，不应把 `threadStatuses` 简单重建为后端列表中的状态。后端刷新可能是分页结果或滞后快照，因此只能为新增线程补默认 `idle` 记录，不能删除未展示线程的状态，也不能覆盖已有记录。线程删除、工作区删除或明确收到后端删除事件时，才清理对应 `threadStatuses`。

### 补充约束：Phase 1 过渡期的防乱序策略

Phase 1 过渡期（Phase 3 之前），`setThreadStatus` 包含最小 `runId` 防乱序守卫：如果当前线程的 `threadStatuses[id]` 已记录了更新的 `runId` 和更晚的 `updatedAt`，忽略旧 `runId` 的写入。这比无条件覆盖更安全，能防止旧 run 的晚到 `finished` 事件覆盖新 run 的 `running` 状态——这是当前三源不一致的核心 bug 之一。Phase 3 引入 `runLifecycleMachine` 后，状态机的合法转换图天然防止非法跳转和旧事件覆盖，此处的 `runId` 守卫可简化或删除。
