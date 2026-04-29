# Phase 2：uiLayoutStore + composerStore — 消除 Props Drilling

## Summary

从 `DashboardWorkbench` 中抽取两个 Domain Store：`uiLayoutStore`（跨组件共享的 UI 布局与面板状态）和 `composerStore`（需要跨线程/跨组件保留的输入草稿状态），将当前通过 props 层层传递的共享 UI 状态改为子组件直接订阅。本阶段预计从上帝组件中移除约 15 个跨域 `useState`，但不要求把焦点、光标、拖拽中间态、一次性菜单开关等严格局部交互状态 Store 化。

## Context

### 前置依赖

- Phase 0：`createStore` 工厂函数（`src/shared/lib/create-store.ts`）
- Phase 1：`threadStore` 已接管线程与工作区状态，`DashboardWorkbench` 已减少 9 个 `useState` + 3 个 `useRef`

### Phase 1 完成后的残余状态

Phase 1 迁移了线程/工作区相关的 9 个 `useState`。`DashboardWorkbench` 中仍残留约 **30 个 `useState`**，按业务域可分为两大类：

**UI 布局类**（不涉及 IPC，纯前端状态）：

| 状态变量 | 用途 |
|---------|------|
| `activeOverlay` | 当前覆盖层（settings/marketplace/extensions-center/null） |
| `activeSettingsCategory` | 设置面板当前 Tab |
| `openSettingsSection` | 设置区域展开项 |
| `panelVisibilityState` | Sidebar/Drawer 开关状态 |
| `activeDrawerPanel` | Drawer 面板类型（project/git） |
| `selectedDiffSelection` | Git Diff 选中内容 |
| `terminalCollapsedByThreadKey` | 每线程的终端折叠状态 |
| `terminalHeight` | 终端面板高度 |
| `terminalResize` | 拖拽 Resize 起始状态 |
| `isUserMenuOpen` | 用户菜单开关 |
| `activeWorkspaceMenuId` | 当前打开菜单的工作区 ID |
| `showOnboarding` | Onboarding 显示控制 |

**Composer 输入类**：

| 状态变量 | 用途 |
|---------|------|
| `composerValue` | 新线程 Composer 输入值 |
| `composerDrafts` | 每线程草稿缓存（`Record<string, string>`） |
| `composerError` | Composer 错误信息 |
| `newThreadRunMode` | 新线程的 RunMode（default/plan） |

### Props Drilling 现状

Phase 1 精简了 `DashboardSidebar` 的数据类 props，但 UI 布局类 props 仍在传递：

- `DashboardOverlays` 接收约 **97 个 props**，其中大量是 settings 的 getter/setter、overlay 控制回调、以及 `syncWorkspaceSidebar` 等跨域函数。UI 布局类 props（`activeOverlay`、`setActiveOverlay`、`activeSettingsCategory`、`setActiveSettingsCategory`、`openSettingsSection`、`setOpenSettingsSection`）占约 10 个。
- `WorkbenchTopBar` 接收约 **24 个 props**，其中 `panelVisibilityState`、`activeDrawerPanel`、`terminalCollapsed` 等布局状态占 6-8 个。
- `WorkbenchPromptComposer` 接收 `composerValue`、`onComposerChange`、`composerError`、`runMode` 等约 25 个 props。

### Composer 草稿重复问题

当前 Composer 草稿存在三处：
1. `composerValue`（workbench state）：新线程模式的输入值
2. `composerDrafts[threadId]`（workbench state map）：存量线程的草稿
3. `localComposerValue`（`RuntimeThreadSurface` 内部 state）：当 `onComposerDraftChange` 未传入时的降级

`composerStore` 将统一管理所有草稿，消除这种碎片化。

## Design

### uiLayoutStore — 纯 UI 布局状态

**设计原则**：这个 Store 只管理不涉及后端持久化、且确实需要跨组件共享或跨副作用协调的前端 UI 状态。严格局部的交互状态继续留在组件或专用 hook 中，避免 Store 成为 UI 杂物箱。

```typescript
interface UILayoutStoreState {
  // 覆盖层
  activeOverlay: OverlayType | null;
  activeSettingsCategory: string | null;
  openSettingsSection: string | null;

  // 面板可见性
  panelVisibility: PanelVisibilityState;
  activeDrawerPanel: DrawerPanel | null;
  selectedDiffSelection: DiffSelection | null;

  // 终端布局
  terminalCollapsedByThreadKey: Record<string, boolean>;
  terminalHeight: number;
  terminalResize: TerminalResizeState | null;

  // 菜单状态（仅当多个组件需要读写时才进入 Store；局部菜单可保留本地 state）
  isUserMenuOpen: boolean;
  activeWorkspaceMenuId: string | null;

  // Onboarding
  showOnboarding: boolean;
}
```

**Action 设计**：每个状态对应一个简单的 setter action，无复杂逻辑：

```typescript
function openOverlay(type: OverlayType, settingsCategory?: string): void;
function closeOverlay(): void;
function toggleSidebar(): void;
function toggleDrawer(panel: DrawerPanel): void;
function setTerminalCollapsed(threadKey: string, collapsed: boolean): void;
function setTerminalHeight(height: number): void;
```

**局部状态保留规则**：以下状态不应为了“减少 useState”而迁入 Store：输入框焦点、光标位置、临时 hover/pressed 状态、拖拽中的鼠标坐标、只被单个弹层使用的一次性菜单开关、DOM ref。它们的生命周期与具体组件绑定，留在组件本地更简单、更可预测。

**特殊处理**：`terminalHeight` 的拖拽 resize 涉及高频更新（mousemove），Store 的 `setState` 每次都会通知所有订阅者。为避免性能问题，resize 过程中拖拽中间态由 `TerminalResize` 组件内部用本地 `useState` 管理（不写入 Store），仅在 `mouseup` 时将最终高度写入 `uiLayoutStore.setTerminalHeight`。

### composerStore — 输入与草稿状态

```typescript
interface ComposerStoreState {
  // 新线程输入
  newThreadValue: string;
  newThreadRunMode: RunMode;

  // 存量线程草稿
  drafts: Record<string, string>;

  // 错误
  error: string | null;
}
```

**Action 设计**：

```typescript
function setNewThreadValue(value: string): void;
function setNewThreadRunMode(mode: RunMode): void;
function setDraft(threadId: string, value: string): void;
function getDraft(threadId: string): string;
function removeDraft(threadId: string): void;
function setError(error: string | null): void;
function clearError(): void;
```

**草稿统一策略**：
- 新线程模式：读写 `newThreadValue`
- 存量线程模式：读写 `drafts[threadId]`
- `RuntimeThreadSurface` 不再需要 `localComposerValue` 降级逻辑，直接从 `composerStore` 读取
- 切换线程时，当前输入自动保存到 `drafts[currentThreadId]`，新线程的草稿自动加载

### 子组件直接订阅模式

改造后的数据流：

```
DashboardWorkbench（编排层，不再持有 UI 布局状态）
├── WorkbenchTopBar
│   └── useStore(uiLayoutStore, s => s.panelVisibility)  // 直接订阅
├── DashboardSidebar
│   └── useStore(threadStore, ...)  // Phase 1 已改造
├── RuntimeThreadSurface
│   ├── useStore(threadStore, s => s.threadStatuses[id])  // Phase 1
│   └── useStore(composerStore, s => s.drafts[id])        // 本阶段
├── WorkbenchPromptComposer
│   └── useStore(composerStore, s => s.newThreadValue)    // 直接订阅
└── DashboardOverlays
    └── useStore(uiLayoutStore, s => s.activeOverlay)     // 直接订阅
```

### 消除或下沉的 useEffect

以下 `DashboardWorkbench` 中的 useEffect 可以简化或删除：
- **userMenu 点击关闭 effect**：如果菜单只属于 TopBar，可下沉到 TopBar 本地处理；只有多个组件需要协调时才通过 `uiLayoutStore` action 处理
- **workspaceMenu 关闭 effect**：如果菜单只属于 Sidebar，可下沉到 Sidebar 本地处理；只有 overlay/全局快捷键需要协调时才进入 Store
- **overlay Escape 监听 effect**：改为 `DashboardOverlays` 组件内部自行监听，通过 `uiLayoutStore.closeOverlay()` 关闭
- **isOverlayOpen → closeMenu effect**：改为 `openOverlay` action 内部联动关闭菜单

## Key Implementation

### 文件结构

```
src/modules/workbench-shell/model/
├── ui-layout-store.ts           ← UI 布局 Store（~150 行）
├── composer-store.ts            ← Composer Store（~100 行）
├── ui-layout-store.test.ts      ← 单元测试
└── composer-store.test.ts       ← 单元测试

src/modules/workbench-shell/ui/
├── dashboard-workbench.tsx      ← 删除 ~15 个 useState，删除 4 个 useEffect
├── dashboard-overlays.tsx       ← 从 97 props 精简到 ~55 props（settings 相关仍需传递）
├── workbench-top-bar.tsx        ← 从 ~24 props 精简到 ~12 props
└── runtime-thread-surface.tsx   ← 删除 localComposerValue，改用 composerStore
```

### uiLayoutStore 核心实现

```typescript
// src/modules/workbench-shell/model/ui-layout-store.ts
import { createStore } from '@/shared/lib/create-store';

export const uiLayoutStore = createStore<UILayoutStoreState>({
  activeOverlay: null,
  activeSettingsCategory: null,
  openSettingsSection: null,
  panelVisibility: { sidebar: true, drawer: false },
  activeDrawerPanel: null,
  selectedDiffSelection: null,
  terminalCollapsedByThreadKey: {},
  terminalHeight: 300,
  terminalResize: null,
  isUserMenuOpen: false,
  activeWorkspaceMenuId: null,
  showOnboarding: false,
});

export function openOverlay(type: OverlayType, settingsCategory?: string) {
  uiLayoutStore.setState({
    activeOverlay: type,
    activeSettingsCategory: settingsCategory ?? null,
    // 联动：打开 overlay 时关闭所有菜单
    isUserMenuOpen: false,
    activeWorkspaceMenuId: null,
  });
}

export function closeOverlay() {
  uiLayoutStore.setState({
    activeOverlay: null,
    activeSettingsCategory: null,
    openSettingsSection: null,
  });
}
```

### composerStore 核心实现

```typescript
// src/modules/workbench-shell/model/composer-store.ts
import { createStore } from '@/shared/lib/create-store';

export const composerStore = createStore<ComposerStoreState>({
  newThreadValue: '',
  newThreadRunMode: 'default',
  drafts: {},
  error: null,
});

export function setDraft(threadId: string, value: string) {
  composerStore.setState((prev) => ({
    drafts: { ...prev.drafts, [threadId]: value },
  }));
}

export function getDraftOrEmpty(threadId: string): string {
  return composerStore.getState().drafts[threadId] ?? '';
}

// 清空新线程 Composer 状态（用于线程切换或提交后）
export function clearNewThreadComposer() {
  composerStore.setState({
    newThreadValue: '',
    error: null,
  });
}
```

### DashboardOverlays Props 精简

```typescript
// 之前（~130 props，包含大量 UI 布局控制）
interface DashboardOverlaysProps {
  activeOverlay: OverlayType | null;
  setActiveOverlay: (o: OverlayType | null) => void;
  activeSettingsCategory: string | null;
  setActiveSettingsCategory: (c: string) => void;
  openSettingsSection: string | null;
  setOpenSettingsSection: (s: string | null) => void;
  // ... 还有 settings 的全量 getter/setter
}

// 之后（~30 props，仅保留 settings 业务数据）
interface DashboardOverlaysProps {
  // UI 布局状态从 uiLayoutStore 直接订阅，不再通过 props
  // 仅保留 settings 业务数据和回调
  settings: SettingsState;
  onSettingsChange: (patch: Partial<SettingsState>) => void;
  extensions: ExtensionsState;
  syncWorkspaceSidebar: () => Promise<void>;
  // ...
}
```

## Steps

1. **创建 `ui-layout-store.ts`**
   - 定义 `UILayoutStoreState` 接口和初始值
   - 实现 action 函数：`openOverlay`、`closeOverlay`、`toggleSidebar`、`toggleDrawer`、`setTerminalCollapsed`、`setTerminalHeight`、`setUserMenuOpen`、`setWorkspaceMenuId`
   - 在 `openOverlay` 中实现联动关闭菜单逻辑
   - 文件：`src/modules/workbench-shell/model/ui-layout-store.ts`

2. **创建 `composer-store.ts`**
   - 定义 `ComposerStoreState` 接口和初始值
   - 实现 action 函数：`setNewThreadValue`、`setNewThreadRunMode`、`setDraft`、`getDraftOrEmpty`、`removeDraft`、`setError`、`clearError`、`clearNewThreadComposer`
   - 文件：`src/modules/workbench-shell/model/composer-store.ts`

3. **迁移 DashboardWorkbench 的 UI 布局状态**
   - 删除或下沉 12 个 `useState`（`activeOverlay`、`activeSettingsCategory`、`openSettingsSection`、`panelVisibilityState`、`activeDrawerPanel`、`selectedDiffSelection`、`terminalCollapsedByThreadKey`、`terminalHeight`、`terminalResize`、`isUserMenuOpen`、`activeWorkspaceMenuId`、`showOnboarding`）；其中严格局部状态可以迁入对应组件或 hook，而不是必须进入 Store
   - 对跨组件共享状态替换为 `useStore(uiLayoutStore, selector)` 订阅；对局部状态下沉到组件本地
   - 删除 4 个关联的 useEffect（userMenu 关闭、workspaceMenu 关闭、overlay Escape、isOverlayOpen 联动）
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

4. **迁移 DashboardWorkbench 的 Composer 状态**
   - 删除 4 个 `useState`（`composerValue`、`composerDrafts`、`composerError`、`newThreadRunMode`）
   - 替换为 `useStore(composerStore, selector)` 订阅
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

5. **改造 DashboardOverlays**
   - 从 props 中移除 UI 布局类字段（`activeOverlay`、`setActiveOverlay` 等约 10 个）
   - 组件内部直接订阅 `uiLayoutStore`
   - Escape 键监听移入组件内部，调用 `closeOverlay()`
   - 文件：`src/modules/workbench-shell/ui/dashboard-overlays.tsx`

6. **改造 WorkbenchTopBar**
   - 从 props 中移除布局类字段（`panelVisibility`、`activeDrawerPanel` 等约 6 个）
   - 组件内部直接订阅 `uiLayoutStore`
   - 文件：`src/modules/workbench-shell/ui/workbench-top-bar.tsx`（或对应文件名）

7. **改造 RuntimeThreadSurface 的 Composer 集成**
   - 删除 `localComposerValue` useState
   - 改为 `useStore(composerStore, s => s.drafts[threadId])`
   - 删除 `onComposerDraftChange` props 回调
   - 文件：`src/modules/workbench-shell/ui/runtime-thread-surface.tsx`

8. **编写单元测试**
   - `ui-layout-store.test.ts`：测试 `openOverlay` 联动关闭菜单、`toggleSidebar` 切换、`closeOverlay` 清理
   - `composer-store.test.ts`：测试草稿读写、`clearNewThreadComposer` 清理、错误清理
   - 文件：`src/modules/workbench-shell/model/ui-layout-store.test.ts`、`composer-store.test.ts`

## Verification

1. **单元测试**：`npm run test:unit -- --run src/modules/workbench-shell/model/ui-layout-store.test.ts src/modules/workbench-shell/model/composer-store.test.ts`
2. **类型检查**：`npm run typecheck` 通过。重点关注 `DashboardOverlays` 和 `WorkbenchTopBar` 的 props 类型变更。
3. **手动验证**：
   - 打开/关闭 Settings overlay → 正常切换，Escape 键关闭正常
   - 切换 Sidebar/Drawer 面板 → 面板可见性正确
   - 终端折叠/展开、拖拽 resize → 高度变化流畅
   - 在新线程输入内容 → 切换到存量线程 → 切回 → 草稿保留
   - 存量线程输入草稿 → 切换线程 → 切回 → 草稿保留
4. **Props 数量验证**：通过 TypeScript 接口定义确认 `DashboardOverlays` props 从 97 降至 ~55，`WorkbenchTopBar` 从 ~24 降至 ~12。

## Risks

1. **终端高度拖拽性能**：`terminalHeight` 在 mousemove 期间高频更新，每次 `setState` 都会通知所有 `uiLayoutStore` 订阅者。缓解措施：resize 期间使用 `requestAnimationFrame` 节流，或在 resize 组件内部用本地 state 管理拖拽中间态，mouseup 时才写入 Store。
2. **Overlay 内部的 Settings 状态**：`DashboardOverlays` 的 97 个 props 中，约 50 个是 settings 相关的 getter/setter。本阶段只移除 UI 布局类 props（~10 个），settings 相关 props 留给 Phase 4 处理。中间态下 `DashboardOverlays` 仍有约 55 个 props。
3. **Composer 焦点管理**：当前 Composer 的焦点状态（是否聚焦、光标位置）通过 ref 管理，不在 Store 范围内。迁移草稿状态时需确保焦点行为不受影响。
4. **`selectedDiffSelection` 的跨组件通信**：这个状态在 Drawer 的 Git Diff 面板中设置，在主内容区域中消费，属于适合进入 `uiLayoutStore` 的共享状态；迁移后需确保选中/取消选中的时序正确。
5. **UI Store 过度膨胀**：如果把焦点、hover、拖拽坐标、单组件菜单等局部状态也放入 `uiLayoutStore`，会让 Store 变成杂物箱并增加无关重渲染。缓解措施是执行“跨组件共享才入 Store”的准入规则。

## Assumptions

- Phase 0 和 Phase 1 已完成，`createStore` 和 `threadStore` 可用。
- `DashboardWorkbench` 在 Phase 1 后已减少到约 30 个 `useState`。
- `OverlayType`、`PanelVisibilityState`、`DrawerPanel`、`DiffSelection`、`RunMode` 等类型已在现有代码中定义，可直接复用。
- 终端高度的 localStorage 持久化（如有）在 Store 初始化时读取，不需要额外的 useEffect。
