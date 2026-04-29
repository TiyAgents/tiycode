# Phase 3：显式状态机 — Run 生命周期 + Sidebar 同步

## Summary

用 Phase 0 产出的 `createMachine` 工厂将 Run 生命周期替换为显式状态机，并评估 Sidebar 同步流控是否需要状态机化；如果专用 coalesced async runner 足够表达同步语义，则优先采用更小的 runner，避免过度设计。这消除了散布在多个 `useEffect`、`useRef` 和事件回调中的隐式状态转换逻辑，使状态图可视化、非法转换被自动阻止、调试时可直接检查当前状态和可用转换。

## Context

### 前置依赖

- Phase 0：`createMachine` 状态机工厂（`src/shared/lib/create-machine.ts`）
- Phase 1：`threadStore` 已统一线程状态事实源，`setThreadStatus` 是唯一写入入口
- Phase 2：`uiLayoutStore` 和 `composerStore` 已抽取，`DashboardWorkbench` 已大幅瘦身

### 隐式状态机 ①：Run 生命周期

**当前实现**：`RunState` 是一个 9 值字符串联合类型（`idle | running | waiting_approval | needs_reply | completed | failed | cancelled | interrupted | limit_reached`），转换逻辑分散在 **4 个不同位置**：

1. **`ThreadStream.handleEvent`**（`thread-stream.ts`）：接收 Channel 流事件，通过 switch-case 映射到 `RunState`，调用 `onRunStateChange` 回调。这是最精确的事件源。
2. **`loadSnapshot`**（`runtime-thread-surface.tsx`）：快照加载后通过 `mapSnapshotToRunState` 重新推断当前状态。这是恢复/重连场景的入口。
3. **全局 `thread-run-finished` 事件监听**（`dashboard-workbench.tsx` 和 `runtime-thread-surface.tsx`）：作为安全网，当 Channel 流遗漏终止事件时兜底。
4. **`handleRuntimeThreadRunStateChange` 回调**（`dashboard-workbench.tsx`）：Surface 通过 props 回调上报状态变化，Workbench 再更新 Sidebar。

**问题**：
- 没有对非法转换的防护。理论上 `idle → completed` 是不合法的（应该先经过 `running`），但当前代码不会阻止这种跳转。
- 快照恢复（`loadSnapshot`）可能将状态"回退"到更早的阶段（如从 `running` 回到 `idle`），与流式事件竞争。
- `run_retrying` 事件的处理特殊：不改变对外 `RunState`，但内部重置 `currentRunId`。这种"内部状态变化但对外状态不变"的语义在字符串联合类型中无法表达。

### 隐式状态机 ②：Sidebar 同步流控

**当前实现**：`syncWorkspaceSidebar` 函数用 **4 个 `useRef`** 手动实现了一个 coalescing + throttling 状态机：

```
sidebarSyncInFlightRef:        boolean          ← 是否有正在执行的 sync
sidebarSyncInFlightPromiseRef: Promise | null   ← 飞行中的 Promise
sidebarSyncLastFinishedAtRef:  number           ← 上次完成时间戳
sidebarSyncPendingPromiseRef:  Promise | null   ← 排队中的 Promise
sidebarSyncPendingOptionsRef:  Options | null   ← 排队中的合并选项
```

**隐含状态图**：
```
idle → (sync requested) → in_flight → (completed) → idle
                                     → (new request during flight) → pending_trailing
pending_trailing → (flight completed) → in_flight (执行 pending)
```

**问题**：
- 5 个 ref 的组合状态空间是 2^5 = 32 种，但合法组合只有 3 种（idle / in_flight / pending_trailing）。当前代码通过精心编排的 if-else 确保只进入合法组合，但没有显式约束。
- `SIDEBAR_SYNC_MIN_GAP_MS` 节流逻辑内联在函数体中，与状态转换逻辑交织。
- 整个函数通过 `runSyncWorkspaceSidebarRef` 镜像模式暴露给空 deps 的 `useCallback`，调试时无法从 deps 推断触发条件。

### 隐式状态机 ③：线程删除确认

**当前实现**：用 2 个 `useState` 表达 4 个阶段：

```
pendingDeleteThreadId: string | null  ← 待确认删除的线程 ID
deletingThreadId: string | null       ← 正在删除的线程 ID
```

**隐含状态图**：`idle → confirming(threadId) → deleting(threadId) → idle`

这是最简单的案例，但也是最适合首先状态机化的——作为团队熟悉 `createMachine` 模式的入门练习。

### 后端 Run 状态机参考

Rust 后端的 `thread_runs.status` 字段已经有一个清晰的状态机：
```
created → running → waiting_tool_result / waiting_approval / needs_reply
                  → cancelling → cancelled
       → completed | failed | limit_reached | interrupted
```

前端的 Run 状态机应与后端对齐，但不需要完全一致（前端不需要 `created` 和 `waiting_tool_result` 这两个后端内部状态）。

## Design

### Run 生命周期状态机

**状态定义**（与 Phase 1 的 `ThreadRunStatus` 对齐）：

```typescript
type RunMachineState =
  | 'idle' | 'running' | 'waiting_approval' | 'needs_reply'
  | 'completed' | 'failed' | 'cancelled' | 'interrupted' | 'limit_reached';

type RunMachineEvent =
  | 'RUN_STARTED' | 'APPROVAL_REQUIRED' | 'CLARIFY_REQUIRED'
  | 'APPROVAL_RESOLVED' | 'CLARIFY_RESOLVED'
  | 'RUN_RETRYING'
  | 'RUN_COMPLETED' | 'RUN_FAILED' | 'RUN_CANCELLED'
  | 'RUN_INTERRUPTED' | 'LIMIT_REACHED';

interface RunMachineContext {
  runId: string | null;
  errorMessage: string | null;
  retryCount: number;
}
```

**状态图**：

```
idle ──────────── RUN_STARTED ──────────→ running

running ────────── APPROVAL_REQUIRED ──→ waiting_approval
                   CLARIFY_REQUIRED ───→ needs_reply
                   RUN_RETRYING ───────→ running (self, context 更新 runId + retryCount)
                   RUN_COMPLETED ──────→ completed
                   RUN_FAILED ─────────→ failed
                   RUN_CANCELLED ──────→ cancelled
                   RUN_INTERRUPTED ────→ interrupted
                   LIMIT_REACHED ──────→ limit_reached

waiting_approval ─ APPROVAL_RESOLVED ──→ running
                   RUN_CANCELLED ──────→ cancelled
                   RUN_INTERRUPTED ────→ interrupted
                   RUN_FAILED ─────────→ failed
                   RUN_COMPLETED ──────→ completed
                   LIMIT_REACHED ──────→ limit_reached

needs_reply ────── CLARIFY_RESOLVED ───→ running
                   RUN_CANCELLED ──────→ cancelled
                   RUN_INTERRUPTED ────→ interrupted
                   RUN_FAILED ─────────→ failed
                   RUN_COMPLETED ──────→ completed
                   LIMIT_REACHED ──────→ limit_reached

completed ──────── RUN_STARTED ────────→ running
failed ─────────── RUN_STARTED ────────→ running
cancelled ──────── RUN_STARTED ────────→ running
interrupted ────── RUN_STARTED ────────→ running
limit_reached ──── RUN_STARTED ────────→ running
```

> **`SNAPSHOT_RESTORED` 已移除**：快照恢复不通过事件驱动，而是直接调用 `machine.reset(restoredState, restoredContext)`。这避免了需要为每个状态都定义 `SNAPSHOT_RESTORED` 全局转换的复杂度，且语义更清晰——快照恢复是"强制重置"而非"合法状态转换"。

**`run_retrying` 处理**：不改变状态机的对外状态（保持 `running`），但更新 context 中的 `runId` 和 `retryCount`。通过 action 回调实现：

```typescript
running: {
  on: {
    RUN_RETRYING: {
      target: 'running',  // 自转换
      action: (ctx, payload) => ({ ...ctx, runId: payload.newRunId, retryCount: ctx.retryCount + 1 }),
    },
  },
},
```

**每线程一个状态机实例**：不是全局单例，而是每个活跃线程创建一个 `runLifecycleMachine` 实例。实例的生命周期与 `RuntimeThreadSurface` 组件绑定（组件挂载时创建，卸载时销毁）。状态变化通过 `machine.subscribe` 单向同步写入 `threadStore.setThreadStatus`——状态机是权威来源，threadStore 只做被动记录。Phase 1 过渡期由 Tauri 全局事件直接写入 threadStore 的代码，在 Phase 3 完成后改为先经过状态机再由 subscribe 同步。

> **与 Phase 1 的职责边界**：Phase 1 的 `threadStore.setThreadStatus` 在 Phase 3 后不再由外部直接调用，仅作为状态机 `subscribe` 回调的内部写入点。所有合法性检查（非法转换忽略、旧事件防覆盖）由状态机的转换图承担，threadStore 不做重复的防乱序 reducer。这消除了双重管理冲突。

### Sidebar 同步流控：优先专用 runner，必要时再状态机化

Sidebar 同步的核心需求是 single-flight、coalescing、throttling 和失败后恢复。如果这些需求可以用一个专用 `createCoalescedAsyncRunner` 清晰表达，应优先使用 runner；只有当后续出现更多可视化状态、用户可见状态或复杂取消语义时，才升级为完整状态机。

**可选状态机定义**：

```typescript
type SyncState = 'idle' | 'in_flight' | 'pending_trailing' | 'throttled';

type SyncEvent = 'SYNC_REQUESTED' | 'SYNC_COMPLETED' | 'SYNC_FAILED' | 'THROTTLE_EXPIRED';

interface SyncContext {
  pendingOptions: SyncOptions | null;
  lastFinishedAt: number;
  currentPromise: Promise<void> | null;
}
```

**状态图**：

```
idle ──────────── SYNC_REQUESTED ──→ in_flight (如果超过 min gap)
                                   → throttled (如果在 min gap 内)

throttled ──────── THROTTLE_EXPIRED → in_flight
                   SYNC_REQUESTED ──→ throttled (合并 options)

in_flight ──────── SYNC_REQUESTED ──→ pending_trailing (记录 pending options)
                   SYNC_COMPLETED ──→ idle
                   SYNC_FAILED ─────→ idle

pending_trailing ── SYNC_REQUESTED ──→ pending_trailing (合并 options)
                    SYNC_COMPLETED ──→ in_flight (执行 pending)
                    SYNC_FAILED ─────→ idle
```

**关键改进**：
- 若使用 runner，4 个 ref 替换为 1 个专用异步协调器，直接暴露 `request(options)`，减少 FSM 间接层
- 若使用状态机，4 个 ref 替换为 1 个状态机实例，合法状态组合从 32 种降为 4 种
- 节流逻辑（`SIDEBAR_SYNC_MIN_GAP_MS`）通过 runner 或 `throttled` 状态显式表达
- Options 合并逻辑集中在 runner/request 或 `SYNC_REQUESTED` 的 action 回调
- `runSyncWorkspaceSidebarRef` 镜像模式可以删除

### 线程删除确认：判别联合体（不使用 FSM）

3 个状态的删除确认流用 `createMachine` 过度设计，改为简单的判别联合体 `useState`：

```typescript
type DeletePhase =
  | { kind: 'idle' }
  | { kind: 'confirming'; threadId: string }
  | { kind: 'deleting'; threadId: string };
```

替代当前的 `pendingDeleteThreadId` + `deletingThreadId` 两个 useState。这样一个 `useState<DeletePhase>` 即可表达所有状态，TypeScript 自动保证 `kind === 'confirming'` 时 `threadId` 存在。

## Key Implementation

### 文件结构

```
src/modules/workbench-shell/model/
├── run-lifecycle-machine.ts         ← Run 状态机定义（~120 行）
├── run-event-dispatcher.ts          ← 全局 Tauri 事件 → 状态机实例路由（~60 行）
├── sidebar-sync-runner.ts           ← Sidebar 同步 runner（首选，~80 行）
├── sidebar-sync-machine.ts          ← Sidebar 同步状态机（备选，仅 runner 不足时创建）
├── delete-confirm-types.ts          ← DeletePhase 判别联合体类型定义（~15 行）
├── run-lifecycle-machine.test.ts    ← 单元测试
├── sidebar-sync-runner.test.ts      ← runner 单元测试（首选）
├── sidebar-sync-machine.test.ts     ← 状态机单元测试（备选）
└── run-event-dispatcher.test.ts     ← 事件分发单元测试

src/modules/workbench-shell/ui/
├── runtime-thread-surface.tsx       ← 使用 runLifecycleMachine 替代 runState useState
├── dashboard-workbench.tsx          ← 使用 sidebarSyncRunner（首选）或 sidebarSyncMachine（备选）替代 4 个 ref
└── dashboard-sidebar.tsx            ← 使用 deleteConfirmMachine 替代 2 个 useState
```

### Run 生命周期状态机实现

```typescript
// src/modules/workbench-shell/model/run-lifecycle-machine.ts
import { createMachine } from '@/shared/lib/create-machine';
import { setThreadStatus } from './thread-store';

export function createRunLifecycleMachine(threadId: string) {
  const machine = createMachine<RunMachineState, RunMachineEvent, RunMachineContext>({
    initial: 'idle',
    context: { runId: null, errorMessage: null, retryCount: 0 },
    states: {
      idle: {
        on: {
          RUN_STARTED: {
            target: 'running',
            action: (ctx, payload) => ({ ...ctx, runId: payload?.runId ?? null, retryCount: 0, errorMessage: null }),
          },
        },
      },
      running: {
        on: {
          APPROVAL_REQUIRED: 'waiting_approval',
          CLARIFY_REQUIRED: 'needs_reply',
          RUN_RETRYING: {
            target: 'running',  // 自转换
            action: (ctx, payload) => ({ ...ctx, runId: payload?.newRunId ?? ctx.runId, retryCount: ctx.retryCount + 1 }),
          },
          RUN_COMPLETED: 'completed',
          RUN_FAILED: {
            target: 'failed',
            action: (ctx, payload) => ({ ...ctx, errorMessage: payload?.message ?? null }),
          },
          RUN_CANCELLED: 'cancelled',
          RUN_INTERRUPTED: 'interrupted',
          LIMIT_REACHED: 'limit_reached',
        },
      },
      waiting_approval: {
        on: {
          APPROVAL_RESOLVED: 'running',
          RUN_CANCELLED: 'cancelled',
          RUN_INTERRUPTED: 'interrupted',
          RUN_FAILED: { target: 'failed', action: (ctx, payload) => ({ ...ctx, errorMessage: payload?.message ?? null }) },
          RUN_COMPLETED: 'completed',
          LIMIT_REACHED: 'limit_reached',
        },
      },
      needs_reply: {
        on: {
          CLARIFY_RESOLVED: 'running',
          RUN_CANCELLED: 'cancelled',
          RUN_INTERRUPTED: 'interrupted',
          RUN_FAILED: { target: 'failed', action: (ctx, payload) => ({ ...ctx, errorMessage: payload?.message ?? null }) },
          RUN_COMPLETED: 'completed',
          LIMIT_REACHED: 'limit_reached',
        },
      },
      completed: { on: { RUN_STARTED: 'running' } },
      failed:    { on: { RUN_STARTED: 'running' } },
      cancelled: { on: { RUN_STARTED: 'running' } },
      interrupted: { on: { RUN_STARTED: 'running' } },
      limit_reached: { on: { RUN_STARTED: 'running' } },
    },
  });

  // 自动同步到 threadStore
  machine.subscribe(() => {
    setThreadStatus(threadId, machine.getState());
  });

  return machine;
}
```

### ThreadStream 事件到状态机事件的映射

```typescript
// src/modules/workbench-shell/model/run-lifecycle-machine.ts

export function mapStreamEventToMachineEvent(
  eventType: string
): RunMachineEvent | null {
  const mapping: Record<string, RunMachineEvent> = {
    run_started: 'RUN_STARTED',
    approval_required: 'APPROVAL_REQUIRED',
    clarify_required: 'CLARIFY_REQUIRED',
    approval_resolved: 'APPROVAL_RESOLVED',
    run_checkpointed: 'APPROVAL_REQUIRED',  // plan 模式的 checkpoint 等同于 approval
    run_retrying: 'RUN_RETRYING',
    run_completed: 'RUN_COMPLETED',
    run_failed: 'RUN_FAILED',
    run_cancelled: 'RUN_CANCELLED',
    run_interrupted: 'RUN_INTERRUPTED',
    run_limit_reached: 'LIMIT_REACHED',
  };
  return mapping[eventType] ?? null;
}
```

### 全局 Tauri 事件分发层

**问题**：`runLifecycleMachine` 是每线程一个实例，绑定在 `RuntimeThreadSurface` 组件上。但 Tauri 全局事件（如 `thread-run-started`、`thread-run-finished`）对所有线程广播，包括非活跃线程（未渲染 `RuntimeThreadSurface`，因此无状态机实例）。需要一个分发层将全局事件路由到对应线程的状态机实例。

**设计**：创建 `run-event-dispatcher.ts`，维护一个模块级的 `Map<threadId, Machine>` 注册表：

```typescript
// src/modules/workbench-shell/model/run-event-dispatcher.ts
const activeMachines = new Map<string, Machine<RunMachineState, RunMachineEvent, RunMachineContext>>();

export function registerRunMachine(threadId: string, machine: Machine<...>) {
  activeMachines.set(threadId, machine);
}

export function unregisterRunMachine(threadId: string) {
  activeMachines.delete(threadId);
}

export function dispatchGlobalEvent(threadId: string, event: RunMachineEvent, payload?: unknown) {
  const machine = activeMachines.get(threadId);
  if (machine) {
    machine.send(event, payload);
  } else {
    // 非活跃线程：直接写入 threadStore 作为兜底
    // （保持 Phase 1 的 setThreadStatus 行为，确保 Sidebar 状态更新）
    setThreadStatus(threadId, mapMachineEventToStatus(event), { ...payload, source: 'tauri_event' });
  }
}
```

**生命周期**：`RuntimeThreadSurface` 挂载时调用 `registerRunMachine(threadId, machine)`，卸载时调用 `unregisterRunMachine(threadId)`。`DashboardWorkbench` 的 Tauri 全局事件监听改为调用 `dispatchGlobalEvent(threadId, event, payload)` 而非直接写 `threadStore`。

### RuntimeThreadSurface 集成

```typescript
// 之前
const [runState, setRunState] = useState<RunState>('idle');
stream.onRunStateChange = (state) => { setRunState(state); onRunStateChange?.(state); };

// 之后：状态机实例同步创建，组件不条件调用 machine.use()
const machine = useMemo(() => createRunLifecycleMachine(threadId), [threadId]);

useEffect(() => {
  return () => machine.destroy();
}, [machine]);

// 组件从 threadStore 读取状态；状态机通过 subscribe 同步 Store。
const runState = useStore(threadStore, s => s.threadStatuses[threadId]?.status ?? 'idle');

// ThreadStream 事件驱动状态机，并携带 runId/sequence/updatedAt，避免旧事件覆盖新状态。
stream.onEvent = (event) => {
  const machineEvent = mapStreamEventToMachineEvent(event.type);
  if (machineEvent) {
    machine.send(machineEvent, {
      ...event.payload,
      runId: event.runId,
      sequence: event.sequence,
      updatedAt: event.timestamp ?? Date.now(),
    });
  }
};

// 快照恢复时强制重置，但仍携带 source='snapshot' 和 updatedAt，交给 threadStore reducer 防回退。
async function loadSnapshot(threadId: string) {
  const snapshot = await threadLoad(threadId);
  const restoredState = mapSnapshotToRunState(snapshot);
  machine.reset(restoredState, {
    runId: snapshot.activeRunId,
    errorMessage: null,
    retryCount: 0,
  });
}
```

### Sidebar 同步流控集成

```typescript
// 之前（4 个 ref）
const sidebarSyncInFlightRef = useRef(false);
const sidebarSyncInFlightPromiseRef = useRef<Promise<void> | null>(null);
const sidebarSyncLastFinishedAtRef = useRef(0);
const sidebarSyncPendingPromiseRef = useRef<Promise<void> | null>(null);
const sidebarSyncPendingOptionsRef = useRef<SyncOptions | null>(null);

// 之后（优先 1 个专用 runner；必要时可替换为状态机）
const sidebarSyncRunner = createCoalescedAsyncRunner({
  minGapMs: SIDEBAR_SYNC_MIN_GAP_MS,
  executeFn: async (options) => {
    // 实际的 IPC 调用逻辑
    const data = await listVisibleWorkspaceThreads(options);
    threadStore.setWorkspaces(data);
  },
});

// 触发同步
function syncWorkspaceSidebar(options?: SyncOptions) {
  return sidebarSyncRunner.request(options);
}
```

## Steps

1. **创建 `run-lifecycle-machine.ts`**
   - 定义 `RunMachineState`、`RunMachineEvent`、`RunMachineContext` 类型
   - 实现 `createRunLifecycleMachine(threadId)` 工厂函数
   - 实现 `mapStreamEventToMachineEvent` 映射函数
   - 内置 `subscribe` 自动同步到 `threadStore`
   - 文件：`src/modules/workbench-shell/model/run-lifecycle-machine.ts`

1.5. **创建 `run-event-dispatcher.ts`**
   - 实现全局 Tauri 事件 → 状态机实例路由
   - 维护 `activeMachines: Map<string, Machine>` 注册表
   - 导出 `registerRunMachine`、`unregisterRunMachine`、`dispatchGlobalEvent`
   - 非活跃线程事件兜底写入 `threadStore`（保持 Sidebar 更新）
   - 文件：`src/modules/workbench-shell/model/run-event-dispatcher.ts`

2. **创建 Sidebar 同步流控工具**
   - 优先创建 `sidebar-sync-runner.ts`，实现 `createCoalescedAsyncRunner(config)`，覆盖 single-flight、coalescing、throttling、失败恢复
   - 如果 runner 无法表达现有语义，再创建 `sidebar-sync-machine.ts`，定义 `SyncState`、`SyncEvent`、`SyncContext` 类型并实现 `createSidebarSyncMachine(config)`
   - 文件：`src/modules/workbench-shell/model/sidebar-sync-runner.ts`（首选）或 `sidebar-sync-machine.ts`（备选）

3. **定义 `delete-confirm-types.ts`**
   - 定义 `DeletePhase` 判别联合体类型
   - 不使用 `createMachine`——3 态线性流不需要 FSM
   - 文件：`src/modules/workbench-shell/model/delete-confirm-types.ts`

4. **改造 RuntimeThreadSurface 使用 Run 状态机**
   - 删除 `runState` useState（如 Phase 1 未完全删除）
   - 使用 `useMemo` 或 `useState` 同步创建状态机实例，严禁在 `machineRef.current?.use()` 这类条件路径中调用 Hook
   - 挂载时调用 `registerRunMachine(threadId, machine)`，卸载时调用 `unregisterRunMachine(threadId)`
   - `ThreadStream` 事件通过 `mapStreamEventToMachineEvent` 驱动状态机
   - Tauri 全局事件（`thread-run-started`、`thread-run-finished`）改为通过 `dispatchGlobalEvent` 路由到状态机，不再直接调用 `threadStore.setThreadStatus`
   - `loadSnapshot` 使用 `machine.reset()` 强制恢复
   - 删除 `onRunStateChange` props 回调（状态机自动同步到 `threadStore`）
   - 文件：`src/modules/workbench-shell/ui/runtime-thread-surface.tsx`

5. **改造 DashboardWorkbench 使用 Sidebar 同步状态机**
   - 删除 4 个 sync ref（`sidebarSyncInFlightRef`、`sidebarSyncInFlightPromiseRef`、`sidebarSyncLastFinishedAtRef`、`sidebarSyncPendingPromiseRef`、`sidebarSyncPendingOptionsRef`）
   - 删除 `runSyncWorkspaceSidebarRef` 镜像模式
   - 创建 `sidebarSyncRunner`（首选）或 `sidebarSyncMachine`（备选）实例
   - `syncWorkspaceSidebar` 改为 `sidebarSyncRunner.request(options)`；若采用 FSM，则改为 `syncMachine.send('SYNC_REQUESTED', options)`
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

6. **改造线程删除使用判别联合体**
   - 删除 `pendingDeleteThreadId` 和 `deletingThreadId` 两个 useState
   - 替换为 `useState<DeletePhase>({ kind: 'idle' })`
   - 删除按钮 → `setDeletePhase({ kind: 'confirming', threadId })`
   - 确认按钮 → `setDeletePhase({ kind: 'deleting', threadId })`
   - 取消按钮 → `setDeletePhase({ kind: 'idle' })`
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx` 或 `dashboard-sidebar.tsx`

7. **编写单元测试**
   - `run-lifecycle-machine.test.ts`：测试完整状态图（所有合法转换 + 非法转换忽略 + context 更新 + RUN_RETRYING 自转换 context 通知 + reset + 旧事件不覆盖新 run + 从 waiting_approval/needs_reply 的终止转换）
   - `run-event-dispatcher.test.ts`：测试活跃线程事件路由到状态机、非活跃线程事件兜底写入 threadStore、注册/注销生命周期
   - `sidebar-sync-runner.test.ts` 或 `sidebar-sync-machine.test.ts`：测试 coalescing（多次请求合并）、throttling（min gap 内排队）、失败恢复
   - 文件：对应的 `.test.ts` 文件

## Verification

1. **单元测试**：`npm run test:unit -- --run src/modules/workbench-shell/model/run-lifecycle-machine.test.ts src/modules/workbench-shell/model/sidebar-sync-runner.test.ts src/modules/workbench-shell/model/delete-confirm-machine.test.ts`。如果最终采用 FSM 备选方案，则把 `sidebar-sync-runner.test.ts` 替换为 `sidebar-sync-machine.test.ts`
2. **类型检查**：`npm run typecheck` 通过。
3. **Run 状态机验证**（手动）：
   - 提交 prompt → 状态从 idle → running → completed，Sidebar 和 Surface 同步
   - 触发 approval → 状态到 waiting_approval → 批准后回到 running
   - 取消 run → 状态到 cancelled
   - 快速连续提交两次 → 第二次在 running 状态下被正确处理（不会跳到 idle）
   - 断开重连（`stream_resync_required`）→ `machine.reset()` 正确恢复
4. **Sidebar 同步验证**（手动）：
   - 快速切换多个线程 → 同步请求被合并，不会发出 N 次 IPC 调用
   - 同步失败 → 状态回到 idle，下次请求正常执行
5. **删除确认验证**（手动）：
   - 点击删除 → 显示确认对话框 → 确认 → 线程被删除
   - 点击删除 → 显示确认对话框 → 取消 → 回到正常状态

## Risks

1. **Run 状态机的实例生命周期**：每个线程一个状态机实例，需要在线程切换时正确销毁旧实例、创建新实例。如果 `RuntimeThreadSurface` 的 `threadId` prop 变化但组件不卸载（React key 未变），需要在 `useEffect` 中手动重建状态机。
2. **`SNAPSHOT_RESTORED` 的竞态**：快照恢复（`machine.reset`）和流式事件（`machine.send`）可能在同一个 tick 内发生。由于状态机是权威来源，`reset` 后状态机处于新状态，后续的 `send` 基于新状态执行转换。缓解措施：在 `loadSnapshot` 中先 `reset`，再重新订阅流。
3. **Sidebar 同步过度状态机化**：如果仅有 single-flight、coalescing 和 throttle，完整 FSM 会增加异步 action 与 subscribe 间接层。优先使用专用 runner；只有 runner 无法覆盖状态可视化或复杂取消语义时再升级为 FSM。
4. **`run_retrying` 的自转换**：状态机的自转换（`running → running`）不会触发 `subscribe` 通知（因为状态没变），但 context 变了（`runId` 更新）。需要确保 `createMachine` 的实现在 context 变化时也通知订阅者。

## Assumptions

- Phase 0 的 `createMachine` 支持 `destroy()` 方法和 context 变化时的订阅者通知（即使状态未变）。
- Phase 1 的 `threadStore.setThreadStatus` 已实现且可用。
- `ThreadStream` 的事件类型字符串（`run_started`、`approval_required` 等）是稳定的，不会频繁变更。
- `SIDEBAR_SYNC_MIN_GAP_MS` 常量值保持不变（当前定义在 `dashboard-workbench-logic.ts` 中）。
- `ThreadStream` 本身保持回调驱动模式，不在此 Phase 中 Store 化（其生命周期逻辑与组件强绑定）。
