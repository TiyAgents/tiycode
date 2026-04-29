# Phase 0：基础设施 — createStore 工厂 + createMachine 工具

## Summary

为后续所有阶段提供两个核心基础设施：`createStore`（泛型 Store 工厂函数）和 `createMachine`（轻量有限状态机工厂）。这两个工具是整个重构的地基，所有 Domain Store 和显式状态机都将基于它们构建。本阶段不改动任何现有业务代码，仅新增 `src/shared/lib/` 下的工具模块和对应的单元测试。

## Context

### 项目现状

本项目是一个基于 Tauri + React + TypeScript 的桌面 AI Agent 应用。前端状态管理**完全依赖 React 原生 API**（`useState`/`useRef`/`useContext`/`useSyncExternalStore`），没有引入任何第三方状态库（无 Zustand/Redux/Jotai）。

当前唯一的全局外部 Store 是 `src/features/terminal/model/terminal-store.ts`（97 行），采用手写的发布-订阅模式 + `useSyncExternalStore` 接入 React。其核心结构为：

```
模块级 let state: T          ← 单例状态
const listeners = new Set()   ← 订阅者集合
function setState(next)       ← 更新 + 通知
export function useXxxStore(selector) ← useSyncExternalStore 封装
```

这个模式已在生产环境验证，设计简洁高效。但它是一次性手写的，没有抽象为可复用的工厂函数。

### 隐式状态机现状

项目中至少有 5 个隐式状态机散布在 `useEffect` 和 `useRef` 逻辑中：
- **Run 生命周期**：9 个状态，转换逻辑分散在 `ThreadStream.handleEvent`、`loadSnapshot`、全局 `thread-run-finished` 事件监听、`handleRuntimeThreadRunStateChange` 回调中。
- **Settings 加载阶段**：5 个阶段，用 `backendHydrated` boolean + try/catch 分支控制。
- **Sidebar 同步流控**：3 个状态，用 4 个 ref 手动实现。
- **工作区发现/绑定**：4 个阶段，用 ref guards + cancelled flag 实现。
- **线程删除确认**：4 个阶段，用 2 个 `useState` 表达。

这些状态机缺少显式的状态图定义和非法转换防护。

### 已有正向积累

- `terminalStore` 的手写 pub-sub 模式是 `createStore` 的直接原型。
- `useAppUpdater` hook 已用判别联合体（`UpdatePhase`）实现了一个显式状态机，证明团队有能力采用结构化方式。
- `runtime-thread-surface-state.ts` 中的 `TOOL_STATE_ORDER` 和 `MESSAGE_STATUS_ORDER` 数值表已经在用"阶数比较"驱动状态合并，这与状态机的"合法转换"理念一致。

## Design

### createStore 工厂

**设计目标**：将 `terminalStore` 的手写模式抽象为泛型工厂，使后续每个 Domain Store 只需定义状态形状和 action，不需要重复编写订阅/通知/React 集成的样板代码。

**API 设计**：

```typescript
const store = createStore<State>(initialState);
// 读取
store.getState(): State
// 更新（支持函数式和直接赋值）
store.setState(next: Partial<State> | (prev: State) => Partial<State>): void
// 重置为初始状态（主要用于测试隔离）
store.reset(): void
// React 集成（独立 Hook，遵循 React Hook 命名规范）
useStore<T>(store, selector: (s: State) => T, isEqual?: (a: T, b: T) => boolean): T  // 内部用 useSyncExternalStore
// 底层订阅（用于 Store 间联动或非 React 消费者）
store.subscribe(listener: () => void): () => void
```

> **命名规范说明**：React Hook 作为独立导出函数 `useStore(store, selector)` 而非 `store.use(selector)` 方法，确保：（1）eslint-plugin-react-hooks 能正确识别 Hook 调用规则（不能在条件分支或循环中调用）；（2）函数名以 `use` 前缀开头，符合 React 社区惯例；（3）消费者不会误将其当作普通方法在非组件上下文中调用。

**关键决策**：
1. **Partial 合并语义**：`setState` 接受 `Partial<State>`，内部做浅合并（`{ ...prev, ...next }`），与 Zustand 行为一致。深层嵌套更新由调用方负责构造完整的子对象。
2. **Selector + 引用相等 + 可选 isEqual**：`useStore(store, selector, isEqual?)` 内部用 `useSyncExternalStore` + selector。默认使用 `Object.is` 比较 selector 返回值，当返回值引用不变时不触发重渲染。传入 `isEqual`（如 `shallowEqual`）时，即使 selector 每次返回新引用（如 `.filter()`、对象 pick），只要内容相等也不触发重渲染。这是避免 inline selector 导致无限重渲染的关键安全网，与 Zustand 的 `useStoreWithEqualityFn` 行为一致。
3. **不内置中间件**：Phase 0 不加 devtools/persist/immer 等中间件能力，保持最小化。Phase 5 的 IPC 同步中间件作为外部组合使用。
4. **不内置 action 定义**：Store 只提供 `getState/setState/reset/subscribe`，具体的 action 函数由各 Domain Store 模块自行导出（与 `terminalStore` 现有模式一致）。
5. **`reset()` 用于测试隔离**：将 Store 状态重置为 `initialState` 并通知所有订阅者。主要用于单元测试中避免跨 test case 的状态泄漏，生产代码一般不调用。
6. **Listener 异常隔离**：`setState`/`reset` 通知订阅者时，逐个 try/catch 包裹每个 listener 回调。单个订阅者抛出异常不应中断其他订阅者的通知链，也不应导致白屏。异常统一通过 `console.error` 上报。

**与 Zustand 的对比**：API 几乎一致，但不引入依赖。如果未来需要 DevTools 支持，可以一行代码迁移到 Zustand（`create` → `createStore`，接口兼容）。

### createMachine 状态机工厂

**设计目标**：提供一个 ~60 行的泛型有限状态机，支持：
- 显式状态图定义（所有合法状态和转换一目了然）
- 非法转换静默忽略（可选 dev-mode 警告）
- 转换时的副作用 action
- `useSyncExternalStore` 集成
- 可选的 context（扩展数据，用于携带转换时的附加信息）

**API 设计**：

```typescript
const machine = createMachine<State, Event, Context>({
  initial: 'idle',
  context: {},  // 可选的扩展数据
  states: {
    idle: {
      on: {
        START: 'running',
        START_WITH_DATA: { target: 'running', action: (ctx, payload) => { ... } },
      },
    },
    running: { on: { COMPLETE: 'completed', FAIL: 'failed' } },
    completed: { on: { START: 'running' } },
    failed: { on: { START: 'running' } },
  },
});

machine.getState(): State
machine.getContext(): Context
machine.send(event: Event, payload?: any): void
machine.subscribe(listener: () => void): () => void
machine.reset(state?: State, context?: Context): void
machine.destroy(): void

// React Hooks（独立导出，遵循 Hook 命名规范）
useMachine(machine): State
useMachineContext(machine): Context
```

> **命名规范说明**：与 `useStore` 同理，React Hook 作为独立导出函数 `useMachine(machine)` 和 `useMachineContext(machine)`，而非 `machine.use()` / `machine.useContext()` 方法。这确保 eslint-plugin-react-hooks 能正确检测 Hook 调用位置，并避免开发者在条件分支中误用。

**关键决策**：
1. **不实现层级状态/并行状态**：当前 5 个隐式状态机都是扁平的，不需要 XState 级别的复杂度。如果未来需要，再引入 XState。
2. **Context 是可选的**：简单状态机（如删除确认流）不需要 context，复杂状态机（如 Run 生命周期）可以用 context 携带 `runId`、`errorMessage` 等附加数据。
3. **Action 是同步的**：异步副作用不在状态机内部处理，而是由外部监听状态变化后触发（通过 `subscribe` 或 React `useEffect`）。这保持了状态机的纯粹性和可测试性。
4. **`destroy()` 清空订阅者**：调用后清空所有 listener，标记实例为已销毁，后续 `send`/`reset` 被静默忽略。用于组件卸载时防止内存泄漏和僵尸更新。
5. **自转换时 context 变化仍通知订阅者**：当 `send` 的目标状态与当前相同（如 `running → running`）但 action 返回了新 context，状态机仍通知所有 listener——因为 `useMachineContext()` 消费者需要感知 context 变化。实现要求：`send` 内部必须同时比较 state 和 context：`const changed = !Object.is(prevState, nextState) || !Object.is(prevContext, nextContext)`；只有两者都不变时才跳过通知。**action 必须返回新 context 对象**（不可原地 mutate），否则 `Object.is` 无法检测变化。

## Key Implementation

### 文件结构

```
src/shared/lib/
├── create-store.ts          ← Store 工厂（~80 行）
├── create-machine.ts        ← 状态机工厂（~60 行）
├── create-store.test.ts     ← Store 单元测试
└── create-machine.test.ts   ← 状态机单元测试
```

### createStore 核心实现

```typescript
// src/shared/lib/create-store.ts
import { useSyncExternalStore, useRef, useCallback } from 'react';

export interface Store<S> {
  getState: () => S;
  setState: (next: Partial<S> | ((prev: S) => Partial<S>)) => void;
  subscribe: (listener: () => void) => () => void;
  reset: () => void;
}

export function createStore<S extends Record<string, unknown>>(
  initialState: S
): Store<S> {
  let state = initialState;
  const listeners = new Set<() => void>();

  const emitSafe = () => {
    listeners.forEach((l) => {
      try { l(); } catch (e) { console.error('[Store] listener threw:', e); }
    });
  };

  const getState = () => state;

  const setState = (next: Partial<S> | ((prev: S) => Partial<S>)) => {
    const partial = typeof next === 'function' ? next(state) : next;
    const nextState = { ...state, ...partial };
    if (Object.is(state, nextState)) return;
    state = nextState;
    emitSafe();
  };

  const subscribe = (listener: () => void) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };

  const reset = () => {
    state = initialState;
    emitSafe();
  };

  return { getState, setState, subscribe, reset };
}

/**
 * React Hook：从 Store 中订阅指定 selector 的值。
 * ⚠️ 这是一个 React Hook，必须在组件或自定义 Hook 顶层调用，不能在条件分支或循环中调用。
 *
 * @param isEqual - 可选的相等比较函数。默认使用 Object.is。
 *   传入 shallowEqual 可避免 selector 返回新引用（如 .filter()、对象 pick）时的不必要重渲染。
 *   这是避免 inline selector 导致 useSyncExternalStore 无限重渲染的关键安全网。
 *
 * @important Store 的 setState 采用浅合并语义（{ ...prev, ...next }），
 * 嵌套对象会被整体替换而非深合并。调用方需要自行构造完整的子对象。
 */
export function useStore<S extends Record<string, unknown>, T>(
  store: Store<S>,
  selector: (s: S) => T,
  isEqual: (a: T, b: T) => boolean = Object.is,
): T {
  // 用 useRef 缓存上一次的 selector 结果，配合 isEqual 实现稳定的 getSnapshot。
  // 注意：ref.current 仅在 getSnapshot 内部读写，不在渲染函数体中赋值，
  // 因此不违反 React Compiler 对渲染纯函数的假设。
  const prevRef = useRef<{ value: T } | null>(null);

  const getSnapshot = useCallback(() => {
    const next = selector(store.getState());
    if (prevRef.current !== null && isEqual(prevRef.current.value, next)) {
      return prevRef.current.value; // 引用稳定，避免重渲染
    }
    prevRef.current = { value: next };
    return next;
  }, [store, selector, isEqual]);

  return useSyncExternalStore(store.subscribe, getSnapshot);
}
```

**注意**：`useStore` 使用 `useRef` + `useCallback` + `isEqual` 组合实现稳定的 `getSnapshot`。`prevRef.current` 仅在 `getSnapshot` 回调内部（非渲染函数体）读写，不违反 React Compiler 对渲染纯函数的假设。`isEqual` 默认为 `Object.is`（零开销），传入 `shallowEqual` 时可以安全地使用 inline selector 和返回新对象的 selector。消费者仍应尽量使用稳定的 selector 引用（`useCallback` 或模块级函数）以减少 `getSnapshot` 重建，但不再因为 inline selector 而导致无限重渲染。

### createMachine 核心实现

```typescript
// src/shared/lib/create-machine.ts
import { useSyncExternalStore } from 'react';

export interface MachineConfig<S extends string, E extends string, C = void> {
  initial: S;
  context?: C;
  states: Record<S, {
    on?: Partial<Record<E, S | { target: S; action?: (ctx: C, payload?: unknown) => C | void }>>;
  }>;
}

export interface Machine<S extends string, E extends string, C = void> {
  getState: () => S;
  getContext: () => C;
  send: (event: E, payload?: unknown) => void;
  subscribe: (listener: () => void) => () => void;
  reset: (state?: S, context?: C) => void;
  destroy: () => void;
}

export function createMachine<S extends string, E extends string, C = void>(
  config: MachineConfig<S, E, C>
): Machine<S, E, C> {
  // ... 实现
}
```

状态机的 `send` 方法查找当前状态的 `on[event]` 定义，如果存在则执行转换并通知订阅者；如果不存在则静默忽略（开发模式下可选 `console.warn`）。`action` 回调可以返回新的 `context`（用于携带转换时的附加数据），也可以不返回（纯副作用）。

## Steps

1. **创建 `src/shared/lib/create-store.ts`**
   - 实现 `createStore<S>` 工厂函数
   - 包含 `getState`、`setState`（支持函数式更新）、`subscribe`、`reset`（重置为初始状态，用于测试隔离）
   - 实现独立导出的 `useStore<S, T>(store, selector, isEqual?)` React Hook（使用 `useRef` + `useCallback` + `isEqual` 稳定化 `getSnapshot`，默认 `Object.is`，可选 `shallowEqual`）
   - 导出 `Store<S>` 类型接口
   - 导出 `shallowEqual` 工具函数（用于对象/数组浅比较）
   - 文件：`src/shared/lib/create-store.ts`

2. **创建 `src/shared/lib/create-machine.ts`**
   - 实现 `createMachine<S, E, C>` 工厂函数
   - 包含 `getState`、`getContext`、`send`、`subscribe`、`reset`、`destroy`
   - 实现独立导出的 `useMachine(machine)` 和 `useMachineContext(machine)` React Hooks
   - 导出 `MachineConfig<S, E, C>` 和 `Machine<S, E, C>` 类型接口
   - 文件：`src/shared/lib/create-machine.ts`

3. **创建 `src/shared/lib/create-store.test.ts`**
   - 测试基本 getState/setState 读写
   - 测试函数式更新 `setState(prev => ...)`
   - 测试 Partial 浅合并语义（嵌套对象整体替换而非深合并）
   - 测试 subscribe/unsubscribe 通知机制
   - 测试 Object.is 相等性跳过（无变化不通知）
   - 测试 `reset()` 恢复初始状态并通知订阅者
   - 测试多个 test case 之间调用 `reset()` 实现状态隔离
   - 测试 listener 异常隔离：一个 listener 抛出异常不影响其他 listener 的通知
   - 测试 `useStore` 的 `isEqual` 参数：传入 `shallowEqual` 时，selector 返回新引用但内容相等不触发重渲染
   - 文件：`src/shared/lib/create-store.test.ts`

4. **创建 `src/shared/lib/create-machine.test.ts`**
   - 测试合法转换执行
   - 测试非法转换静默忽略
   - 测试 action 回调执行和 context 更新
   - 测试自转换时 context 变化通知订阅者（状态不变但 context 变）
   - 测试 reset 回到初始状态
   - 测试 destroy 清空订阅者并阻止后续 send/reset
   - 测试 subscribe 通知
   - 文件：`src/shared/lib/create-machine.test.ts`

5. **导出入口**
   - 在 `src/shared/lib/index.ts`（如存在）中添加 re-export，或确保各模块可通过 `@/shared/lib/create-store` 路径导入
   - 文件：`src/shared/lib/index.ts`（可选）

6. **`terminalStore` 不迁移为 `createStore` 实例**
   - `src/features/terminal/model/terminal-store.ts` 当前是独立手写的 pub-sub（97 行），与 `createStore` 语义等价
   - 迁移收益低（仅减少约 20 行样板代码），且不影响外部 API
   - 保持现有实现不变，避免引入不必要的变更风险

## Verification

1. **单元测试**：`npm run test:unit -- --run src/shared/lib/create-store.test.ts src/shared/lib/create-machine.test.ts`，所有用例通过。
2. **类型检查**：`npm run typecheck` 通过，无新增类型错误。
3. **零业务影响**：本阶段不修改任何现有文件，`git diff --stat` 应仅显示 `src/shared/lib/` 下的新增文件。
4. **手动验证**：在一个临时组件中调用 `useStore(store, s => s.xxx)` 和 `useMachine(machine)`，确认 React 渲染正常响应状态变化。
5. **React Compiler 兼容性**：启用 Compiler 后编译项目（或运行 `npx react-compiler-healthcheck`），确认 `useStore` 和 `useMachine`/`useMachineContext` 不会触发 Compiler 的 lint 错误或运行时异常。当前实现使用 `useRef` + `useCallback`（在回调内读写 ref），不在渲染函数体中进行 side effect，应与 Compiler 兼容。

## Risks

1. **`useSyncExternalStore` 的 selector 稳定性**：`useStore(store, selector, isEqual?)` 通过 `isEqual` 参数提供安全网。默认 `Object.is` 与 Zustand 行为一致；传入 `shallowEqual` 可兜底 inline selector 返回新引用的场景。即便如此，消费者仍应优先使用稳定的 selector 引用，因为每次 selector 变化会重建 `getSnapshot` 并触发一次 snapshot 计算。
2. **浅合并的局限性**：`setState({ nested: { a: 1 } })` 会完全替换 `nested` 对象而非深合并。这是有意为之（与 Zustand 一致），但需要在文档中明确说明，避免后续阶段的 Store 实现者误用。
3. **状态机 context 的不可变性**：`action` 回调中如果直接修改 `ctx` 对象而非返回新对象，会导致 `Object.is(prevContext, nextContext)` 为 true，订阅者不被通知。需要在 JSDoc 中明确要求 action 必须返回新对象。
4. **Listener 异常隔离的上报方式**：当前使用 `console.error` 上报 listener 异常，不会被 React Error Boundary 捕获。如果 listener 中的异常需要用户可见的降级展示，应在上报逻辑中集成应用级错误上报机制。

## Assumptions

- 项目已有 Vitest 配置，`npm run test:unit` 可正常运行。
- `useSyncExternalStore` 在当前 React 版本（18+）中可用。
- `@/shared/lib/` 路径别名已配置或可通过相对路径导入。
