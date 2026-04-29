# 状态管理架构重构总览

> **目标**：将 DashboardWorkbench 从 3100 行 / 39 useState / 18 useRef / 21 useEffect 的上帝组件，重构为以编排为主、少持有跨域业务状态的工作台入口；消除三源状态不一致、Ref-Mirror 反模式、隐式状态机和深层 Props Drilling，建立可扩展、可维护的 Domain Store + 显式状态机架构。
>
> **非目标**：不机械追求 `0 useState` 或把所有交互状态全局 Store 化。焦点、光标位置、拖拽中间态、一次性菜单开关、DOM ref 等严格局部状态可以继续保留在组件或专用 hook 中。

---

## 一、现状问题摘要

### 1.1 上帝组件

`DashboardWorkbench`（3100 行）用 **39 个 `useState`** 集中持有应用核心状态，向子组件传递最多 **97 个 props**（`DashboardOverlays`），是所有状态变更的必经之路。任何一个 `setState` 都可能触发整棵子树重渲染。

### 1.2 三源状态不一致

同一个"线程运行状态"在三处独立维护：

| 位置 | 数据源 | 类型 |
|------|--------|------|
| Sidebar（`workspaces[].threads[].status`） | Tauri 全局 `emit` 事件 | `WorkbenchThreadStatus` |
| Surface（`runState` useState） | `ThreadStream` Channel 流 | `RunState` |
| Workbench（`pendingThreadRuns`） | 本地乐观写入 | `PendingThreadRun` |

事件到达时序差异会导致 Sidebar 显示 "running" 而 Surface 已 "idle"。

### 1.3 Ref-Mirror 反模式

项目中至少 **10 处** `useState + useRef` 双写镜像，用于绕过 React 闭包的 stale capture 问题。这使得函数的实际依赖对 React、ESLint `exhaustive-deps` 和 React Compiler 完全不透明。此外，组件内还有 **18 个 `useRef`** 和 **21 个 `useEffect`**，大量副作用逻辑紧耦合在组件体内。

### 1.4 隐式状态机

至少 **5 个隐式状态机**散布在 `useEffect` 和 `useRef` 逻辑中（Run 生命周期 9 态、Settings 加载 5 阶段、Sidebar 同步 3 态、工作区发现 4 阶段、线程删除 4 阶段），缺少显式状态图和非法转换防护。

### 1.5 乐观更新无回滚 + 错误标准化缺失

Settings 同步采用乐观更新，IPC 失败仅 `console.warn`，不回滚本地状态。无请求去重、无重试。此外错误提取逻辑分裂为两处独立实现（`getInvokeErrorMessage` 在 `shared/lib/invoke-error.ts`，私有 `extractErrorMessage` 在 `thread-stream.ts`），且提取链不一致（前者先检查 `userMessage`，后者不检查），缺少统一的错误标准化入口。

---

## 二、已有正向积累

重构方案建立在以下已验证的设计之上，保持延续而非推翻：

1. **`terminalStore`**（手写 pub-sub + `useSyncExternalStore`，97 行）— `createStore` 工厂的直接原型
2. **纯函数逻辑层**（`runtime-thread-surface-state.ts` 681 行，~30 个纯函数）— 所有消息/工具状态合并逻辑已从组件中剥离
3. **快照与流式状态合并策略**（`mergeSnapshotTools`、`MESSAGE_STATUS_ORDER` 阶数比较）— 精心设计的"不回退"语义
4. **`useAppUpdater`** hook 的字符串状态枚举 `UpdatePhase` — 已验证的显式阶段管理模式（注意：当前 `phase`、`updateInfo`、`downloadProgress`、`errorMessage` 是分散的多个 `useState`，尚未形成完整的判别联合体，但阶段枚举方向正确）
5. **Rust 后端并发模型** — `tokio::Mutex` 短暂持锁 + `broadcast/mpsc/watch/oneshot` 通道组合，清晰规范

---

## 三、目标架构

```
┌───────────────────────────────────────────────────────────┐
│                    Domain Stores                          │
│  ┌────────────┐ ┌────────────┐ ┌───────────────────────┐  │
│  │ threadStore │ │ uiLayout   │ │ settingsStore         │  │
│  │ (aggregate)│ │ Store      │ │                       │  │
│  └────────────┘ └────────────┘ └───────────────────────┘  │
│  ┌────────────┐ ┌────────────┐ ┌───────────────────────┐  │
│  │ terminal   │ │ composer   │ │ projectStore          │  │
│  │ Store      │ │ Store      │ │                       │  │
│  └────────────┘ └────────────┘ └───────────────────────┘  │
├───────────────────────────────────────────────────────────┤
│              State Machines / Phase Enums                  │
│  ┌──────────────────┐ ┌────────────────────────────────┐  │
│  │ runLifecycle      │ │ settingsHydrationPhase         │  │
│  │ Machine (per-thd) │ │ (string enum, not FSM)         │  │
│  └──────────────────┘ └────────────────────────────────┘  │
│  ┌──────────────────┐ ┌────────────────────────────────┐  │
│  │ sidebarSync       │ │ deleteConfirmPhase             │  │
│  │ Runner            │ │ (discriminated union, not FSM) │  │
│  └──────────────────┘ └────────────────────────────────┘  │
├───────────────────────────────────────────────────────────┤
│  Pure Logic Layer（已有，保持并扩展）                        │
│  runtime-thread-surface-state.ts · dashboard-workbench-   │
│  logic.ts · model/helpers.ts · model/task-board.ts        │
├───────────────────────────────────────────────────────────┤
│  IPC Sync Layer                                           │
│  ┌─────────────────────────────────────────────────────┐  │
│  │ syncToBackend — 乐观更新 · 回滚 · 去重 · 错误标准化   │  │
│  └─────────────────────────────────────────────────────┘  │
├───────────────────────────────────────────────────────────┤
│  Orchestration Action Layer                               │
│  ┌─────────────────────────────────────────────────────┐  │
│  │ workbench-actions.ts — 跨 Store 编排函数              │  │
│  │ (submitNewThread · selectThread · removeWorkspace …) │  │
│  └─────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────┘
```

### 设计原则

1. **渐进式迁移** — 每个 Phase 可独立合并，不需要一次性完成
2. **延续现有模式** — 以 `terminalStore` 的手写 pub-sub 为原型扩展，不强制引入重量级框架
3. **显式优于隐式** — 判别联合体 + 有限状态机替代散布的 boolean 标志和字符串状态
4. **单一事实源由状态机驱动** — 线程运行状态以 `runLifecycleMachine`（每线程一个实例）为权威来源，状态机负责所有合法转换检查和非法事件忽略。状态机通过 `subscribe` 单向同步到 `threadStore.threadStatuses` 作为被动记录（供 Sidebar 等消费者读取）。`threadStore` 本身不做防乱序 reducer，避免双重管理冲突。Tauri 全局事件也经过状态机处理后再写入 Store。
5. **局部状态保留** — 只有跨组件共享、跨副作用协调或需要逃离闭包的状态进入 Store；焦点、光标、拖拽中间态、局部菜单等保留在组件或专用 hook，避免过度设计

---

## 四、改造前后对比

### DashboardWorkbench

| 指标 | 改造前 | 改造后 |
|------|--------|--------|
| 文件行数 | ~3100 | ~300 |
| `useState` | 39 | 少量局部 UI state 允许保留 |
| `useRef` | 18 | 4（DOM ref） |
| `useEffect` | 21 | 仅保留页面级副作用与专用 hook 调用 |
| 子组件最大 props 数 | 97（DashboardOverlays） | ~5 |
| 复合函数 | 7 个（共 ~700 行） | 0（全部提取到 action 模块） |

### RuntimeThreadSurface

| 指标 | 改造前 | 改造后 |
|------|--------|--------|
| props 数量 | 17 | 1（`threadId`） |
| 回调 props | 4 个 | 0（直接写 Store） |
| 内部 state | 17+（保持组件本地） | 17+（不变，本质上正确；局部实时 UI 状态不全局化） |

### 状态管理全景

| 指标 | 改造前 | 改造后 |
|------|--------|--------|
| 全局 Store | 1（`terminalStore`） | 6 个 Domain Store（含已有 `terminalStore`） |
| 显式状态机 | 0 | 1 个（`runLifecycleMachine`） |
| 显式阶段枚举/联合体 | 0 | 3 个（`settingsHydrationPhase`、`deleteConfirmPhase`、`sidebarSyncRunner`） |
| Ref-Mirror 双写 | 10+ 处 | 消除跨域 Ref-Mirror；DOM ref 与局部交互 ref 可保留 |
| IPC 同步模式 | 每处手动 try/catch | 声明式 `syncToBackend` |
| 线程状态事实源 | 3 处（不一致） | 1 处（`runLifecycleMachine` 为线程状态权威来源，通过 `subscribe` 单向同步到 `threadStore.threadStatuses` 被动记录） |

---

## 五、Store 职责矩阵

> **注意**：`threadStore` 是一个 **thread/sidebar aggregate store**，混合了线程实体/状态、Sidebar UI 分页和 Run 乐观编排三个子域。这是经过权衡的设计选择——这些子域围绕线程导航紧密协作，拆分为 3 个独立 Store 会增加跨 Store 编排复杂度，但需要通过精确 selector 避免 Sidebar 分页变化不必要地通知 Surface 订阅者。

| Store | 状态内容 | 持久化 | IPC 同步 |
|-------|---------|--------|---------|
| `threadStore`（aggregate） | 工作区列表、结构化线程状态记录、分页、pending runs | 无 | Tauri 事件驱动 |
| `uiLayoutStore` | 面板可见性、overlay、终端布局、菜单状态 | localStorage（部分） | 无 |
| `composerStore` | 新线程输入值、线程草稿、run mode、错误 | 无 | 无 |
| `settingsStore` | providers、profiles、policies、general/terminal | localStorage + SQLite | 双向 IPC |
| `projectStore` | 选中项目、最近项目、终端绑定、路径绑定 | 无 | 由编排 action 触发部分 IPC |
| `terminalStore`（已有，保持不变） | 终端 session 状态 | 无 | IPC 驱动 |

> **关于 `workbenchUiStore`**：Phase 6 中可能残留的跨组件 UI 状态（如 `runtimeContextUsage`、`topBarGitSnapshot`）优先分配到现有 Store 或通过专用 hook 本地化。仅当实际迁移后确认有多个状态确实需要独立 Store 时，再按需创建。避免预先创建"杂物箱" Store。

### Store 间依赖规则

各 Store action 允许读取的其他 Store 如下（只读，禁止写入其他 Store）：

| Store | 允许读取的 Store | 说明 |
|-------|----------------|------|
| `threadStore` | `settingsStore`（读取 provider） | 新线程创建时需要 provider 配置 |
| `projectStore` | 无 | 只维护项目/绑定本域状态；需要 `threadStore` 或 `settingsStore` 数据时由 `workbench-actions.ts` 编排 |
| `settingsStore` | 无 | 独立加载，不依赖其他 Store |
| `composerStore` | `threadStore`（读取 activeThreadId） | 输入内容绑定到当前线程 |
| `uiLayoutStore` | 无 | 纯布局状态，独立 |
| `terminalStore` | 无 | 终端 Store 不直接读项目 Store；终端与工作区绑定由 `projectStore` 和编排 action 协调 |

**禁止循环依赖**：`settingsStore` 不读取任何其他 Store；`projectStore`、`terminalStore` 等 Domain Store 不直接互读以完成业务流程。跨 Store 的读取和写入统一通过 `workbench-actions.ts` 中的编排函数完成，Store action 本身只处理本域状态。

---

## 六、状态机与阶段管理总览

| 名称 | 类型 | 状态数 | 替代的旧实现 | 自动同步目标 |
|------|------|--------|-------------|-------------|
| `runLifecycleMachine` | `createMachine` FSM | 9 态 | `runState` 字符串联合 + 4 处分散转换 | `threadStore.threadStatuses` |
| `sidebarSyncRunner` | 专用 async runner | N/A | 4 个 ref 手动 coalescing/throttling | 内部管理 |
| `deleteConfirmPhase` | 判别联合体 `useState` | 3 态 | `pendingDeleteThreadId` + `deletingThreadId` | 内部管理 |
| `settingsHydrationPhase` | 字符串枚举 + async 函数 | 6 态 | `backendHydrated` boolean + try/catch | `settingsStore.hydrationPhase` |

> **设计选择**：只有 `runLifecycleMachine` 使用 `createMachine` FSM，因为它有 9 个状态、复杂的转换图和 context 携带需求。`deleteConfirmPhase`（3 态）和 `settingsHydrationPhase`（线性加载）的复杂度不足以证明引入 FSM 的间接层。

---

## 七、实施路线图

```
Phase 0 ──→ Phase 1 ──→ Phase 2 ──→ Phase 3 ──→ Phase 4 ──→ Phase 5 ──→ Phase 6
 基础设施     核心痛点     消除drilling   显式状态机    Settings     IPC中间件     收官瘦身
 (小)        (中)        (中)          (中)        (大)         (中)        (中)
```

| Phase | 内容 | 预估规模 | 核心收益 | 详细文档 |
|-------|------|---------|---------|---------|
| **0** | `createStore` 工厂 + `createMachine` 工具 | 小 | 后续所有阶段的地基 | [Phase 0](./refact-state-arch-phase-0.md) |
| **1** | `threadStore` — 统一线程状态事实源 | 中 | **解决三源不一致**；减少 9 useState + 3 useRef | [Phase 1](./refact-state-arch-phase-1.md) |
| **2** | `uiLayoutStore` + `composerStore` | 中 | **消除 Props Drilling**；Overlays 80→30 props | [Phase 2](./refact-state-arch-phase-2.md) |
| **3** | Run 生命周期 + Sidebar 同步 + 删除确认状态机 | 中 | **消除隐式状态机**；4 ref→1 实例 | [Phase 3](./refact-state-arch-phase-3.md) |
| **4** | `settingsStore` + 加载状态机 + IPC action 拆分 | 大 | **拆解 1486 行 Hook**；Overlays 30→0 props | [Phase 4](./refact-state-arch-phase-4.md) |
| **5** | `syncToBackend` IPC 同步中间件 | 中 | **标准化前后端同步**；乐观更新+回滚+去重 | [Phase 5](./refact-state-arch-phase-5.md) |
| **6** | DashboardWorkbench 最终瘦身 | 中 | **移除跨域业务状态**；允许少量局部 UI state；props 全面精简 | [Phase 6](./refact-state-arch-phase-6.md) |

### 依赖关系

```
Phase 0 ← Phase 1 ← Phase 3
              ↑
          Phase 2
              
Phase 3 ← Phase 4 ← Phase 5 ← Phase 6 (依赖全部 0–5)
```

- **Phase 0** 是所有阶段的前提
- **Phase 1** 依赖 Phase 0
- **Phase 2** 依赖 Phase 0 和 Phase 1（需要 `threadStore` 已接管线程状态）
- **Phase 3** 依赖 Phase 0 和 Phase 1（不依赖 Phase 2，可与 Phase 2 并行或先于 Phase 2 交付）
- **Phase 1–3** 可以作为一个批次连续交付
- **Phase 4** 依赖 Phase 0–3，是最大的单阶段工作，建议独立 PR
- **Phase 5** 是跨切面改进，可在 Phase 4 之后随时进行
- **Phase 6** 是收官阶段，依赖所有前置阶段

---

## 八、技术选型与权衡

### 为什么不用 Zustand / Redux / Jotai？

手写 Store 模式（`createStore`）的 API 与 Zustand 几乎一致，但不引入依赖。如果未来规模继续增长或需要 DevTools 支持，可以一行代码迁移到 Zustand（`createStore` → `zustand/vanilla` 的 `createStore`，接口兼容）。

### 为什么不用 XState？

当前只有 Run 生命周期（9 态 + context）需要完整的 FSM。其他隐式状态机（删除确认 3 态、Settings 加载 6 态线性链）复杂度不足以使用 FSM，改用判别联合体或字符串阶段枚举即可。Sidebar 同步流控用专用 async runner 表达比 FSM 更直接。如果未来 Run 状态机需要 guards、hierarchical states 或 parallel states，再考虑引入 XState。

### 为什么 RuntimeThreadSurface 的 17+ useState 不 Store 化？

这些状态（`messages`、`tools`、`helpers`、`snapshotReady` 等）是线程实时执行的 UI 状态，生命周期与组件严格绑定（切换线程时全部重置）。Store 化会引入不必要的全局状态和清理逻辑，保持组件本地是正确的设计。

---

## 九、风险总览

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 迁移中的双写窗口 | 部分代码写 Store、部分写旧 useState | 每个 Phase 内确保所有写入点迁移完毕再合并 |
| Selector 粒度导致不必要重渲染 | 性能退化 | 高频变化的字段独立 selector；React DevTools Profiler 验证 |
| 跨 Store action 无事务性 | 中间步骤失败导致不一致 | 将 IPC 调用放在 Store 写入之前；catch 中手动回滚 |
| Tauri IPC 不支持请求取消 | 去重的 `last` 策略只能忽略返回值 | 非幂等操作用 `first` 策略 |
| 回滚 snapshot 可能过期 | 覆盖飞行期间的其他修改 | `optimistic` 只返回受影响的字段，字段级回滚 |
| Hook 提取后 deps 语义变化 | 副作用触发条件改变 | 逐个 Hook 验证 deps 完整性 |
| Store subscriber 异常导致通知链中断或白屏 | 任一 listener/selector 抛出异常影响其他订阅者 | Store emit 隔离 listener 异常，并在 DashboardWorkbench 层级包裹 Error Boundary 处理渲染错误 |

---

## 十、验证策略

每个 Phase 完成后执行：

1. **`npm run typecheck`** — 类型检查通过（大量 props 接口变更容易遗漏）
2. **`npm run test:unit`** — 新增的 Store / Machine / 中间件单元测试全部通过
3. **手动回归** — 核心流程走查（新线程→运行→完成、线程切换、删除、Settings 修改、终端操作）
4. **React DevTools Profiler** — 确认无渲染性能退化

Phase 6 完成后额外执行全量回归测试（覆盖新线程、线程切换/删除、工作区管理、终端、Git、Settings、快捷键全部场景）。

**贯穿各 Phase 的防御措施**：
- 在 `DashboardWorkbench` 层级包裹 React Error Boundary，捕获渲染阶段错误并降级展示。
- Store 的 `emit`/subscriber 通知应隔离 listener 异常（逐个 try/catch 或统一上报），因为订阅回调中的异常不一定能被 React Error Boundary 可靠捕获。
- 所有线程状态写入经过 `runLifecycleMachine` 的合法转换检查，状态机通过 `subscribe` 单向同步到 `threadStore.threadStatuses`，避免旧 run 的 `finished`、快照恢复或全局事件覆盖新 run 的 `running` 状态。
