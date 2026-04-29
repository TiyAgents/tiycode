# Phase 4：settingsStore — 从 Hook 迁移为 Store + 加载状态机

## Summary

将当前 1486 行的 `useSettingsController` 巨型 Hook 拆分为三个独立关注点：`settingsStore`（纯数据 Store）、`settingsHydrationMachine`（加载阶段状态机）、以及一组独立的 IPC 同步函数。这是工作量最大的单阶段改造，完成后将彻底解耦 Settings 的数据持有、加载生命周期和后端同步三个正交关注点，并使 `DashboardOverlays` 的剩余 ~30 个 settings props 大幅精简。

## Context

### 前置依赖

- Phase 0：`createStore` 和 `createMachine` 工厂
- Phase 1-2：`threadStore`、`uiLayoutStore`、`composerStore` 已抽取
- Phase 3：`createMachine` 模式已在 Run 生命周期和 Sidebar 同步中验证

### useSettingsController 现状分析

**文件**：`src/modules/settings-center/model/use-settings-controller.ts`（1486 行）

这是一个巨型自定义 Hook，同时承担三个职责：

**职责 ①：数据持有**
- 用 `useState<SettingsState>` 持有完整的设置状态树
- `SettingsState` 包含：`providers`、`agentProfiles`、`activeAgentProfileId`、`workspaces`、`general`（UI 偏好）、`terminal`（终端配置）、`policies`、`promptCommands`、`availableShells`、`modelCatalog` 等
- 用 `settingsRef = useRef(settings)` 镜像，供回调函数读取最新值

**职责 ②：加载生命周期**
- 分两阶段从 Tauri IPC 加载数据：
  - Phase 1（关键路径）：并发加载 `providers`、`workspaces`、`activeAgentProfileId`，完成后立即渲染
  - Phase 2（延迟，`requestIdleCallback` 或 50ms timeout）：加载 `modelCatalog`、`policies`、`agentProfiles`、`promptCommands`、`availableShells`
- 用 `backendHydrated` boolean 标记是否完成
- 加载失败时 `console.warn` + 降级到默认值

**职责 ③：IPC 同步**
- 每个设置项的变更都有对应的 IPC 同步函数（`updateProvider`、`removeProvider`、`updateAgentProfile`、`removeAgentProfile`、`updatePolicy`、`updatePromptCommand` 等约 20 个）
- 采用乐观更新模式：先 `setSettings(...)` → 再 `invoke(...)` → 成功后用后端返回值纠正 → 失败仅 `console.warn`
- `updateAgentProfile` 有特殊的 ghost profile → 真实 DB 记录升级路径

**问题**：
1. 1486 行的 Hook 认知负荷极高，修改任何一个设置项都需要理解整个文件
2. `settingsRef` 镜像模式（Phase 0 分析中的 Ref-Mirror 反模式）
3. 加载阶段是隐式状态机（`uninitialized → loading_phase1 → phase1_ready → loading_phase2 → hydrated`），用 boolean 控制
4. 乐观更新无回滚机制（IPC 失败时本地状态与后端不一致）
5. Hook 返回的 `SettingsController` 对象包含 ~30 个方法，全部通过 props 传递给 `DashboardOverlays`

### Settings 数据流

```
useSettingsController (Hook, 在 DashboardWorkbench 中调用)
  ├── useState<SettingsState>     ← 完整设置树
  ├── useEffect[] (hydration)     ← 两阶段加载
  ├── useEffect[] (persist)       ← localStorage 持久化 (general/terminal)
  ├── useEffect[] (sync)          ← 特定字段同步到后端
  ├── 20+ update/remove 函数      ← 乐观更新 + IPC
  └── 返回 SettingsController     ← 通过 props 传递给子组件
```

### DashboardOverlays 的 Settings Props

Phase 2 完成后，`DashboardOverlays` 仍有约 30 个 settings 相关 props：
- `settings`（完整 SettingsState 对象）
- `updateProvider`、`removeProvider`、`addProvider`
- `updateAgentProfile`、`removeAgentProfile`、`setActiveAgentProfileId`
- `updatePolicy`、`updatePromptCommand`、`removePromptCommand`
- `updateGeneralSettings`、`updateTerminalSettings`
- 等约 20 个回调函数

## Design

### 架构拆分

将 `useSettingsController` 的三个职责拆分为三个独立模块：

```
useSettingsController (1486 行, 单一 Hook)
        ↓ 拆分为 ↓
┌─────────────────────────────────────────┐
│ settingsStore (数据 Store, ~200 行)      │ ← createStore
│ - 纯数据持有                              │
│ - selector 订阅                          │
│ - 简单的 setState action                 │
│ - hydrationPhase 字符串枚举               │
├─────────────────────────────────────────┤
│ settings-hydration.ts (~180 行)          │ ← async 函数 + phase 枚举
│ - 两阶段加载逻辑                          │
│ - single-flight 防重入                   │
│ - hydrationPhase 状态管理                │
│ - 不使用 createMachine（线性加载不需要 FSM）│
├─────────────────────────────────────────┤
│ settings-ipc-actions.ts (~400 行)        │ ← 独立函数模块
│ - 20+ 个 IPC 同步函数                    │
│ - 乐观更新 + 回滚（Phase 5 集成）         │
│ - 每个函数读 settingsStore.getState()    │
│ - 每个函数写 settingsStore.setState()    │
├─────────────────────────────────────────┤
│ settings-persistence.ts (~60 行)         │ ← localStorage 持久化
│ - general/terminal 的 localStorage 读写  │
│ - subscribe settingsStore 自动持久化     │
└─────────────────────────────────────────┘
```

### settingsStore 状态形状

```typescript
interface SettingsStoreState {
  // 核心数据
  providers: ProviderConfig[];
  agentProfiles: AgentProfile[];
  activeAgentProfileId: string | null;
  workspaces: WorkspaceConfig[];
  modelCatalog: ModelCatalogEntry[];

  // UI 偏好（localStorage 持久化）
  general: GeneralSettings;
  terminal: TerminalSettings;

  // 策略与命令
  policies: PolicyConfig[];
  promptCommands: PromptCommand[];
  availableShells: string[];

  // 加载阶段（由 hydration machine 驱动）
  hydrationPhase: HydrationPhase;
}

type HydrationPhase =
  | 'uninitialized'
  | 'loading_phase1'
  | 'phase1_ready'
  | 'loading_phase2'
  | 'hydrated'
  | 'error';
```

### settingsHydrationPhase（字符串枚举，不使用 FSM）

Settings 加载是一个线性的两阶段流程，不存在复杂的分支转换或并发状态。用 `createMachine` 会引入不必要的间接层，用字符串枚举 + async 函数足以清晰表达：

```typescript
type HydrationPhase =
  | 'uninitialized'
  | 'loading_phase1'
  | 'phase1_ready'
  | 'loading_phase2'
  | 'hydrated'
  | 'error';
```

转换逻辑在 `settings-hydration.ts` 的 async 函数中线性表达，直接调用 `settingsStore.setState({ hydrationPhase: '...' })` 推进阶段。Phase 2 失败时直接设置 `hydrated`（降级处理），与当前行为一致。只有 Phase 1 失败才进入 `error`。

状态机的状态变化自动同步到 `settingsStore.hydrationPhase`，UI 组件可以根据 `hydrationPhase` 渲染不同的 loading 骨架：
- `uninitialized` / `loading_phase1`：全屏 loading
- `phase1_ready` / `loading_phase2`：核心 UI 可用，次要区域 skeleton
- `hydrated`：完全可用
- `error`：错误提示 + 重试按钮

### IPC Action 函数设计

每个 IPC action 函数是一个独立的导出函数，不依赖 React Hook，直接读写 `settingsStore`：

```typescript
// src/modules/settings-center/model/settings-ipc-actions.ts

export async function updateProvider(providerId: string, patch: Partial<ProviderConfig>) {
  const prev = settingsStore.getState();
  // 乐观更新
  settingsStore.setState({
    providers: prev.providers.map(p =>
      p.id === providerId ? { ...p, ...patch } : p
    ),
  });
  try {
    const updated = await providerUpdate(providerId, patch);
    // 用后端返回值纠正
    settingsStore.setState({
      providers: prev.providers.map(p =>
        p.id === providerId ? updated : p
      ),
    });
  } catch (error) {
    // 回滚（Phase 5 的 IPC 同步中间件将标准化此逻辑）
    settingsStore.setState({ providers: prev.providers });
    console.warn('Failed to update provider:', error);
  }
}
```

**与 Phase 5 的关系**：本阶段先用手动的 try/catch + 回滚实现乐观更新。Phase 5 引入 `syncToBackend` 中间件后，可以将这些函数简化为声明式调用。

### DashboardOverlays 改造

Settings 相关 props 从 ~30 个降为 0 个——`DashboardOverlays` 内部的各 Settings Panel 直接订阅 `settingsStore` 并调用 IPC action 函数：

```typescript
// 之前
<SettingsPanel
  providers={settings.providers}
  onUpdateProvider={updateProvider}
  onRemoveProvider={removeProvider}
  agentProfiles={settings.agentProfiles}
  onUpdateAgentProfile={updateAgentProfile}
  // ... 20+ props
/>

// 之后
function SettingsPanel() {
  const providers = useStore(settingsStore, s => s.providers);
  const profiles = useStore(settingsStore, s => s.agentProfiles);
  // 直接调用 action
  const handleUpdate = (id, patch) => updateProvider(id, patch);
}
```

### localStorage 持久化

当前 `useSettingsController` 中有一个 `useEffect` 监听 `settings.general` 和 `settings.terminal` 变化，写入 localStorage。迁移后改为 Store 的 `subscribe`，但订阅初始化必须只执行一次，避免多个消费者挂载时重复注册、重复写 localStorage。

```typescript
// src/modules/settings-center/model/settings-persistence.ts
let persistenceInitialized = false;

export function initializeSettingsPersistenceOnce() {
  if (persistenceInitialized) return;
  persistenceInitialized = true;
  // 注册 settingsStore.subscribe
}
```

订阅主体如下：

```typescript
// src/modules/settings-center/model/settings-persistence.ts
import { settingsStore } from './settings-store';

let prevGeneral = settingsStore.getState().general;
let prevTerminal = settingsStore.getState().terminal;

settingsStore.subscribe(() => {
  const { general, terminal, hydrationPhase } = settingsStore.getState();
  // 关键防护：hydration 完成前不写入 localStorage，避免默认值覆盖用户数据
  if (hydrationPhase !== 'hydrated' && hydrationPhase !== 'phase1_ready') return;

  if (general !== prevGeneral || terminal !== prevTerminal) {
    prevGeneral = general;
    prevTerminal = terminal;
    persistLocalUiSettings({ general, terminal });
  }
});
```

### settingsRef 镜像消除

当前 `settingsRef = useRef(settings)` 用于让回调函数读取最新值。迁移到 Store 后，所有回调函数通过 `settingsStore.getState()` 读取最新值，不再需要 ref 镜像。

## Key Implementation

### 文件结构

```
src/modules/settings-center/model/
├── settings-store.ts              ← Store 定义（~200 行）
├── settings-ipc-actions.ts        ← IPC 同步函数（~400 行）
├── settings-persistence.ts        ← localStorage 持久化（~60 行）
├── settings-hydration.ts          ← 两阶段加载逻辑 + hydrationPhase 管理（~180 行，从 Hook 提取）
├── settings-store.test.ts         ← 单元测试
├── settings-hydration.test.ts     ← 单元测试
├── use-settings-controller.ts     ← 保留为薄包装（~50 行），逐步废弃
├── defaults.ts                    ← 不变
├── settings-storage.ts            ← 不变
└── types.ts                       ← 不变
```

### settingsStore 核心实现

```typescript
// src/modules/settings-center/model/settings-store.ts
import { createStore } from '@/shared/lib/create-store';
import { defaultGeneralSettings, defaultTerminalSettings } from './defaults';

export const settingsStore = createStore<SettingsStoreState>({
  providers: [],
  agentProfiles: [],
  activeAgentProfileId: null,
  workspaces: [],
  modelCatalog: [],
  general: defaultGeneralSettings,
  terminal: defaultTerminalSettings,
  policies: [],
  promptCommands: [],
  availableShells: [],
  hydrationPhase: 'uninitialized',
});
```

### 两阶段加载逻辑与 single-flight 防重入

Settings hydration 必须是模块级 single-flight。`useSettingsController` 薄包装可能被多个消费者挂载，HMR 或未来页面拆分也可能重复调用 `hydrateSettings()`；重复 hydration 会造成多次 IPC、状态覆盖和 localStorage 持久化时序问题。因此 `settings-hydration.ts` 需要维护模块级 `hydratePromise`，已在执行中的 hydration 直接复用同一个 Promise。

```typescript
// src/modules/settings-center/model/settings-hydration.ts
import { settingsStore } from './settings-store';

let hydratePromise: Promise<void> | null = null;

export function hydrateSettingsOnce(): Promise<void> {
  const phase = settingsStore.getState().hydrationPhase;
  if (phase === 'hydrated' || phase === 'loading_phase1' || phase === 'loading_phase2') {
    return hydratePromise ?? Promise.resolve();
  }
  hydratePromise = hydrateSettings().finally(() => {
    hydratePromise = null;
  });
  return hydratePromise;
}

async function hydrateSettings() {
  // 先从 localStorage 恢复 UI 设置
  const localUi = loadLocalUiSettings();
  if (localUi) {
    settingsStore.setState({ general: localUi.general, terminal: localUi.terminal });
  }

  // Phase 1：关键路径
  settingsStore.setState({ hydrationPhase: 'loading_phase1' });
  try {
    const [providers, workspaces, activeProfileSetting] = await Promise.all([
      providerList(),
      workspaceList(),
      settingsGet('active_agent_profile_id'),
    ]);
    settingsStore.setState({ providers, workspaces, activeAgentProfileId: activeProfileSetting, hydrationPhase: 'phase1_ready' });
  } catch (error) {
    console.error('Settings phase 1 failed:', error);
    settingsStore.setState({ hydrationPhase: 'error' });
    return;
  }

  // Phase 2：延迟加载
  const startPhase2 = () => {
    settingsStore.setState({ hydrationPhase: 'loading_phase2' });
    Promise.all([
      modelCatalogList(),
      policyList(),
      profileList(),
      promptCommandList(),
      shellList(),
    ]).then(([catalog, policies, profiles, commands, shells]) => {
      settingsStore.setState({
        modelCatalog: catalog, policies, agentProfiles: profiles,
        promptCommands: commands, availableShells: shells,
        hydrationPhase: 'hydrated',
      });
    }).catch((error) => {
      console.warn('Settings phase 2 failed (non-critical):', error);
      settingsStore.setState({ hydrationPhase: 'hydrated' }); // 降级
    });
  };

  if ('requestIdleCallback' in window) {
    requestIdleCallback(startPhase2);
  } else {
    setTimeout(startPhase2, 50);
  }
}
```

### useSettingsController 薄包装（过渡期）

```typescript
// src/modules/settings-center/model/use-settings-controller.ts（精简为 ~50 行）
// 过渡期保留，供尚未迁移的组件使用

export function useSettingsController(): SettingsController {
  const settings = useStore(settingsStore, s => s);
  const hydrationPhase = useStore(settingsStore, s => s.hydrationPhase);

  // 首次挂载时触发加载
  useEffect(() => {
    initializeSettingsPersistenceOnce();
    if (hydrationPhase === 'uninitialized' || hydrationPhase === 'error') {
      void hydrateSettingsOnce();
    }
  }, []);

  return {
    settings,
    backendHydrated: hydrationPhase === 'hydrated' || hydrationPhase === 'phase1_ready',
    // 直接导出 IPC action 函数
    updateProvider,
    removeProvider,
    updateAgentProfile,
    // ... 其余 action
  };
}
```

## Steps

1. **创建 `settings-store.ts`**
   - 定义 `SettingsStoreState` 接口（含 `hydrationPhase`）
   - 使用 `createStore` 创建 `settingsStore` 实例
   - 导出 Store 和基本 setter action
   - 文件：`src/modules/settings-center/model/settings-store.ts`

2. **创建 `settings-hydration.ts`**
   - 从 `useSettingsController` 的 `hydrateDbBackedSettings` effect 中提取两阶段加载逻辑
   - 实现 `hydrateSettingsOnce()` 作为对外入口，内部用模块级 `hydratePromise` 保证 single-flight
   - 通过 `settingsStore.setState({ hydrationPhase: '...' })` 直接推进阶段，不使用 `createMachine`
   - 文件：`src/modules/settings-center/model/settings-hydration.ts`

3. **创建 `settings-ipc-actions.ts`**
   - 从 `useSettingsController` 中提取所有 IPC 同步函数（约 20 个）
   - 每个函数改为读写 `settingsStore`（替代 `setSettings` + `settingsRef`）
   - 实现手动乐观更新 + 回滚（try/catch）
   - 文件：`src/modules/settings-center/model/settings-ipc-actions.ts`

4. **创建 `settings-persistence.ts`**
   - 实现 `settingsStore.subscribe` 监听 `general`/`terminal` 变化
   - 导出 `initializeSettingsPersistenceOnce()`，确保订阅只注册一次
   - 写入 localStorage（复用现有 `persistLocalUiSettings` 函数）
   - 在应用启动或 `useSettingsController` 首次挂载时调用（在 `hydrateSettingsOnce` 之前）
   - 文件：`src/modules/settings-center/model/settings-persistence.ts`

5. **精简 `use-settings-controller.ts`**
   - 从 1486 行缩减为 ~50 行的薄包装
   - 内部委托给 `settingsStore`、`hydrateSettings`、IPC action 函数
   - 保持返回类型 `SettingsController` 不变（向后兼容）
   - 文件：`src/modules/settings-center/model/use-settings-controller.ts`

6. **改造 DashboardOverlays 直接订阅 settingsStore**
   - 各 Settings Panel 组件从 props 改为直接 `useStore(settingsStore, selector)` + 调用 IPC action
   - `DashboardOverlays` 的 settings 相关 props 从 ~30 个降为 0
   - 文件：`src/modules/workbench-shell/ui/dashboard-overlays.tsx` 及其子组件

7. **改造 DashboardWorkbench 移除 settings 传递**
   - 删除 `useSettingsController()` 调用（或保留但不再传递返回值给子组件）
   - 删除向 `DashboardOverlays` 传递的 settings props
   - 文件：`src/modules/workbench-shell/ui/dashboard-workbench.tsx`

8. **编写单元测试**
   - `settings-store.test.ts`：测试基本读写、`hydrationPhase` 状态
   - `settings-hydration.test.ts`：测试两阶段加载（mock IPC）、phase2 失败降级、重试、多个消费者同时调用时只发起一次 IPC
   - 文件：对应的 `.test.ts` 文件

## Verification

1. **单元测试**：`npm run test:unit -- --run src/modules/settings-center/model/settings-store.test.ts src/modules/settings-center/model/settings-hydration.test.ts`
2. **类型检查**：`npm run typecheck` 通过。重点关注 `SettingsController` 返回类型的向后兼容性。
3. **手动验证**：
   - 应用启动 → Settings 正常加载（phase1 → phase2），无 loading 卡死
   - 同时挂载两个 `useSettingsController` 消费者 → 只触发一次 hydration IPC 批次，localStorage 订阅只注册一次
   - 修改 Provider → 乐观更新即时反映 → 刷新后持久化
   - 修改 Agent Profile → 正常保存，ghost profile 升级路径正常
   - 修改 General Settings（主题、语言）→ localStorage 持久化正常
   - 断网时修改 Settings → 乐观更新显示 → 重连后状态正确（或回滚）
   - Settings overlay 打开/关闭 → 无性能退化
4. **`SETTINGS_STORAGE_SCHEMA_VERSION` 验证**：确认 localStorage 的 UI 设置持久化逻辑与 `defaults.ts` 中的版本控制机制保持一致。

## Risks

1. **迁移规模大**：1486 行 Hook 的拆分涉及约 20 个 IPC 函数的逐一迁移，每个都有微妙的乐观更新逻辑。建议分批迁移：先迁移数据持有（Store）和加载（hydration），再逐步迁移 IPC action。
2. **`updateAgentProfile` 的 ghost profile 升级路径**：这是最复杂的单个 action，包含 `.not_found` 错误时的 create 升级逻辑和 `activeAgentProfileId` 联动更新。迁移时需要完整保留此逻辑。
3. **`useSettingsController` 的消费者**：除了 `DashboardWorkbench`，可能还有其他组件通过 Context 或 props 消费 `SettingsController`。需要全局搜索所有消费点，确保薄包装的向后兼容性；多个消费者同时挂载时必须复用 `hydrateSettingsOnce()` 的同一个 Promise。
4. **localStorage 持久化的时序**：当前 `useEffect` 在 `backendHydrated` 为 true 后才开始持久化。迁移到 `subscribe` 后，需要确保在 hydration 完成前不会将默认值写入 localStorage 覆盖用户数据，并通过 `initializeSettingsPersistenceOnce()` 避免重复订阅。
5. **`SETTINGS_STORAGE_SCHEMA_VERSION`**：根据 AGENTS.md 的规定，如果本次改造改变了 `general` 或 `terminal` 的 localStorage 数据形状，必须递增此版本号。如果仅改变了代码组织而不改变数据形状，则不需要递增。

## Assumptions

- Phase 0-3 已完成，`createStore`、`createMachine` 和 Domain Store 模式已验证。
- `SettingsState` 的类型定义（`src/modules/settings-center/model/types.ts`）不需要大幅修改。
- `DashboardOverlays` 内部的各 Settings Panel 组件可以独立改造（不需要一次性全部迁移）。
- `settings-storage.ts` 中的 `persistLocalUiSettings` 和 `loadLocalUiSettings` 函数可直接复用。
