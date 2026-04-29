# Phase 5：IPC 同步中间件 — 乐观更新 / 回滚 / 去重

## Summary

创建一个标准化的 IPC 同步中间件 `syncToBackend`，将当前散布在各 Store action 中的乐观更新、失败回滚、请求去重、错误标准化等横切关注点统一为声明式 API。本阶段是一个跨切面改进，完成后所有 Store 的 IPC 同步函数都可以从手动 try/catch 模式迁移为中间件驱动的声明式调用，减少约 60% 的同步样板代码。

## Context

### 前置依赖

- Phase 0：`createStore` 工厂（中间件需要与 Store 的 `getState/setState` 接口交互）
- Phase 4：`settingsStore` 和 `settings-ipc-actions.ts` 已实现手动乐观更新 + 回滚模式

### 当前 IPC 同步模式分析

项目中所有前端→后端的状态同步都遵循相似的模式，但每处都是手动实现：

**模式 A：乐观更新 + 无回滚**（Settings 当前模式）
```typescript
// settings-ipc-actions.ts (Phase 4 产出)
export async function updateProvider(id: string, patch: Partial<Provider>) {
  const prev = settingsStore.getState();
  settingsStore.setState({ providers: prev.providers.map(p => p.id === id ? {...p, ...patch} : p) });
  try {
    const updated = await providerUpdate(id, patch);
    settingsStore.setState({ providers: prev.providers.map(p => p.id === id ? updated : p) });
  } catch (error) {
    settingsStore.setState({ providers: prev.providers }); // Phase 4 新增的回滚
    console.warn('Failed:', error);
  }
}
```

**模式 B：先请求后更新**（Thread 操作）
```typescript
// 线程删除：先调 IPC，成功后再更新 Store
async function deleteThread(threadId: string) {
  await threadDelete(threadId);  // 先请求
  threadStore.removeThread(threadId);  // 成功后更新
}
```

**模式 C：请求合并 + 节流**（Sidebar 同步，Phase 3 优先用 runner 管理，状态机仅作备选）
```typescript
// 已由 sidebarSyncRunner/createCoalescedAsyncRunner 管理；若 runner 不足，再由 sidebarSyncMachine 备选处理
```

**共性问题**：
1. 每个 IPC action 都手动编写 snapshot → optimistic update → try/catch → rollback/correct 的完整流程
2. 错误处理不一致：有的 `console.warn`，有的 `console.error`，有的设置 error state，有的静默忽略
3. 无请求去重：快速连续点击同一个操作可能发出多个并发 IPC 调用
4. 无重试机制：IPC 失败后不会自动重试（依赖用户手动操作）
5. 错误格式不统一：Tauri 错误可能是 `{ message }` / `{ error }` / `{ description }` / 纯字符串

### 现有错误消息提取

`src/shared/lib/invoke-error.ts` 中已有 `getInvokeErrorMessage(error, fallback)`，并优先读取 `userMessage`。`src/services/thread-stream/thread-stream.ts` 另有私有 `extractErrorMessage`，但提取链不包含 `userMessage`。本阶段只要求把消息提取收敛到共享 formatter，并让 `syncToBackend` 与 ThreadStream 复用；不要求一次性替换所有 UI 层调用。

### 现有的去重/合并机制

- **Sidebar 同步**：Phase 3 优先通过 `sidebarSyncRunner` / `createCoalescedAsyncRunner` 实现 coalescing + throttling；只有 runner 不足时才使用 `sidebarSyncMachine` 备选
- **提交防重入**：`submittingRef` boolean guard（`runtime-thread-surface.tsx`）
- **订阅防重复**：`subscribingRef` boolean guard
- **快照版本化**：`snapshotLoadRequestRef` 单调递增版本号

这些都是手动实现的，没有统一的抽象。

## Design

### 中间件 API 设计

```typescript
// src/shared/lib/ipc-sync.ts

interface SyncOptions<TState, TResult> {
  // 乐观更新：在 IPC 调用前立即应用到 Store
  optimistic?: (state: TState) => Partial<TState>;

  // 成功后的状态纠正：用后端返回值替换乐观值
  onSuccess?: (state: TState, result: TResult) => Partial<TState>;

  // 失败时是否回滚到 snapshot（默认 true）
  rollback?: boolean;

  // 失败时的自定义处理（在回滚之后调用）
  onError?: (error: SyncError) => void;

  // 请求去重策略
  dedupe?: DedupeStrategy;
}

type DedupeStrategy =
  | 'none'           // 不去重（默认）
  | 'first'          // 保留第一个，忽略后续
  | 'last'           // 取消前一个，执行最新的
  | { key: string; strategy: 'first' | 'last' }; // 按 key 去重，对象形式必须显式指定 strategy

export class SyncError extends Error {
  constructor(
    public override readonly message: string,
    public readonly raw: unknown,
    public readonly aborted?: boolean,
  ) {
    super(message);
    this.name = 'SyncError';
  }
}
```

### 核心函数签名

```typescript
function syncToBackend<TState extends Record<string, unknown>, TResult>(
  store: Store<TState>,
  ipcCall: () => Promise<TResult>,
  options?: SyncOptions<TState, TResult>,
): Promise<TResult>;
```

### 执行流程

```
syncToBackend(store, ipcCall, options)
  │
  ├─ 1. Snapshot: prev = store.getState()
  │
  ├─ 2. Dedupe check: 如果有同 key 的飞行中请求，按策略处理
  │
  ├─ 3. Optimistic update: store.setState(options.optimistic(prev))
  │
  ├─ 4. Execute IPC: result = await ipcCall()
  │     │
  │     ├─ 成功 → 5a. Correct: store.setState(options.onSuccess(state, result))
  │     │         → 返回 result
  │     │
  │     └─ 失败 → 5b. Rollback: store.setState(rollbackPatch)
  │                    ⚠️ 字段级回滚：仅恢复 optimistic 涉及的字段
  │               → 6. onError(normalizedError)
  │               → throw normalizedError
  │
  └─ 8. 清理 dedupe 记录
```

### 去重实现

使用模块级 `Map<string, InFlightEntry>` 追踪飞行中的请求。`last` 策略不能在旧请求完成时通过 `inFlight.get(key)` 判断是否被取消，因为 key 可能已经指向新 entry；必须保存当前请求自己的 entry/token，并用 identity 判断结果是否仍然有效：

```typescript
interface InFlightEntry {
  token: symbol;
  aborted: boolean;
  promise?: Promise<unknown>;
}

const inFlightRequests = new Map<string, InFlightEntry>();

// dedupe: 'last' 策略
if (dedupeKey && inFlightRequests.has(dedupeKey)) {
  inFlightRequests.get(dedupeKey)!.aborted = true; // Tauri IPC 不能真正取消，只能忽略旧结果
}
const entry = { token: Symbol(dedupeKey), aborted: false };
inFlightRequests.set(dedupeKey, entry);

// 旧请求完成时只检查自己的 entry，而不是重新按 key 读取新 entry。
if (entry.aborted || inFlightRequests.get(dedupeKey) !== entry) {
  return result; // 静默返回，不更新 Store
}
```

对于 Tauri IPC 调用，"取消"意味着忽略返回值（Tauri 不支持真正的请求取消），通过 entry/token 标记让回调跳过状态更新。

### 错误标准化

将错误消息提取收敛到共享 formatter，而不是全面废弃现有 `getInvokeErrorMessage`。推荐把 `src/shared/lib/invoke-error.ts` 扩展为同时导出 `formatInvokeErrorMessage` 或 `normalizeIpcErrorMessage`，`syncToBackend` 和 `ThreadStream` 都复用同一个消息提取链（`userMessage > message > detail > description > error`）：

```typescript
function normalizeIpcError(raw: unknown, attempt: number, willRetry: boolean): SyncError {
  const message = formatInvokeErrorMessage(raw, 'Unknown IPC error');
  return new SyncError(message, raw, attempt, willRetry);
}
```

这统一了当前 `getInvokeErrorMessage`（`src/shared/lib/invoke-error.ts`，先检查 `userMessage`）和私有 `extractErrorMessage`（`src/services/thread-stream/thread-stream.ts`，不检查 `userMessage`）两条提取链，但保留 `getInvokeErrorMessage(error, fallback)` 作为 UI 调用方的兼容 API。

### 与现有 Store 的集成

**Settings IPC actions 改造**（Phase 4 产出 → Phase 5 简化）：

```typescript
// 之前（Phase 4，手动 try/catch）
export async function updateProvider(id: string, patch: Partial<Provider>) {
  const prev = settingsStore.getState();
  settingsStore.setState({ providers: prev.providers.map(p => p.id === id ? {...p, ...patch} : p) });
  try {
    const updated = await providerUpdate(id, patch);
    settingsStore.setState({ providers: prev.providers.map(p => p.id === id ? updated : p) });
  } catch (error) {
    settingsStore.setState({ providers: prev.providers });
    console.warn('Failed:', error);
  }
}

// 之后（Phase 5，声明式）
export function updateProvider(id: string, patch: Partial<Provider>) {
  return syncToBackend(settingsStore, () => providerUpdate(id, patch), {
    optimistic: (s) => ({
      providers: s.providers.map(p => p.id === id ? { ...p, ...patch } : p),
    }),
    onSuccess: (s, updated) => ({
      providers: s.providers.map(p => p.id === id ? updated : p),
    }),
    dedupe: { key: `provider:${id}`, strategy: 'last' },
  });
}
```

**Thread 操作改造**：

```typescript
// 线程删除（模式 B：先请求后更新）
export function deleteThread(threadId: string) {
  return syncToBackend(threadStore, () => threadDelete(threadId), {
    // 无 optimistic（等 IPC 成功后再更新）
    onSuccess: () => {
      // removeThread 是一个复合 action，不适合用 Partial 表达
      // 在 onSuccess 中手动调用
      threadStore.removeThread(threadId);
      return {};
    },
    dedupe: { key: `thread-delete:${threadId}`, strategy: 'first' },
  });
}
```

### 不适用中间件的场景

以下场景不适合用 `syncToBackend`，应保持现有模式：
- **流式操作**（`thread_start_run`）：通过 Channel 流推送，不是简单的请求/响应
- **Sidebar 同步**：已由 Phase 3 的 `sidebarSyncRunner` 管理；如果采用 FSM 备选，则由 `sidebarSyncMachine` 管理
- **终端 I/O**：实时双向流，不是状态同步

## Key Implementation

### 文件结构

```
src/shared/lib/
├── ipc-sync.ts              ← 中间件核心（~180 行）
├── ipc-sync.test.ts         ← 单元测试

src/modules/settings-center/model/
├── settings-ipc-actions.ts  ← 改造为声明式调用（从 ~400 行减至 ~250 行）

src/modules/workbench-shell/model/
├── thread-store.ts          ← 添加 syncToBackend 集成的 action
```

### ipc-sync 核心实现

```typescript
// src/shared/lib/ipc-sync.ts
import type { Store } from './create-store';

const inFlight = new Map<string, { token: symbol; aborted: boolean }>();

export async function syncToBackend<S extends Record<string, unknown>, R>(
  store: Store<S>,
  ipcCall: () => Promise<R>,
  options: SyncOptions<S, R> = {},
): Promise<R> {
  const { optimistic, onSuccess, rollback = true, onError, dedupe = 'none' } = options;

  // 1. Dedupe
  const dedupeKey = typeof dedupe === 'object' ? dedupe.key : dedupe !== 'none' ? dedupe : null;
  const dedupeStrategy = typeof dedupe === 'object' ? dedupe.strategy : dedupe !== 'none' ? dedupe : 'none';
  let entry: { token: symbol; aborted: boolean } | null = null;
  if (dedupeKey && dedupeStrategy !== 'none') {
    const existing = inFlight.get(dedupeKey);
    if (existing) {
      if (dedupeStrategy === 'first') {
        // 'first'：忽略后续请求
        return Promise.reject(new SyncError('Request superseded', null, 1, false, true));
      }
      existing.aborted = true; // 'last'：标记前一个为已取消，但后端请求仍可能完成
    }
    entry = { token: Symbol(dedupeKey), aborted: false };
    inFlight.set(dedupeKey, entry);
  }

  // 2. Snapshot + Optimistic
  const snapshot = store.getState();
  if (optimistic) {
    store.setState(optimistic(snapshot));
  }

  // 3. Execute
  try {
    const result = await ipcCall();

    // 检查是否已被取消（dedupe: 'last'）
    if (entry && dedupeKey && (entry.aborted || inFlight.get(dedupeKey) !== entry)) {
      return result; // 静默返回，不更新 Store；不要删除新 entry
    }

    // 4. Success correction
    if (onSuccess) {
      store.setState(onSuccess(store.getState(), result));
    }

    if (entry && dedupeKey && inFlight.get(dedupeKey) === entry) inFlight.delete(dedupeKey);
    return result;

  } catch (error) {
    const syncError = normalizeIpcError(error);

    // 被取消的请求（dedupe: 'last'）失败时不回滚，避免覆盖新请求的乐观状态
    if (entry && (entry.aborted || (dedupeKey && inFlight.get(dedupeKey) !== entry))) {
      if (entry && dedupeKey && inFlight.get(dedupeKey) === entry) inFlight.delete(dedupeKey);
      throw syncError;
    }

    // 5. Rollback (字段级：只恢复 optimistic 涉及的字段)
    // ⚠️ ABA 限制：如果飞行期间另一个操作修改了同一字段，回滚会覆盖该修改。
    // 对于高风险场景（如列表增删），使用 dedupe: { key, strategy: 'first' } 避免并发。
    if (rollback && optimistic) {
      const optimisticPatch = optimistic(snapshot);
      const rollbackPatch: Partial<S> = {};
      for (const key of Object.keys(optimisticPatch) as (keyof S)[]) {
        rollbackPatch[key] = snapshot[key];
      }
      store.setState(rollbackPatch);
    }

    // 6. Error callback
    onError?.(syncError);

    if (entry && dedupeKey && inFlight.get(dedupeKey) === entry) inFlight.delete(dedupeKey);
    throw syncError;
  }
}

function normalizeIpcError(raw: unknown): SyncError {
  const message = formatInvokeErrorMessage(raw, 'Unknown IPC error');
  return new SyncError(message, raw);
}
```

### Settings IPC Actions 改造示例

```typescript
// src/modules/settings-center/model/settings-ipc-actions.ts

// Provider CRUD
export const updateProvider = (id: string, patch: Partial<ProviderConfig>) =>
  syncToBackend(settingsStore, () => providerUpdate(id, patch), {
    optimistic: (s) => ({
      providers: s.providers.map(p => p.id === id ? { ...p, ...patch } : p),
    }),
    onSuccess: (s, updated) => ({
      providers: s.providers.map(p => p.id === id ? updated : p),
    }),
    dedupe: { key: `provider:${id}`, strategy: 'last' },
  });

export const removeProvider = (id: string) =>
  syncToBackend(settingsStore, () => providerDelete(id), {
    optimistic: (s) => ({
      providers: s.providers.filter(p => p.id !== id),
    }),
    // 无 onSuccess（删除操作不需要后端返回值纠正）
  });

// Agent Profile（含 ghost profile 升级路径）
export async function updateAgentProfile(id: string, patch: Partial<AgentProfile>) {
  try {
    return await syncToBackend(settingsStore, () => profileUpdate(id, patch), {
      optimistic: (s) => ({
        agentProfiles: s.agentProfiles.map(p => p.id === id ? { ...p, ...patch } : p),
      }),
      onSuccess: (s, updated) => ({
        agentProfiles: s.agentProfiles.map(p => p.id === id ? updated : p),
      }),
    });
  } catch (error) {
    // Ghost profile 升级路径：.not_found → create
    if (error instanceof SyncError && error.message.includes('not_found')) {
      const created = await profileCreate({ id, ...patch });
      settingsStore.setState((s) => ({
        agentProfiles: [...s.agentProfiles, created],
        activeAgentProfileId: created.id,
      }));
      return created;
    }
    throw error;
  }
}
```

## Steps

1. **创建 `ipc-sync.ts`**
   - 实现 `syncToBackend` 核心函数
   - 实现 `normalizeIpcError` 错误标准化
   - 实现去重逻辑（`inFlight` Map + `aborted` 标记）
   - 被取消请求（dedupe: 'last'）失败时跳过回滚（防止覆盖新请求的乐观状态）
   - 导出 `SyncOptions`、`SyncError`、`DedupeStrategy` 类型
   - 不实现重试机制（YAGNI，Tauri IPC 失败通常是应用级错误）
   - 文件：`src/shared/lib/ipc-sync.ts`

2. **编写 `ipc-sync.test.ts`**
   - 测试乐观更新 + 成功纠正
   - 测试乐观更新 + 失败回滚
   - 测试无乐观更新（模式 B）
   - 测试去重 `first`（忽略后续）
   - 测试去重 `last`（旧请求完成时因 entry/token 失效而不更新 Store，且不会删除新 entry）
   - 测试去重 `last` 被取消请求失败时不回滚（关键安全测试）
   - 测试去重 `key`（同 key 合并）
   - 测试错误标准化（各种 Tauri 错误格式）
   - 文件：`src/shared/lib/ipc-sync.test.ts`

3. **改造 `settings-ipc-actions.ts`**
   - 将约 20 个 IPC action 从手动 try/catch 改为 `syncToBackend` 声明式调用
   - 保留 `updateAgentProfile` 的 ghost profile 升级路径（在 catch 中处理）
   - 为高频操作添加 `dedupe: { key, strategy: 'last' }` 或 `strategy: 'first'` 配置，禁止省略策略
   - 文件：`src/modules/settings-center/model/settings-ipc-actions.ts`

4. **改造 `thread-store.ts` 的 IPC action**
   - 线程删除、线程重命名等操作改为 `syncToBackend` 调用
   - 文件：`src/modules/workbench-shell/model/thread-store.ts`

5. **统一错误处理策略**
   - 定义全局的 `onError` 默认处理（`console.warn` + 可选的 toast 通知）
   - 在 `syncToBackend` 中支持全局默认 `onError`（可被单次调用覆盖）
   - 文件：`src/shared/lib/ipc-sync.ts`

6. **统一错误消息提取**
   - 扩展 `src/shared/lib/invoke-error.ts`，保留 `getInvokeErrorMessage(error, fallback)` 兼容 API，并新增可复用的 `formatInvokeErrorMessage`
   - `normalizeIpcError` 调用共享 formatter；`ThreadStream` 删除私有 `extractErrorMessage` 或改为委托共享 formatter
   - 更新 thread-stream 中的调用点，不要求一次性替换所有 UI 层 `getInvokeErrorMessage` 调用
   - 文件：`src/shared/lib/invoke-error.ts`、`src/services/thread-stream/thread-stream.ts`

## Verification

1. **单元测试**：`npm run test:unit -- --run src/shared/lib/ipc-sync.test.ts`，所有用例通过。
2. **类型检查**：`npm run typecheck` 通过。
3. **手动验证**：
   - 修改 Provider → 乐观更新即时反映 → 刷新后持久化（与 Phase 4 行为一致）
   - 快速连续点击同一个 Provider 的保存按钮 → 只发出一次 IPC 调用（去重生效）
   - 断网时修改 Settings → 乐观更新显示 → 回滚到修改前状态 → 错误提示
   - 删除线程 → IPC 成功后 Store 更新 → Sidebar 移除线程
4. **回归验证**：确认所有 Settings 操作（Provider CRUD、Profile CRUD、Policy 更新、Prompt Command 更新）的行为与 Phase 4 一致，无功能退化。

## Risks

1. **Tauri IPC 不支持真正的请求取消**：`dedupe: 'last'` 策略只能标记前一个请求为“已取消”并忽略其返回值，但后端仍会执行该请求。对于幂等操作（如 update）这通常可接受，但对于非幂等操作（如 create/delete）可能导致重复创建或误删。缓解措施：非幂等操作必须使用 `dedupe: { key, strategy: 'first' }` 或不去重，禁止使用 `'last'`；对象形式的 dedupe 必须显式写出 `strategy`。
2. **回滚的 snapshot 存在 ABA 问题**：如果在 IPC 飞行期间有其他操作修改了 Store 中的同一字段，回滚会将该字段恢复到 snapshot 时刻的值，覆盖中间的修改。示例：操作A snapshot `providers=[1,2]` → 乐观 `[1,2,3]` → 操作B 将 `providers` 改为 `[1,2,3,4]` → 操作A 失败回滚 `providers=[1,2]`，操作B 的修改丢失。缓解措施：（1）`optimistic` 函数只返回受影响的字段（`Partial<State>`），回滚只恢复这些字段；（2）对高风险的并发场景（如 Provider 列表增删），使用 `dedupe: { key: 'providers', strategy: 'first' }` 避免并发修改同一字段；（3）在文档中明确此限制，由调用方根据业务场景选择合适的 dedupe 策略。
3. **`SyncError` 类型与现有错误处理的兼容性**：引入 `SyncError` 后，catch 块中的错误类型从 `unknown` 变为 `SyncError`。需要确保所有调用方正确处理新的错误类型。

## Assumptions

- Phase 0-4 已完成，所有 Domain Store 和 IPC action 函数已就位。
- Tauri IPC 调用是幂等的（对于 update/delete 操作），或者调用方会选择合适的去重策略。
- 项目不需要离线队列（offline queue）功能——IPC 失败后不会缓存请求等待重连。如果未来需要，可以在中间件中扩展。
- `ThreadStream` 私有错误提取应委托共享 formatter；Bridge 或 UI 层现有 `getInvokeErrorMessage(error, fallback)` 兼容 API 不要求一次性替换。
