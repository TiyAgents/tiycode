# Tiy Agent 技术架构与模块设计审查报告

## 文档信息

- 日期：2026-03-16
- 审查范围：`docs/technical-architecture-20260316.md` + `docs/module/` 全部 10 篇模块设计文档
- 对照基准：`docs/product-story-20260316.md`
- 审查目标：识别重大缺陷、缺失、不合理或过度设计问题
- 状态：已完成第三轮审查，剩余微调项见 §八

## 总体评价

整体架构设计质量较高。三层分离（React 展示层 / Rust 系统真源层 / TS Sidecar 决策层）的判断正确，权限边界严格，模块间职责划分清晰。以下逐项列出审查发现。

---

## 一、重大缺陷

### 1.1 Sidecar 单实例常驻 — 单点故障与资源隔离风险 ✅ 已处理

**所涉文档**：`agent-sidecar-design`、`technical-architecture` §5-6

**问题**

架构要求一个 sidecar 进程承载所有线程的 Agent Loop，通过 `thread_id + run_id` 多路复用。文档只描述了 crash 后 restart 的恢复路径，但没有覆盖以下关键场景：

- **内存泄漏累积**：长时间常驻的 Node.js/TS 进程，多个 session 反复创建销毁，JS 运行时的 GC 压力和闭包泄漏是现实问题。文档仅提到"explicit session cleanup + health metrics"，但没有定义健康检查协议和主动重启策略（如达到内存阈值后 graceful restart）。
- **单线程 event loop 阻塞**：如果某个 provider SDK 的 HTTP 请求 hang 住 event loop（TS 是单线程的），所有其他线程的 run 都会被阻塞。文档没有讨论请求级超时隔离或 Worker Thread 分离策略。
- **重启期间的请求丢失**：sidecar 重启过程中如果有正在进行的 tool result 返回或 agent event，Rust 侧的消息缓冲和重投递机制没有定义。

**建议**

- 补充 sidecar 健康度指标定义（RSS 内存、event loop lag、active session count）。
- 设计基于阈值的 graceful restart 策略：Rust 在 sidecar 健康度恶化时，先将活跃 run 标记为 interrupted，再重启进程。
- 为每个 provider 请求引入 AbortController + timeout 机制，防止单个 HTTP 调用阻塞全局 event loop。
- 评估是否需要在极端负载场景下支持多 sidecar 实例（可后续，但架构上应预留）。

**处理结果**：`agent-sidecar-design` 新增 Health Contract 专节，定义 4 项健康指标（`rss_bytes`、`event_loop_lag_ms`、`active_run_count`、`uptime_ms`）和 graceful restart 流程。`technical-architecture` §6.3 同步补充健康治理要求和 provider 请求超时约束。

### 1.2 线程上下文压缩 — 关键路径缺乏具体方案 ✅ 已处理

**所涉文档**：`thread-design` §Snapshot Construction、`agent-run-design` §Plan Artifact

**问题**

Thread 和 Agent Run 文档反复提到"summary compaction"、"message window cache"、"tool result digest"、"execution seed"等概念，但关键实现细节完全缺失：

- **摘要由谁生成？** 没有明确说明。如果由 sidecar 调用 LLM 生成摘要，这本身是额外的模型调用开销和延迟；如果由 Rust 做纯文本截断，则质量无法保证。
- **compaction 触发时机**未定义：是每次 run 结束后？达到消息数量阈值后？后台定时任务？
- **`CleanContextFromPlan` 的 `execution_seed` 生成**依赖摘要能力，但没有定义谁负责结构化提取以及降级策略。
- **摘要失败的后果**未考虑：如果 LLM 调用失败，是降级到全文还是阻塞后续操作？

**建议**

- 在 Thread 设计文档中补充 compaction pipeline 专节，明确执行者（推荐：sidecar 使用 lightweight 模型生成结构化摘要，Rust 负责触发和持久化）。
- 定义触发策略：run 结束后检查消息总量，超过阈值时由 Rust 发起 compaction 请求到 sidecar。
- 定义降级方案：LLM 不可用时，使用简单的消息截断（保留最近 N 条 + 首条用户消息 + 所有未完成 approval）。
- 对 `execution_seed` 明确其结构化格式和生成责任方。

**处理结果**：`thread-design` 新增 Compaction Pipeline 专节（触发时机、执行者、降级方案）。`agent-run-design` 新增 `ExecutionSeed` 结构体和降级规则。`technical-architecture` §7.3 同步补充 compaction 策略和降级路径。

**遗留 R1（已解决）**：第三轮补充了 v1 建议初始阈值（消息数 > 50 / token 估计 > 32k / 单次 tool 结果超预算），标注为建议初始值。`thread-design` 和 `technical-architecture` §7.3 均已同步。

### 1.3 前端与 Rust 之间缺少类型契约层设计 ✅ 已处理

**所涉文档**：`technical-architecture` §8.1、各模块的 Recommended Types

**问题**

所有模块文档定义了 Rust 侧类型（`ThreadSnapshot`、`GitSnapshot`、`ToolExecutionRequest` 等），但前端消费这些数据的 DTO/接口契约完全没有定义。架构有两层 IPC 边界：

- Rust ↔ Sidecar：有 JSON-RPC schema 规范和协议版本。
- Rust ↔ Frontend：只说"invoke + channels"，没有类型一致性保证。

具体缺失：

- 没有统一的 DTO 定义规范或 codegen 方案。
- 没有序列化策略说明（Rust struct 如何映射到 TS type，snake_case vs camelCase 等）。
- 没有版本化机制（Rust 侧 struct 变更后前端如何感知不兼容变更）。

**建议**

- 补充 IPC 契约层专题设计。
- 引入 codegen 方案（如 `specta` 或 `ts-rs`），从 Rust 类型自动生成 TS 类型定义，保证类型一致性。
- 定义序列化约定：建议 Rust 侧 serde 配置 `#[serde(rename_all = "camelCase")]` 以匹配前端习惯。
- 对 channel 事件载荷使用 tagged union（TypeScript discriminated union），确保前端可以安全 switch。

**处理结果**：`technical-architecture` §8.1 新增 Frontend/Rust 类型契约小节，定义了 `specta` codegen 方向、`camelCase` 序列化、discriminated union 事件格式和 `schema_version` 要求。

**遗留 R2（已解决）**：第三轮将 `specta` 明确为 `tauri-specta`，补充了 `schema_version` 不兼容时"阻断使用并提示升级应用"的策略，以及所有 invoke/channel 失败复用 `AppError` 契约。

---

## 二、重要缺失

### 2.1 缺少统一的错误传播与用户反馈模型 ✅ 已处理

**所涉文档**：各模块的 Failure Modes 表

**问题**

每个模块定义了各自的 Failure Modes，但没有跨模块的统一错误模型：

- Git 操作失败的结构化错误如何传递到 ThreadStreamEvent？
- Terminal 的 PTY spawn 失败如何在 UI 上表达？
- Tool 执行失败和 policy deny 的错误类型是否统一？
- 前端如何区分"可重试错误"和"终止性错误"？
- Sidecar 的 provider 错误（rate limit、auth failure）如何映射到用户可理解的反馈？

**建议**

- 在架构层面补充统一的 `AppError` 类型体系设计。
- 定义错误分级：`Fatal`（需要用户干预或重启）、`Recoverable`（可重试或降级）、`Informational`（仅通知）。
- 定义跨模块错误传播规范：所有子系统错误需要包含 `error_code`、`category`、`user_message`、`detail`、`retryable` 等标准字段。
- 定义前端错误展示策略：线程内错误走 ThreadStreamEvent，面板级错误走 toast 或 inline status。

**处理结果**：`technical-architecture` §7.4 新增统一错误模型，定义了 `AppError` 的 7 个标准字段（`errorCode`、`category`、`source`、`userMessage`、`detail`、`retryable`、`correlationId`）和展示原则。

**遗留 R3（已解决）**：第三轮将 §7.4 标题改为"统一错误模型（跨层契约）"以明确定位，补充了 `errorCode` 编码格式 `<source>.<kind>.<detail>` 和 4 个示例（`tool.policy.denied`、`git.remote.auth_failed` 等）。

### 2.2 缺少前端状态管理架构设计 ✅ 已处理

**所涉文档**：`technical-architecture` §6.1

**问题**

架构列出了 5 个 store（workbench / thread-view / settings / marketplace / terminal-view），但缺少：

- **Store 间联动关系**：切换 workspace 后，thread-view-store、terminal-view-store 如何联动清理？线程删除后终端 store 如何响应？
- **Channel 订阅生命周期**：ThreadStreamEvent、TerminalStreamEvent、GitStreamEvent 的订阅何时建立、何时销毁、断线重连策略是什么？
- **App 启动状态恢复流程**：前端如何从 Rust 拉取完整初始状态？是一次性 batch load 还是按需 lazy load？加载顺序是什么？
- **乐观更新策略**：用户操作（如 stage file）是先更新 UI 再等 Rust 确认，还是等 Rust 返回后再更新？

**建议**

补充前端状态管理专题设计文档，至少覆盖：

- store 依赖关系图和联动清理规则
- channel 订阅生命周期管理（推荐与 React 组件生命周期绑定 + 全局 ThreadStream 保持长连接）
- app 启动阶段的状态初始化序列
- 乐观更新的适用范围与回滚机制

**处理结果**：`technical-architecture` §6.1 新增 store 联动约束、启动加载顺序（settings/workspaces bootstrap → workbench 骨架 → 线程按需加载 → 面板懒加载）、流式订阅生命周期规则和断线重连策略。

### 2.3 `shell` 工具缺少安全边界定义 ✅ 已处理

**所涉文档**：`agent-tools-design`、`tool-gateway-policy-design`

**问题**

Agent Tools 文档列出 `shell` 为系统工具，但所有模块文档都没有对其安全策略做专项定义。`shell` 是风险最高的工具，允许任意 shell 命令执行：

- 没有定义命令白名单/黑名单策略。
- 没有定义命令解析和危险命令检测（如 `rm -rf /`、`curl | sh`）。
- 没有定义执行沙箱隔离方式（是否在受限 shell 中运行？是否限制环境变量继承？）。
- 没有定义超时和输出截断策略。
- 没有说明与 Terminal 工具族的关系和区分（`shell` 是一次性执行并返回结果，还是等价于 `term_write`？）。

**建议**

- 在 Tool Gateway + Policy 文档中增加 `shell` 安全专节。
- 定义 `shell` 为"无交互式一次性命令执行"，区别于 Terminal 的交互式 PTY 会话。
- 在 PolicyEngine 中为 `shell` 添加专门的命令解析和匹配逻辑（不能仅依赖通用 allow/deny 规则）。
- 强制超时（建议默认 60s，可配置）和输出大小限制。
- 默认策略建议为 `require-approval`，除非命令匹配用户显式允许的模式。

**处理结果**：`tool-gateway-policy-design` 新增 `shell Is a Special System Tool` 和 `shell Execution Contract` 两个专节，定义了非交互式语义、默认 require-approval、命令 deny 前置检查、超时与输出截断、结构化失败类型。`technical-architecture` §9.2 同步补充。

**遗留 R4（已解决）**：第三轮在 `tool-gateway-policy-design` 补充了 Dangerous-pattern handling 专节，定义了内置硬拒绝列表（`rm -rf /`、`sudo`、`mkfs`、`curl | sh` 等）、argv 归一化优先于原始 shell 文本、用户 `denyList` 可扩展但不可削弱内置拒绝、内置拒绝使用显式归一化谓词而非脆弱的子串匹配。

### 2.4 缺少 AI Elements 集成技术方案 ✅ 已处理

**所涉文档**：`technical-architecture` §6.1

**问题**

AI Elements 作为线程 UI 的核心组件层被反复提及，但没有任何文档说明：

- AI Elements 的数据接入方式：是直接消费 `ThreadStreamEvent`，还是需要中间适配层？
- `ThreadStreamEvent` 的事件类型与 AI Elements 组件之间的映射关系（如 `plan_updated` → `<Plan>`、`reasoning_updated` → `<Reasoning>`）。
- AI Elements 组件的状态管理：组件内部是否自管理状态，还是完全受控于外部 store？
- 自定义扩展 AI Elements 的边界：是否允许 Marketplace 扩展注入自定义渲染组件？

**建议**

补充 AI Elements 集成专题，定义 `ThreadStreamEvent → AI Elements component` 的映射规范、数据流向和状态管理策略。

**处理结果**：`technical-architecture` §8.2 新增 adapter 映射表（`plan_updated → Plan`、`tool_* → Tool`、`approval_required → Confirmation` 等），明确前端通过 adapter 层映射而非组件直接消费协议事件。同时新增了 `approval_required`、`approval_resolved`、`run_interrupted` 三个事件类型。

**遗留 R5（已解决）**：第三轮补充了错误态映射：`tool_failed → Tool 错误态 + Conversation 系统消息`、`run_failed → Conversation 系统消息配合 Failed 状态`、`run_interrupted → Conversation 系统消息明确为外部中断`。

---

## 三、不合理之处

### 3.1 一线程一终端的强制绑定过于刚性 ✅ 已处理

**所涉文档**：`terminal-design` §Decision

**问题**

Terminal 文档强制 `1 thread = 1 terminal session`。在实际开发场景中：

- 一个任务可能需要同时观察前端 dev server 和后端日志（需要多终端）。
- 许多线程根本不需要终端（纯问答型对话、代码 review 等）。
- 如果用户在线程终端里启动了 dev server，线程被归档后终端会被销毁，导致进程中断。

文档将 multi-tab terminals 放入 Non-Goals 可以理解，但强制一对一绑定而非"按需创建、可选关联"不合理。

**建议**

- 修改为"一个线程可以按需创建零或一个终端（v1 限制为最多一个），终端不随线程创建而自动创建"。
- 终端创建应为显式用户动作或 Agent 工具调用触发。
- 为后续多终端扩展预留 `session_id` 的独立性（不以 `thread_id` 作为 session 唯一键）。

**处理结果**：`terminal-design` 修改为 `0..1` 按需创建模型，三处关键位置（Design Decisions、Requirements、Chosen Runtime Model）均已更新。

### 3.2 Git Snapshot 增量刷新模型过于理想化 ✅ 已处理

**所涉文档**：`git-design` §Refresh Model

**问题**

Git 文档定义了 "snapshot + delta" 刷新模式，但存在以下问题：

- Git 状态变化来源复杂（用户在外部编辑器操作、在终端 `git add`、Agent 工具调用、其他进程），增量 delta 检测的触发源没有定义。
- 没有 filesystem watcher 设计，也没有定义 polling 策略。
- `git status` 本身是全量计算操作。所谓"增量"实际上只能是 UI 层面的差异比较（新旧 snapshot diff），底层仍然是每次全量获取。文档暗示底层可以做增量，但这不符合 Git 的工作机制。

**建议**

- 明确说明 v1 的刷新策略是"事件触发的全量快照 + UI 层 diff 渲染优化"，触发源包括：用户打开 Git 面板、用户执行 Git 操作后、Agent 工具执行 Git 操作后、用户手动刷新。
- `file_delta` 和 `history_delta` 事件应明确为"Rust 对比新旧 snapshot 后生成的差异事件"，而非底层 Git 级别的增量。
- 移除对 filesystem watcher 的隐含依赖，或明确标注为 v2 增强项。

**处理结果**：`git-design` 全面修正为"事件触发的全量 snapshot 重算 + 可选派生 UI diff"。`GitStreamEvent` 从 `file_delta / history_delta` 改为 `snapshot_updated`，并明确 UI diff 是 Rust 对比前后 snapshot 的派生事件。`technical-architecture` 同步更新。

### 3.3 Settings 变更的传播链路不完整 ✅ 已处理

**所涉文档**：`agent-sidecar-design` §Protocol、`tool-gateway-policy-design`

**问题**

多个模块依赖 Settings（Provider 配置、Policy 规则、Agent Profile 等），但 Settings 变更后的传播行为没有统一定义：

- Sidecar 协议有 `agent.settings.changed`，但没有定义哪些 settings key 的变更需要实时推送、推送的载荷格式、sidecar 收到后的处理策略。
- Profile 变更对进行中 run 的影响已说明（frozen plan），但 Policy 变更未说明：用户在 run 过程中修改了 allow/deny 规则，是否对当前 run 已持有的 pending approval 生效？
- Provider 配置变更（如更换 API Key、切换 Base URL）后，sidecar 内的 provider client 如何热更新？是销毁重建还是延迟到下一次 run？
- 主题和语言等前端偏好的变更与系统配置的变更是否走同一条路径？

**建议**

补充 Settings 变更传播模型：

| 变更类别 | 生效范围 | 传播方式 |
|---|---|---|
| Theme / Language | 仅前端 | 前端 store 直接响应 |
| Agent Profile | 仅对新 run 生效 | 不需要通知 sidecar |
| Provider 配置 | 对新 run 生效 | 通知 sidecar 刷新 provider registry |
| Policy 规则 | 立即对所有新 tool 请求生效 | 不需要通知 sidecar，Rust PolicyEngine 直接读取最新配置 |
| Workspace 配置 | 立即生效 | 通知相关 manager 重新验证 |

**处理结果**：`technical-architecture` §8.4 新增完整传播矩阵，5 类配置的生效范围和传播方式均已明确定义，包括 pending approval 在执行前的重新评估机制。

---

## 四、过度设计

### 4.1 Index 子系统 v1 范围偏大 ✅ 已处理

**所涉文档**：`index-design`

**问题**

Index 文档定义了三个 layer（File Tree Cache + Content Inverted Index + Activity Signals），对于 v1 来说：

- **在 Rust 中从零构建倒排索引** 的实现成本不低，且 `ripgrep` 本身已经足够快，直接封装为搜索后端可以覆盖绝大多数 `grep` 场景。
- **Activity Signals** 在 v1 没有真实上游数据源："thread-referenced files"、"recently opened files" 等信号在产品第一阶段都不存在成熟的采集链路。

**建议**

v1 范围裁剪为：

- Layer 1：File Tree Cache — 保留，作为 Project Drawer 的数据源。
- Layer 2：Content Search — 改为封装 `ripgrep` 子进程调用，而非自建倒排索引。如果后续证明 ripgrep 启动开销过高或需要更复杂的排序策略，再考虑索引化。
- Layer 3：Activity Signals — 推迟到 v2，待线程和工具执行链路稳定后再接入。

**处理结果**：`index-design` 全面裁剪为 File Tree Cache + ripgrep 封装。Activity Signals 明确标注 deferred to v2，倒排索引移入后续扩展。`technical-architecture` Phase 2 和 Phase 3 描述同步更新。

### 4.2 Automation Scheduler 在 v1 无实际场景支撑 ✅ 已处理

**所涉文档**：`marketplace-automation-design` §Scheduler Model

**问题**

Marketplace 文档为 Automation 定义了完整的调度模型（next run time 计算、并发锁、run history 持久化）。但 PRD 明确说明当前 Marketplace 处于"本地持久化管理"状态，"真实远端目录、安装与运行机制"均属原型态能力。在没有真实 automation 定义来源和执行目标的情况下：

- Scheduler 的 cron 计算、并发控制、run history 都是无法验证的空转逻辑。
- 将 automation 与 thread/tool 系统打通（"automation may open or create a thread"）引入的集成复杂度与当前产品阶段不匹配。

**建议**

v1 仅保留：

- Automation 的目录展示和 install/enable 状态管理（作为 Marketplace 的一个 category）。
- `automation_runs` 表结构预留。
- Scheduler 实现和 thread 集成推迟到 Phase 3。

**处理结果**：`marketplace-automation-design` 将 scheduler 全面推迟到 Phase 3。Scheduler Model 改为 Phase 3 Direction，`automation_runs` 表标注为 Phase 3 预留，`run_automation` 流程标注为 deferred。架构图移除了 Scheduler → ToolGateway 连线。

---

## 五、模块间一致性问题

### 5.1 用户发起的操作与 Agent 发起的操作走不同执行路径 ✅ 已处理

**所涉文档**：`tool-gateway-policy-design`、`git-design`、`terminal-design`

**问题**

Tool Gateway 文档强调"所有 privileged tool requests 必须经过 ToolGateway"，但 Git 和 Terminal 文档中分别提到：

- Git："用户从 Git drawer 调用 Rust commands 直接执行"
- Terminal："用户直接在终端输入不需要额外审批"

这意味着用户发起的操作走 Tauri invoke 直达对应 Manager，Agent 发起的同类操作走 ToolGateway → PolicyEngine → Manager。同一个操作（如 `git push`）有两条执行路径。

**风险**

- 审计不一致：用户操作的 `git push` 可能不留审计记录，Agent 操作的留了。
- 策略判断分裂：如果 policy deny 了 `git push`，用户仍然可以从 drawer 直接推送，策略形同虚设。
- 维护成本：每个子系统需要同时支持两套调用入口，参数校验和错误处理可能分化。

**建议**

推荐两种方案之一：

**方案 A（推荐）：统一走 ToolGateway，但区分调用方身份**

- 所有写操作（Git mutation、terminal write 等）统一经过 ToolGateway。
- ToolGateway 接受 `caller: User | Agent` 标识。
- 当 `caller = User` 时，PolicyEngine 可以应用宽松策略（如自动放行），但审计记录仍统一生成。

**方案 B：Manager 直接执行 + 独立审计**

- 用户操作直接调用 Manager，不经过 ToolGateway。
- 但 Manager 自身需要内置审计能力，确保所有变更操作都有记录。
- Policy 规则只约束 Agent 操作。

无论选哪种，需要在架构层面做出明确决定并统一文档口径。

**处理结果**：`tool-gateway-policy-design` 新增 Clarification，明确 sidecar 必须走 ToolGateway，用户操作可直接走 Rust commands 但"应复用相同 policy primitives、normalized request model 和 audit schema"。`git-design` 同步修改为"mutating actions should reuse the same policy primitives and audit schema"。

**遗留 R6（已解决）**：第三轮采用独立 `audit_events` 表方案（`run_id` nullable，`actor_type` 区分 user/agent/system），`tool-gateway-policy-design` 新增 Shared Policy and Audit Primitives 专节定义了 `PolicyCheck` + `AuditRecord` 共享原语，`git-design` 同步补充了 `AuditRecord` 写入规则，`technical-architecture` §7.2 和 §13 同步更新。

### 5.2 Thread 状态与 Run 状态的推导关系不明确 ✅ 已处理

**所涉文档**：`thread-design` §Thread Lifecycle、`agent-run-design` §State Machine

**问题**

Thread 文档说 `ThreadStatus` 是"derived from the latest run and pending approvals"，但 Thread 和 Run 的状态机分别定义，没有给出明确的推导规则。例如：

- Run 处于 `WaitingToolResult` 时，Thread 状态是 `Running` 还是 `WaitingApproval`？
- Run 处于 `Cancelling` 时，Thread 是什么状态？
- 最后一个 run 为 `Failed`，Thread 状态是 `Failed` 还是 `Idle`？
- 最后一个 run 为 `Completed`，Thread 状态显然是 `Idle`。但如果紧接着用户正在输入下一条消息呢？

**建议**

在 Thread 设计文档中补充明确的推导规则：

```
ThreadStatus = f(latest_run)

| latest_run.status          | ThreadStatus     |
|----------------------------|------------------|
| null (no run)              | Idle             |
| Created / Dispatching      | Running          |
| Running / WaitingToolResult| Running          |
| WaitingApproval            | WaitingApproval  |
| Cancelling                 | Running          |
| Completed / Cancelled      | Idle             |
| Failed                     | Failed           |
| Denied                     | Idle             |
| Interrupted                | Interrupted      |
```

同时明确：Thread 的 `Failed` 和 `Interrupted` 状态在用户发起新 run 后自动回到 `Running`，不需要手动重置。

**处理结果**：`thread-design` 新增 ThreadStatus Derivation 专节，包含完整推导表和补充规则（pending approval 优先级、Failed/Interrupted 自动回退、前端输入态不影响 ThreadStatus）。

---

## 六、第二轮新发现

### N1. Sidecar 协议缺少 summary 和 seed 生成请求方法 ✅ 已处理

**所涉文档**：`agent-sidecar-design` §Protocol、`agent-run-design` §ExecutionSeed、`thread-design` §Compaction Pipeline

**问题**

`agent-run-design` 定义了 `ExecutionSeed` 并说明"sidecar may help synthesize the seed"。`thread-design` 定义了 Compaction Pipeline 并说明"Rust asks sidecar for a structured summary candidate"。但 `agent-sidecar-design` 的 Rust → Sidecar 请求列表中没有新增对应方法。

**建议**

在 sidecar 协议中补充摘要生成和 seed 派生的请求方法，或统一为一个通用辅助任务请求 `agent.auxiliary.task`。

**处理结果**：采用通用辅助任务方案。`agent-sidecar-design` 和 `technical-architecture` §8.3 均新增 `agent.auxiliary.task` 方法，覆盖 `summarize_thread_window` 和 `derive_execution_seed` 两种 v1 任务类型，并定义了 payload shape（`auxiliary_task_id`、`task_type`、`workspace_id`、task-specific payload）。

### N2. AI Elements 映射表缺少错误态组件映射 ✅ 已处理

**所涉文档**：`technical-architecture` §8.2

**问题**

新增的 ThreadStreamEvent → AI Elements 映射表覆盖了正常流事件，但没有定义错误态的映射。

**处理结果**：`technical-architecture` §8.2 新增"错误态与边界态"映射：`tool_failed → Tool 错误态 + Conversation 系统消息`、`run_failed → Conversation 系统消息块`、`run_interrupted → Conversation 系统消息块（明确为外部中断而非模型失败）`。

### N3. 用户操作审计的表结构适配 ✅ 已处理

**所涉文档**：`technical-architecture` §7.2、`tool-gateway-policy-design`

**问题**

用户从 Git drawer 发起的 commit/push 没有 `run_id`，直接写 `tool_calls` 表的外键约束不满足。

**处理结果**：采用独立 `audit_events` 表方案。`technical-architecture` §7.2 新增 `audit_events` 表（`actor_type`、nullable `run_id`、`policy_check_json`、`result_json` 等），`tool_calls` 保持 run-bound 语义不变，两表通过关联键引用。`tool-gateway-policy-design` 新增 Shared Policy and Audit Primitives 专节。`git-design` 补充了用户操作写入 `AuditRecord` 的规则。`technical-architecture` §13 审计落点同步更新为统一写入 `audit_events`。

---

## 七、第二轮遗留项处理汇总

以下为第二轮审查遗留的 8 项（合并后 6 个独立工作项），已在第三轮优化中全部解决。

| 编号 | 内容 | 状态 | 处理方式 |
|---|---|---|---|
| R1 | Compaction 阈值缺少参考值 | ✅ | `thread-design` 和 `technical-architecture` §7.3 补充 v1 初始值（消息 >50 / token >32k / 单次 tool 超预算） |
| R2 | `specta` 应明确为 `tauri-specta` + `schema_version` 策略 | ✅ | `technical-architecture` §8.1 更新为 `tauri-specta`，补充阻断升级策略和 `AppError` 复用 |
| R3 | 错误模型位置 + `errorCode` 编码规范 | ✅ | §7.4 标题标注"跨层契约"，补充 `<source>.<kind>.<detail>` 格式和示例 |
| R4 | `shell` 危险模式匹配规范 | ✅ | `tool-gateway-policy-design` 新增 Dangerous-pattern handling，内置硬拒绝 + argv 归一化 + 用户 denyList 可扩展不可削弱 |
| R5/N2 | 错误态 AI Elements 映射 | ✅ | `technical-architecture` §8.2 补充 `tool_failed`、`run_failed`、`run_interrupted` 映射 |
| R6/N3 | Policy+Audit 原语 + 审计表结构 | ✅ | 新增 `audit_events` 表，`tool-gateway-policy-design` 新增 Shared Primitives 专节，`git-design` 同步 |
| N1 | Sidecar 协议缺辅助任务方法 | ✅ | `agent-sidecar-design` 和总架构新增 `agent.auxiliary.task`，覆盖 summary 和 seed 生成 |

---

## 八、第三轮新发现（微调项，已处理）

第三轮审查发现以下两个实现细节级别的小问题，现已同步回填到设计文档。

### M1. `agent.auxiliary.task` 的响应语义应显式说明 ✅ 已处理

**所涉文档**：`agent-sidecar-design` §Protocol

**问题**

`agent.auxiliary.task` 已添加到 Rust → Sidecar 请求列表，但 Sidecar → Rust 的事件列表中没有对应的完成/失败事件。当前协议是 JSON-RPC style（request 有 `id`，response 回同一个 `id`），所以辅助任务结果可以通过 request-response 对返回而非独立事件。但这一语义选择没有显式说明，实现者可能困惑于该用 event 还是 response。

**建议**

在 `agent-sidecar-design` 的 Protocol Rules 中补充一条：`agent.auxiliary.task` 使用同步 request-response 语义（JSON-RPC response on the same `id`），结果不通过 event stream 返回。

**处理结果**：`agent-sidecar-design` 新增 Response semantics 段落，并在 Protocol Rules 中明确：`agent.auxiliary.task` 使用同一 `id` 的 JSON-RPC request-response 语义，成功与失败结果都不通过独立 event stream 返回。`technical-architecture` §8.3 同步补充了相同约束。

### M2. `audit_events` 与 `tool_calls` 的关联字段未定义 ✅ 已处理

**所涉文档**：`technical-architecture` §7.2

**问题**

文档说明"tool_calls 与 audit_events 通过关联键建立引用"，但两张表都没有对方的引用字段。`audit_events` 没有 `tool_call_id`，`tool_calls` 也没有 `audit_event_id`。

**建议**

在 `audit_events` 表中增加 nullable `tool_call_id` 字段，表示"这条审计记录关联的 tool call（如有）"。这是最自然的关联方向：一条 audit event 可能对应一个 tool call，也可能是纯用户操作不关联任何 tool call。

**处理结果**：`technical-architecture` §7.2 的 `audit_events` 表新增 nullable `tool_call_id` 字段，并明确其含义为“当审计事件对应某个 Agent tool call 时写入该字段”。同时将“`tool_calls` 与 `audit_events` 通过关联键建立引用”的描述收敛为“通过 `tool_call_id` 建立引用”，消除了实现歧义。

---

## 九、最终结论

经过三轮审查与优化迭代，架构文档体系已完整覆盖所有审查项：

- 第一轮发现 14 项问题（3 重大 + 4 重要缺失 + 3 不合理 + 2 过度设计 + 2 一致性问题）
- 第二轮确认 7 项完全解决、5 项大部分解决留有 8 个遗留点、新发现 3 项
- 第三轮确认全部遗留项、新发现项以及两项微调项（M1、M2）均已解决

**架构方案可以正式进入实现规划阶段。** 当前审查报告中的所有条目均已完成闭环，后续可转入实现拆解与 Phase 1 编码准备。
