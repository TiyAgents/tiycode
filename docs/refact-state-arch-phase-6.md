# Phase 6：DashboardWorkbench 最终瘦身

## Summary

将 Phase 1–5 完成后仍残留在 `DashboardWorkbench` 中的跨域状态、Ref-Mirror、页面级副作用和大型复合函数，通过新建 `projectStore`（项目/终端绑定）、将残留 UI 状态按需分配到现有 Store 或局部化、以及将复合逻辑函数提取为独立 action 模块，最终将 `DashboardWorkbench` 从 ~3100 行的上帝组件缩减为以编排为主的入口组件。这是整个重构的收官阶段。

## Context

### 前置依赖

- Phase 0：`createStore` + `createMachine` 基础设施
- Phase 1：`threadStore`（工作区、线程状态唯一事实源）
- Phase 2：`uiLayoutStore`（UI 布局）+ `composerStore`（输入/草稿）
- Phase 3：`runLifecycleMachine`（Run 状态机）+ `sidebarSyncRunner`（同步流控首选）/ `sidebarSyncMachine`（备选）+ `deleteConfirmMachine`
- Phase 4：`settingsStore`（设置数据 + 加载状态机 + IPC action）
- Phase 5：`syncToBackend` IPC 同步中间件

### Phase 1–5 完成后的残余清单

经过 Phase 1–5 的迁移，`DashboardWorkbench` 已将 27 个 `useState` 和大量 `useRef` 迁移到了 Domain Store 和状态机中。但仍有若干跨域 `useState`、Ref-Mirror、页面级 `useEffect` 和 **7 个大型复合函数** 残留。局部 UI state、DOM ref 和组件内交互状态不必为了指标强行迁移。

#### 残留 useState（12 个，按业务域分组）

**项目与终端绑定域**（5 个）：

| 状态变量 | 类型 | 用途 |
|---------|------|------|
| `selectedProject` | `ProjectOption \| null` | 新线程模式的当前选择项目 |
| `recentProjects` | `ProjectOption[]` | 最近项目列表（Sidebar 展示） |
| `terminalThreadBindings` | `Record<string, string>` | new-thread workspace key → threadId 绑定 |
| `terminalWorkspaceBindings` | `Record<string, string>` | path → workspaceId 逆向映射 |
| `terminalBootstrapError` | `string \| null` | 终端/工作区初始化错误 |

**杂项 UI 状态域**（5 个）：

| 状态变量 | 类型 | 用途 |
|---------|------|------|
| `activeThreadProfileIdOverride` | `string \| null` | 活跃线程的 Agent Profile 覆盖 |
| `editingThreadId` | `string \| null` | 正在内联编辑标题的线程 ID |
| `isAddingWorkspace` | `boolean` | 文件夹选择对话框进行中标志 |
| `workspaceAction` | `{ workspaceId, kind } \| null` | 工作区 open/remove 操作进行中的乐观锁 |
| `worktreeDialogContext` | `NewWorktreeDialogContext \| null` | Worktree 创建对话框上下文 |

**回调驱动的派生状态**（2 个）：

| 状态变量 | 类型 | 用途 |
|---------|------|------|
| `runtimeContextUsage` | `ThreadContextUsage \| null` | 当前线程的 Token 使用量（由 RuntimeThreadSurface 回调设置） |
| `topBarGitSnapshot` | `GitSnapshotDto \| null` | 顶栏 Git 分支快照（由 Git 订阅 effect 设置） |

#### 残留 useRef（10+ 个）

| ref | 用途 | 处理方式 |
|-----|------|---------|
| `mainContentRef` | Cmd+A 选择容器 | 保留（DOM ref） |
| `overlayContentRef` | Cmd+A 覆盖层容器 | 保留（DOM ref） |
| `userMenuRef` | outside-click 判定 | 保留（DOM ref） |
| `workspaceMenuRef` | outside-click 判定 | 保留（DOM ref） |
| `editingThreadIdRef` | 事件处理中读取编辑状态 | 随 `editingThreadId` 迁移到 Store |
| `terminalThreadBindingsRef` | state 镜像 | 随 `terminalThreadBindings` 迁移到 Store |
| `terminalWorkspaceBindingsRef` | state 镜像 | 随 `terminalWorkspaceBindings` 迁移到 Store |
| `newThreadCreationRef` | 进行中线程创建 promise 注册表 | 纳入 `projectStore` action 内部 |
| `removedWorkspacePathsRef` | 已删除路径 Set（防无限循环） | 纳入 workspace remove action 内部 |
| `syncVersionRef` | sidebar sync 版本号（stale discard） | Phase 3 已内部化 |
| `sidebarAutoRefreshUntilRef` | 轮询 grace period 时间戳 | Phase 3 已内部化 |

#### 残留 useEffect（10+ 个）

| 编号 | 目的 | 处理方式 |
|------|------|---------|
| E2 | 切换线程时清空 `runtimeContextUsage` | 迁移到 Store 的 `setActiveThread` action |
| E3 | 终端 pre-warm（新线程模式下预创建终端） | 提取为 `useTerminalPreWarm` hook |
| E6 | window.resize 时 clamp terminalHeight | 提取为 `useTerminalResize` hook |
| E8 | terminalResize 拖拽 mousemove/mouseup | 提取为 `useTerminalResize` hook |
| E9 | currentProject 变化时 ensure workspace + path binding | 提取为 `useWorkspaceDiscovery` hook |
| E12 | 一次性清理 localStorage auth session | 保留（最小化 effect） |
| E14 | app updater phase timeout dismiss | 保留（`useAppUpdater` 已是独立 hook） |
| E16 | git subscription + snapshot 更新 | 提取为 `useGitSnapshot` hook |
| E19 | Cmd+A 全局选择处理 | 提取为 `useGlobalKeyboardShortcuts` hook |
| E20 | Cmd+, 设置快捷键 | 合并入 `useGlobalKeyboardShortcuts` hook |

#### 残留复合函数（7 个）

| 函数 | 行数 | 跨域状态 | 处理方式 |
|------|------|---------|---------|
| `handleComposerSubmit`（新线程分支） | ~280 行 | 11 个 state | 提取为 `workbench-actions.ts` 中的 `submitNewThread` |
| `syncWorkspaceSidebar` | ~150 行 | 9 个 state | Phase 3 已由 runner-first 同步流控接管，本阶段完成最后清理 |
| `handleThreadDeleteConfirm` | ~60 行 | 6 个 state | 提取为 `workbench-actions.ts` 中的 `confirmDeleteThread` |
| `handleWorkspaceRemove` | ~80 行 | 9 个 state | 提取为 `workbench-actions.ts` 中的 `removeWorkspace` |
| `getOrCreateNewThreadId` | ~50 行 | 5 个 state | 提取为 `projectStore` 的 action |
| `handleThreadSelect` | ~40 行 | 7 个 state | 提取为 `workbench-actions.ts` 中的 `selectThread` |
| `activateWorkspaceAsNewThreadTarget` | ~50 行 | 9 个 state | 提取为 `workbench-actions.ts` 中的 `activateWorkspace` |

### RuntimeThreadSurface 的剩余整理

`RuntimeThreadSurface` 内部有 17+ 个 `useState` 和 6 个 `useEffect`，但这些都是线程实时执行状态，**本质上应该保持组件本地**，不需要 Store 化。Phase 6 只需整理它与 `DashboardWorkbench` 之间的 props 回调接口：

- `onRunStateChange` → Phase 1/3 已通过 `threadStore` + `runLifecycleMachine` 直接同步，回调可删除
- `onContextUsageChange` → 改为 RuntimeThreadSurface 直接写入 Store
- `onThreadTitleChange` → 改为 RuntimeThreadSurface 直接调用 `threadStore.updateThreadTitle`
- `onComposerDraftChange` → Phase 2 已通过 `composerStore` 直接同步，回调可删除

### DashboardTerminalOrchestrator

`DashboardTerminalOrchestrator`（73 行）是一个纯布局组件，无自身状态。`terminalBootstrapError` 通过 props 从 `DashboardWorkbench` 传入。迁移后 `DashboardTerminalOrchestrator` 直接从 Store 订阅，消除 props 传递。

## Design

### 新增 projectStore — 项目与终端绑定

将项目选择、最近项目列表、终端线程绑定、工作区路径绑定统一管理：

```typescript
interface ProjectStoreState {
  selectedProject: ProjectOption | null;
  recentProjects: ProjectOption[];
  terminalThreadBindings: Record<string, string>;  // workspaceKey → threadId
  terminalWorkspaceBindings: Record<string, string>;  // path → workspaceId
  terminalBootstrapError: string | null;
}
```

**Action**：

```typescript
function selectProject(project: ProjectOption | null): void;
function addRecentProject(project: ProjectOption): void;
function setTerminalBinding(workspaceKey: string, threadId: string): void;
function removeTerminalBinding(workspaceKey: string): void;
function setWorkspaceBinding(path: string, workspaceId: string): void;
function setBootstrapError(error: string | null): void;
```

**关键逻辑 `getOrCreateNewThreadId` 放在编排层**：

当前 `getOrCreateNewThreadId` 函数读取 `terminalThreadBindings`、`newThreadCreationRef`、`activeAgentProfileId`、`workspaces`、`activeThreadProfileIdOverride` 五个状态。该函数直接调用 `threadCreate` IPC，属于跨 Store 编排逻辑，应放在 `workbench-actions.ts` 中而非 `projectStore` 内部——这与架构原则"Store 只管理纯数据、IPC 由 action 层或中间件层处理"一致。`projectStore` 只维护绑定数据的读写：

```typescript
// workbench-actions.ts（编排层）
export async function getOrCreateNewThreadId(
  workspaceKey: string,
  workspaceId: string,
  options: { agentProfileId: string | null },
): Promise<string> {
  const { terminalThreadBindings } = projectStore.getState();

  // 已有绑定，直接返回
  if (terminalThreadBindings[workspaceKey]) {
    return terminalThreadBindings[workspaceKey];
  }

  // 创建新线程（IPC 调用在编排层而非 Store 层）
  const threadId = await threadCreate({
    workspaceId,
    agentProfileId: options.agentProfileId,
  });

  // 绑定（只写 projectStore 的本域数据）
  projectStore.setState((prev) => ({
    terminalThreadBindings: { ...prev.terminalThreadBindings, [workspaceKey]: threadId },
  }));

  return threadId;
}
```

### Phase 6 残留 UI 状态分配策略（不预创建 workbenchUiStore）

Phase 1–5 完成后残留的跨组件 UI 状态，按"优先局部化、按需分配到现有 Store"原则处理，不预创建 `workbenchUiStore`。仅当实际迁移后确认有多个状态确实无法分配到现有 Store 或局部化时，再考虑创建。

**分配方案**：

| 状态变量 | 分配目标 | 理由 |
|---------|---------|------|
| `activeThreadProfileIdOverride` | `threadStore` 或编排 action 内部 | 与活跃线程强关联，`selectThread` action 设置 |
| `editingThreadId` | Sidebar 组件本地 `useState` | 仅 Sidebar + 快捷键两处读写，不需全局化 |
| `isAddingWorkspace` | Sidebar 组件本地 `useState` | 文件夹对话框进行中标志，单组件使用 |
| `workspaceAction` | Sidebar 组件本地 `useState` | 乐观锁标志，单组件使用 |
| `worktreeDialogContext` | `uiLayoutStore` | 属于 overlay/dialog 控制，与 `activeOverlay` 同域 |
| `runtimeContextUsage` | `threadStore`（或专用 hook） | Surface 产出 → TopBar 消费，确需跨组件 |
| `topBarGitSnapshot` | `useGitSnapshot` hook 本地 state | 由 hook 产出 → TopBar 消费，hook 可通过 context 或 store 共享 |

### 复合逻辑提取 — workbench-actions.ts

将 7 个复合函数从组件体内提取为独立模块。每个函数不再通过闭包读取 `useState`，而是通过各 Store 的 `getState()` 读取最新值：

```typescript
// src/modules/workbench-shell/model/workbench-actions.ts

export async function submitNewThread(submission: NewThreadSubmission): Promise<void> {
  const { selectedProject } = projectStore.getState();
  const { newThreadValue, newThreadRunMode } = composerStore.getState();
  const { activeAgentProfileId } = settingsStore.getState();
  const { activeThreadProfileIdOverride } = threadStore.getState();

  // 1. 创建线程
  const threadId = await getOrCreateNewThreadId(workspaceKey, workspaceId, {
    agentProfileId: activeThreadProfileIdOverride ?? activeAgentProfileId,
  });

  // 2. 更新各 Store
  threadStore.setActiveThread(threadId, false);
  threadStore.addPendingRun(threadId, { ... });
  composerStore.setState({ newThreadValue: '', error: null });

  // 3. 同步 sidebar
  sidebarSyncRunner.request({ reason: 'new-thread-submitted' });
}

export async function confirmDeleteThread(threadId: string): Promise<void> { ... }
export async function removeWorkspace(workspaceId: string): Promise<void> { ... }
export function selectThread(threadId: string): void { ... }
export function activateWorkspace(workspaceId: string): void { ... }
```

**关键设计**：这些函数是**跨 Store 的编排逻辑**，它们协调多个 Store 的状态变更。它们不属于任何单个 Store，而是独立的 action 模块。这与 Redux 的 thunk 或 Zustand 的跨 store action 模式一致。

### Effect 提取为专用 Hook

将组件体内的 `useEffect` 提取为语义清晰的自定义 Hook：

```typescript
// src/modules/workbench-shell/hooks/use-workspace-discovery.ts
export function useWorkspaceDiscovery(currentProject: ProjectOption | null): void {
  useEffect(() => {
    // currentProject 变化时的 workspace ensure + path binding 逻辑
    // 读写 projectStore 和 threadStore
  }, [currentProject]);
}

// src/modules/workbench-shell/hooks/use-terminal-pre-warm.ts
export function useTerminalPreWarm(): void {
  const isNewThreadMode = useStore(threadStore, s => s.isNewThreadMode);
  // ...terminal pre-warm 逻辑
}

// src/modules/workbench-shell/hooks/use-git-snapshot.ts
export function useGitSnapshot(workspaceId: string | null): void {
  useEffect(() => {
    // git subscription + snapshot 更新
    // 写入 topBarGitSnapshot 到本地 hook state（由 TopBar 通过 hook 消费）
  }, [workspaceId]);
}

// src/modules/workbench-shell/hooks/use-global-keyboard-shortcuts.ts
export function useGlobalKeyboardShortcuts(refs: { mainContent: RefObject<HTMLElement>; overlay: RefObject<HTMLElement> }): void {
  // Cmd+A, Cmd+, 等全局快捷键
}

// src/modules/workbench-shell/hooks/use-terminal-resize.ts
export function useTerminalResize(): void {
  // window.resize clamp + 拖拽 resize 逻辑
}
```

### 最终的 DashboardWorkbench 形态

```typescript
// src/modules/workbench-shell/ui/dashboard-workbench.tsx (~300 行)

export function DashboardWorkbench() {
  // --- Store 订阅（仅需要的 slice） ---
  const activeThreadId = useStore(threadStore, s => s.activeThreadId);
  const isNewThreadMode = useStore(threadStore, s => s.isNewThreadMode);
  const selectedProject = useStore(projectStore, s => s.selectedProject);
  const activeOverlay = useStore(uiLayoutStore, s => s.activeOverlay);
  const hydrationPhase = useStore(settingsStore, s => s.hydrationPhase);

  // --- 专用 Hook（副作用隔离） ---
  useWorkspaceDiscovery(selectedProject);
  useTerminalPreWarm();
  useGitSnapshot(resolvedWorkspaceId);
  useGlobalKeyboardShortcuts({ mainContent: mainContentRef, overlay: overlayContentRef });
  useTerminalResize();
  const appUpdater = useAppUpdater();

  // --- DOM ref（仅保留 4 个 UI-only ref） ---
  const mainContentRef = useRef<HTMLDivElement>(null);
  const overlayContentRef = useRef<HTMLDivElement>(null);
  const userMenuRef = useRef<HTMLDivElement>(null);
  const workspaceMenuRef = useRef<HTMLDivElement>(null);

  // --- 一次性 effect（最小化） ---
  useEffect(() => {
    localStorage.removeItem('tiy-agent-auth-session');
  }, []);

  // --- 派生计算 ---
  const resolvedWorkspaceId = useMemo(() => /* ... */, [selectedProject, activeThreadId]);
  const resolvedTerminalThreadId = useMemo(() => /* ... */, [activeThreadId, isNewThreadMode]);

  // --- 渲染（纯编排） ---
  return (
    <div className="dashboard-workbench">
      <WorkbenchTopBar />
      <div className="dashboard-body">
        <DashboardSidebar />
        <main ref={mainContentRef}>
          {isNewThreadMode
            ? <NewThreadEmptyState />
            : <RuntimeThreadSurface threadId={activeThreadId!} />
          }
        </main>
        <DashboardTerminalOrchestrator />
      </div>
      <DashboardOverlays ref={overlayContentRef} />
    </div>
  );
}
```

**对比**：

| 指标 | 改造前 | 改造后 |
|------|--------|--------|
| 文件行数 | ~3100 行 | ~300 行 |
| `useState` | 39 个 | 不持有跨域业务状态；允许少量局部 UI state |
| `useRef` | 17+ 个 | 4 个（DOM ref） |
| `useEffect` | 18 个 | 页面级副作用下沉到专用 hook，入口组件只保留必要初始化 |
| 子组件最大 props 数 | 80 个（DashboardOverlays） | ~5 个 |
| 复合函数 | 7 个（共 ~700 行） | 0 个（全部提取到 action 模块） |

### RuntimeThreadSurface Props 精简

```typescript
// 之前（15+ props，含多个回调）
interface RuntimeThreadSurfaceProps {
  threadId: string;
  composerDrafts: Record<string, string>;
  onComposerDraftChange: (id: string, value: string) => void;
  onRunStateChange: (state: RunState) => void;
  onContextUsageChange: (usage: ThreadContextUsage | null) => void;
  onThreadTitleChange: (id: string, title: string) => void;
  pendingThreadRuns: Record<string, PendingThreadRun>;
  workbenchActiveProfileId: string | null;
  // ...
}

// 之后（仅 threadId）
interface RuntimeThreadSurfaceProps {
  threadId: string;
}
```

所有回调改为组件内部直接写入对应 Store：
- `onRunStateChange` → `runLifecycleMachine.send(event)`（Phase 3 已完成）
- `onContextUsageChange` → `threadStore.setState({ runtimeContextUsage: usage })`
- `onThreadTitleChange` → `threadStore.updateThreadTitle(id, title)`
- `onComposerDraftChange` → `composerStore.setDraft(id, value)`（Phase 2 已完成）
- `pendingThreadRuns` → `useStore(threadStore, s => s.pendingRuns)`（Phase 1 已完成）
- `workbenchActiveProfileId` → `useStore(settingsStore, s => s.activeAgentProfileId)` + `useStore(threadStore, s => s.activeThreadProfileIdOverride)`

## Key Implementation

### 文件结构

```
src/modules/workbench-shell/model/
├── project-store.ts               ← 新增（~150 行）
├── workbench-actions.ts           ← 新增（~400 行，7 个复合函数提取）
├── project-store.test.ts          ← 新增
├── workbench-actions.test.ts      ← 新增

src/modules/workbench-shell/hooks/
├── use-workspace-discovery.ts     ← 新增（~80 行，从 E9 effect 提取）
├── use-terminal-pre-warm.ts       ← 新增（~50 行，从 E3 effect 提取）
├── use-git-snapshot.ts            ← 新增（~60 行，从 E16 effect 提取）
├── use-global-keyboard-shortcuts.ts ← 新增（~40 行，从 E19+E20 合并）
├── use-terminal-resize.ts         ← 新增（~50 行，从 E6+E8 合并）

src/modules/workbench-shell/ui/
├── dashboard-workbench.tsx        ← 从 ~3100 行缩减到 ~300 行
├── dashboard-sidebar.tsx          ← 进一步精简 props
├── runtime-thread-surface.tsx     ← props 从 15+ 精简到 1（threadId）
├── dashboard-terminal-orchestrator.tsx ← 从 props 改为 Store 订阅
├── dashboard-overlays.tsx         ← 进一步精简 props

src/features/terminal/model/
├── terminal-store.ts              ← 不迁移为 createStore（手写 pub-sub 与 createStore 语义等价，迁移收益低）。不承接 projectStore 绑定状态；terminalThreadBindings 归属 projectStore，terminalVisibility 归属 uiLayoutStore
```

### projectStore 核心实现

```typescript
// src/modules/workbench-shell/model/project-store.ts
import { createStore } from '@/shared/lib/create-store';
export const projectStore = createStore<ProjectStoreState>({
  selectedProject: null,
  recentProjects: [],
  terminalThreadBindings: {},
  terminalWorkspaceBindings: {},
  terminalBootstrapError: null,
});

export function selectProject(project: ProjectOption | null) {
  const prev = projectStore.getState();
  const updates: Partial<ProjectStoreState> = { selectedProject: project };

  // 添加到最近项目（去重，最多保留 N 个）
  if (project && !prev.recentProjects.some(p => p.path === project.path)) {
    updates.recentProjects = [project, ...prev.recentProjects].slice(0, 20);
  }

  projectStore.setState(updates);
}
```

> **注意**：`projectStore` 只维护纯数据（绑定、最近项目、错误状态），不包含 IPC 调用。`getOrCreateNewThreadId`（含 `threadCreate` IPC）和 `pendingCreations` 防重入逻辑均放在 `workbench-actions.ts` 编排层中。

### workbench-actions.ts 核心实现

```typescript
// src/modules/workbench-shell/model/workbench-actions.ts
import { threadStore } from './thread-store';
import { projectStore, getOrCreateNewThreadId } from './project-store';
import { composerStore } from './composer-store';
import { uiLayoutStore } from './ui-layout-store';
import { settingsStore } from '@/modules/settings-center/model/settings-store';
import { syncToBackend } from '@/shared/lib/ipc-sync';

export async function submitNewThread(submission: {
  value: string;
  runMode: RunMode;
  images?: ImageAttachment[];
}) {
  const { selectedProject } = projectStore.getState();
  if (!selectedProject) return;

  const workspaceId = selectedProject.workspaceId;
  const workspaceKey = selectedProject.path;
  const { activeAgentProfileId } = settingsStore.getState();
  const { activeThreadProfileIdOverride } = threadStore.getState();

  // 1. 获取或创建线程
  const threadId = await getOrCreateNewThreadId(workspaceKey, workspaceId, {
    agentProfileId: activeThreadProfileIdOverride ?? activeAgentProfileId,
  });

  // 2. 切换到线程模式
  threadStore.setActiveThread(threadId, false);

  // 3. 添加 pending run
  threadStore.addPendingRun(threadId, {
    prompt: submission.value,
    runMode: submission.runMode,
    images: submission.images,
  });

  // 4. 清空 composer
  composerStore.setState({ newThreadValue: '', error: null });

  // 5. 触发 sidebar 同步
  // sidebarSyncRunner.request({ reason: 'new-thread-submitted' });
}

export async function confirmDeleteThread(threadId: string) {
  // deleteConfirmMachine.send('CONFIRM');
  await syncToBackend(threadStore, () => threadDelete(threadId), {
    onSuccess: () => {
      threadStore.removeThread(threadId);
      // 清理终端绑定
      projectStore.setState((prev) => {
        const updated = { ...prev.terminalThreadBindings };
        for (const [key, tid] of Object.entries(updated)) {
          if (tid === threadId) delete updated[key];
        }
        return { terminalThreadBindings: updated };
      });
      // 清理终端 session
      terminalStore.removeSession(threadId);
      return {};
    },
  });
  // deleteConfirmMachine.send('DELETE_COMPLETED');
}

export function selectThread(threadId: string) {
  const { workspaces } = threadStore.getState();
  const thread = findThreadInWorkspaces(workspaces, threadId);
  if (!thread) return;

  threadStore.setActiveThread(threadId, false);

  // 解析 profile override
  const profileId = thread.profileId ?? null;
  threadStore.setState({ activeThreadProfileIdOverride: profileId });

  // 关闭菜单
  threadStore.setState({ editingThreadId: null });
  uiLayoutStore.setState({ activeWorkspaceMenuId: null });
}

export async function removeWorkspace(workspaceId: string) {
  // 先 IPC 调用，成功后再更新 Store（避免 IPC 失败时 UI 已不一致）
  await workspaceRemove(workspaceId);

  // IPC 成功后更新 Store
  threadStore.removeWorkspace(workspaceId);

  // 清理项目绑定
  projectStore.setState((prev) => {
    const updated = { ...prev.terminalWorkspaceBindings };
    for (const [path, wsId] of Object.entries(updated)) {
      if (wsId === workspaceId) delete updated[path];
    }
    return {
      terminalWorkspaceBindings: updated,
      selectedProject: prev.selectedProject?.workspaceId === workspaceId ? null : prev.selectedProject,
    };
  });
}

export function activateWorkspace(workspaceId: string, project: ProjectOption) {
  projectStore.selectProject(project);
  threadStore.setState({ isNewThreadMode: true });
  composerStore.setState({ error: null });
  threadStore.setState({
    activeThreadProfileIdOverride: null,
    editingThreadId: null,
  });
  uiLayoutStore.setState({ activeWorkspaceMenuId: null });
  // 确保工作区展开
  threadStore.setState((prev) => ({
    openWorkspaces: { ...prev.openWorkspaces, [workspaceId]: true },
  }));
}
```

## Steps

1. **创建 `project-store.ts`**
   - 定义 `ProjectStoreState` 接口和初始值
   - 实现 `selectProject`、`addRecentProject`、`setTerminalBinding`、`removeTerminalBinding`、`setWorkspaceBinding`、`setBootstrapError`
   - 不包含 IPC 调用——`getOrCreateNewThreadId`（含 `threadCreate` IPC 和防重入逻辑）放在 `workbench-actions.ts` 编排层
   - 文件：`src/modules/workbench-shell/model/project-store.ts`

2. **分配残留 UI 状态**
   - `activeThreadProfileIdOverride` → 加入 `threadStore` 状态（与 `activeThreadId` 同域）
   - `runtimeContextUsage` → 加入 `threadStore` 状态或通过 `useGitSnapshot` hook 共享
   - `worktreeDialogContext` → 加入 `uiLayoutStore`
   - `editingThreadId`、`isAddingWorkspace`、`workspaceAction` → 下沉到 Sidebar 组件本地 `useState`
   - `topBarGitSnapshot` → 由 `useGitSnapshot` hook 本地管理
   - 不预创建 `workbenchUiStore`——如果实际迁移后有状态无法归类，再按需创建

3. **创建 `workbench-actions.ts`**
   - 提取 `getOrCreateNewThreadId`（含 `threadCreate` IPC 调用和 `pendingCreations` 防重入逻辑，替代 `newThreadCreationRef`）
   - 提取 `submitNewThread`（~80 行，从 `handleComposerSubmit` 的新线程分支）
   - 提取 `confirmDeleteThread`（~40 行）
   - 提取 `removeWorkspace`（~50 行）
   - 提取 `selectThread`（~30 行）
   - 提取 `activateWorkspace`（~30 行）
   - 所有函数通过 Store 的 `getState()` 读取状态，不依赖 React 闭包
   - 文件：`src/modules/workbench-shell/model/workbench-actions.ts`

4. **提取 Effect 为专用 Hook**
   - 创建 `use-workspace-discovery.ts`（从 E9 effect 提取）
   - 创建 `use-terminal-pre-warm.ts`（从 E3 effect 提取）
   - 创建 `use-git-snapshot.ts`（从 E16 effect 提取）
   - 创建 `use-global-keyboard-shortcuts.ts`（从 E19+E20 合并）
   - 创建 `use-terminal-resize.ts`（从 E6+E8 合并）
   - 文件：`src/modules/workbench-shell/hooks/` 下 5 个新文件

5. **精简 DashboardWorkbench**
   - 删除所有跨域残留 `useState`，替换为 Store 订阅、专用 hook 或 action；严格局部 UI state 可以保留在对应组件中
   - 删除所有非 DOM `useRef`（约 7 个）
   - 删除所有已提取的 `useEffect`（约 10 个），替换为 Hook 调用
   - 删除所有复合函数体，替换为 `workbench-actions` 调用
   - 最终保留：4 个 DOM ref + 1 个一次性 effect + Store 订阅 + Hook 调用 + 渲染
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

6. **精简 RuntimeThreadSurface props**
   - 删除 `onRunStateChange`、`onContextUsageChange`、`onThreadTitleChange`、`onComposerDraftChange` 回调 props
   - 删除 `composerDrafts`、`pendingThreadRuns`、`workbenchActiveProfileId` 数据 props
   - 组件内部改为直接读写对应 Store
   - 最终 props 仅保留 `threadId: string`
   - 文件：`src/modules/workbench-shell/ui/runtime-thread-surface.tsx`

7. **精简子组件 props**
   - `DashboardTerminalOrchestrator`：从 props 改为 Store 订阅（`uiLayoutStore` + `projectStore`）
   - `DashboardSidebar`：删除已由 Store 替代的剩余 props
   - `WorkbenchTopBar`：删除已由 Store 替代的剩余 props
   - 文件：各子组件文件

8. **编写单元测试**
   - `project-store.test.ts`：测试 `getOrCreateNewThreadId` 的防重入、绑定写入、项目选择
   - `workbench-actions.test.ts`：测试 `submitNewThread`、`confirmDeleteThread`、`selectThread` 的跨 Store 编排逻辑（mock Store）
   - 文件：`src/modules/workbench-shell/model/project-store.test.ts`、`workbench-actions.test.ts`

9. **清理废弃代码**
   - 删除 `useSettingsController` 的薄包装（Phase 4 过渡产物），所有消费者直接使用 `settingsStore`
   - 删除 `useExtensionsController` 中已被 Store 替代的部分（如适用）
   - 搜索全项目确认无残留的旧 props 引用
   - 文件：全项目搜索

## Verification

1. **单元测试**：
   - `npm run test:unit -- --run src/modules/workbench-shell/model/project-store.test.ts`
   - `npm run test:unit -- --run src/modules/workbench-shell/model/workbench-actions.test.ts`
2. **集成测试**：对 `workbench-actions.ts` 中的跨 Store 复合 action（`submitNewThread`、`removeWorkspace`）编写集成测试——使用真实 Store 实例 + mock IPC，验证多 Store 间的数据流一致性。这是单元测试（逐个 mock Store）无法覆盖的场景。
3. **类型检查**：`npm run typecheck` 通过。这是最关键的验证——大量 props 接口变更容易遗漏。
4. **全量回归测试**（手动，完整功能覆盖）：
   - **新线程流程**：选择项目 → 输入 prompt → 提交 → 线程创建 → 运行 → 完成 → Sidebar 更新
   - **线程切换**：点击 Sidebar 线程 → Surface 加载快照 → 草稿恢复 → Profile 覆盖正确
   - **线程删除**：点击删除 → 确认 → Sidebar 移除 → 终端 session 清理
   - **工作区管理**：添加工作区 → 打开工作区 → 移除工作区 → 路径绑定清理
   - **终端**：终端折叠/展开 → 拖拽 resize → 预热 → 错误显示
   - **Git**：分支切换 → 顶栏快照更新 → Diff 面板
   - **Settings**：打开/关闭 → 修改 Provider → 修改 Profile → 持久化
   - **快捷键**：Cmd+A → Cmd+, → Escape
5. **性能验证**：React DevTools Profiler 对比改造前后的渲染频率和渲染时间，确认无性能退化。建议在 CI 中引入 `useRenderCount` 机制，对关键组件（Sidebar、Surface）的渲染次数设定基线。
6. **代码量验证**：`wc -l src/modules/workbench-shell/ui/dashboard-workbench.tsx` 应在 250-350 行之间。

## Risks

1. **跨 Store action 的事务性**：`submitNewThread` 等复合函数涉及多个 Store 的写入。如果中间步骤失败（如 `threadCreate` IPC 失败），已执行的 Store 写入不会自动回滚。缓解措施：将 IPC 调用放在 Store 写入之前（先请求后更新），或在 catch 中手动回滚各 Store。
2. **Hook 提取后的 deps 正确性**：将 `useEffect` 从组件体内提取到自定义 Hook 后，deps 数组的语义可能发生变化（因为 Hook 内部的变量引用链不同）。需要逐个 Hook 验证 deps 的正确性和完整性。
3. **`RuntimeThreadSurface` 的 props 剧变**：从 15+ props 缩减到 1 个是一个大幅度变更，需要确保组件内部正确地从各 Store 订阅了原先通过 props 接收的所有数据。
4. **并发安全**：多个 Store 的读-改-写操作如果在同一个 microtask 中交叉执行，可能读到不一致的中间状态。由于 JavaScript 是单线程的，同步的 `getState()` + `setState()` 序列是原子的，但 `await` 之后的状态可能已被其他操作修改。复合 action 中的 `await` 点需要重新读取 Store 而非使用之前缓存的值。

## Assumptions

- Phase 0–5 已全部完成，所有 Domain Store、状态机、IPC 同步中间件就位。
- `DashboardWorkbench` 的子组件（`DashboardSidebar`、`RuntimeThreadSurface`、`DashboardOverlays`、`WorkbenchTopBar`、`DashboardTerminalOrchestrator`）在 Phase 1–5 中已部分改造为 Store 订阅模式，本阶段只需完成剩余的 props 清理。
- `RuntimeThreadSurface` 的 17+ 个内部 `useState` 保持组件本地，不做 Store 化——它们是线程实时执行状态，本质上与组件生命周期绑定。
- `useAppUpdater` hook 已经是独立的自定义 Hook（`hooks/use-app-updater.ts`），不需要额外处理。
- `useExtensionsController`（`src/modules/extensions-center/model/use-extensions-controller.ts`，270 行）**明确不在本轮重构范围内**。它是 `DashboardOverlays` 的 Extension 相关 props 来源（约 10 个 props），不处理的话 Overlays 的 props 不会降到 0——这些 Extension props 将作为已知残留项保留。其数据域独立于 workbench 核心状态，可后续独立改造为 `extensionStore`。
- 项目的 CI/CD 流程可以覆盖全量回归测试，或有手动测试清单可用。


### 补充约束：最终瘦身指标

Phase 6 的成功标准不是机械达到 `0 useState` 或固定 300 行，而是 `DashboardWorkbench` 不再持有跨域业务状态、不再依赖 Ref-Mirror 读取最新闭包值、不再向深层组件传递大包状态 props。少量局部 UI state、DOM ref 和页面初始化 effect 可以保留，只要它们不承担跨域事实源职责。
