# Prompt 注入逻辑重构方案

> 目标：在保留现有功能的前提下，重构 Prompt 注入链路，使其**更稳健（可降级、可观测、可测试）**、**更可扩展（新增章节/子代理/Surface 不需要改装配器）**、**更易维护（静态文案外置、配置即数据、单一职责）**。
>
> 范围：`src-tauri/src/core/prompt/**`、`agent_session.rs::build_system_prompt + inject_goal_context`、`subagent/orchestrator.rs::build_helper_system_prompt`、`agent_run_summary.rs` / `agent_run_title.rs` 内的 system prompt 构造。

---

## 一、现状分析

### 1.1 主链路（主代理 system prompt）

入口位于 `src-tauri/src/core/agent_session.rs:569`：

```rust
let system_prompt = build_system_prompt(pool, &raw_plan, workspace_path, run_mode).await?;
let system_prompt = inject_goal_context(pool, thread_id, system_prompt).await?;
```

实际由 `src-tauri/src/core/prompt/` 目录下四个文件协作完成：

| 文件 | 职责 | 关键产物 |
|---|---|---|
| `mod.rs` | 模块导出 | `build_system_prompt`、`PromptBuildContext`、`PromptPhase`、`PromptSection`、`PromptSectionProvider` |
| `context.rs` | 构建上下文 | `PromptBuildContext { pool, raw_plan, workspace_path, run_mode }`，全字段 `&'a` 引用 |
| `section.rs` | 数据模型 | `PromptSection { key, title, body, phase, order_in_phase }` + `PromptSectionProvider` trait |
| `assembler.rs` | 装配器 | 顺序调用 5 个 Provider → 过滤 empty → 按 `(phase, order_in_phase)` 排序 → `format!("## {title}\n{body}")` → `"\n\n"` 拼接 |
| `providers.rs` | 5 个内置 Provider | `BaseProvider` / `WorkspaceProvider` / `EnvironmentProvider` / `SkillsProvider` / `ProfileProvider` |

`PromptPhase` 枚举：`Core` / `Capability` / `WorkspacePreference` / `RuntimeContext`。

### 1.2 现有 Section 清单

| key | title | phase | order | 来源 Provider | 静/动 |
|---|---|---|---|---|---|
| `role` | Role | Core | 10 | Base | 静 |
| `behavioral_guidelines` | Behavioral Guidelines | Core | 20 | Base | 静（巨型字面量） |
| `final_response_structure` | Final Response Structure | Core | 30 | Base | 静 |
| `project_context` | Project Context (workspace instructions) | WorkspacePreference | 10 | Workspace | 动（读 `AGENTS.md` 等） |
| `system_environment` | System Environment | RuntimeContext | 10 | Environment | 动（OS / shell / **当前日期**） |
| `sandbox_permissions` | Sandbox & Permissions | RuntimeContext | 20 | Environment | 动（DB 查 policy） |
| `shell_tooling_guide` | Shell Tooling Guide | Capability | 10 | Environment | 静 |
| `skills` | Skills | Capability | 20 | Skills | 动（DB / 工作区配置） |
| `profile_instructions` | Profile Instructions | WorkspacePreference | 20 | Profile | 动（profile_repo） |
| `run_mode` | Run Mode | RuntimeContext | 30 | Profile | 半静（按 `run_mode` 选分支） |
| `runtime_context` | Runtime Context | RuntimeContext | 40 | Profile | 动（`Workspace path: {…}`） |

`providers.rs:257` 的注释明确说明：

> *Dynamic values like the current date are intentionally excluded from the system prompt to keep it stable for LLM prompt prefix caching.*

——但实际上 `system_environment` 仍然把 `current_date` 写入了 system prompt（`providers.rs:402`），与注释意图相悖。

### 1.3 后处理：Goal 注入

`agent_session.rs:1420 inject_goal_context` 在 `build_system_prompt` 之外**追加字符串**：

```rust
system_prompt.push_str("\n\n");
system_prompt.push_str(&goal_block);
```

这是一条独立的"事后注入"路径，绕过了 `PromptSection` 数据模型。

### 1.4 子代理 system prompt（关键反模式）

`src-tauri/src/core/subagent/orchestrator.rs:850 build_helper_system_prompt`：

1. 取父 system prompt 字符串
2. **按 `## ` 行解析回 `(title, body)` 列表**（`collect_prompt_sections`）
3. 用白名单 `HELPER_INHERITED_SECTION_TITLES`（`Profile Instructions`、`Project Context (workspace instructions)`、`System Environment`、`Runtime Context`）过滤
4. 拼接 `inherited + helper_shell_tooling_guide + profile.system_prompt() + output_tail`

这是**典型的"序列化 → 字符串 → 反序列化 → 再序列化"循环**：父端已经持有结构化的 `PromptSection`，渲染为字符串后，子代理又用字符串解析重新过滤——一旦渲染格式微调（如把 `## ` 改成 `### `，或加上版本号），子代理继承立刻失效，**且没有任何编译期检查**。

### 1.5 其他 prompt 入口（散落）

- `agent_run_summary.rs:105 build_compact_summary_system_prompt` —— 上下文压缩
- `agent_run_summary.rs:333 build_merge_summary_system_prompt` —— summary-of-summary 合并
- `agent_run_summary.rs:63 build_implementation_handoff_prompt` —— Plan 审批后切到 Implementation 模式的接力 prompt（用户消息体）
- `agent_run_title.rs:213 build_title_prompt_from_messages` —— 会话标题生成
- `subagent/runtime_orchestration.rs:306 SubagentProfile::system_prompt` —— 三类 helper 的硬编码 prompt

这些路径共享的概念（响应语言、响应风格、工作区路径、当前日期、Run Mode）各自重复实现，没有共享原语。

### 1.6 痛点小结

| 痛点 | 体现 | 影响 |
|---|---|---|
| **Provider 顺序硬编码** | `assembler.rs:18-22` 把 5 个 Provider 写死 | 新增 Provider 必须改装配器 |
| **`order_in_phase` 跨 Provider 冲突** | `Profile.run_mode = 30`、`Environment.sandbox_permissions = 20`，没有命名空间 | 多 Provider 协作排序困难 |
| **Section 数据流双向损失** | 子代理通过字符串解析回来过滤 | 渲染格式改动会破坏继承；难做 i18n、版本灰度 |
| **巨型字面量内嵌代码** | `Behavioral Guidelines` 单条 body > 6KB，单行 | 改一个 bullet 就动一个 .rs 文件；diff 噪音大；无法直接给运营/PM 编辑 |
| **静态/动态混杂** | `current_date` 被写入 system prompt，破坏 prompt-prefix cache 的稳定性 | LLM 端缓存命中率受影响 |
| **事后注入是特殊路径** | `inject_goal_context` 字符串拼接 | 后续 Active Plan、Active Task Board 都会重复这种反模式 |
| **失败硬阻塞** | 任意 Provider 返回 `Err` 都会让整个 system prompt 构建失败 | 例如 Skills 列表读取失败时不应阻塞主代理启动 |
| **缺乏可观测性** | 没有 token / 长度 / Section 命中率指标 | 难调优、难灰度、难定位"为什么这次 prompt 长了 30%" |
| **缺乏长度预算** | 任意 Provider 可输出无限文本 | 极端工作区下系统 prompt 膨胀，吃光 user message 上下文窗口 |
| **测试薄弱** | `providers.rs` 仅 2 个单测 | 重构、灰度都缺安全网 |
| **多 Surface 重复实现** | summary / title / subagent 各自手写共享原语（响应语言、风格） | 一处改风格规则需要扫多处 |

---

## 二、设计目标与原则

| 维度 | 目标 | 设计原则 |
|---|---|---|
| **稳健性** | Provider 失败不阻塞整体；可观测；可回放 | 软失败（`SectionOutcome`）+ 结构化日志 + 构建审计快照 + 版本号 |
| **可扩展性** | 新增 Section / 新 Surface / 新策略不改装配器 | 注册表（`Composer::register`）+ Surface 拣选谓词 + 依赖声明 |
| **易维护性** | 静态文案与代码解耦；单一职责；可独立测试 | 模板外置（`templates/*.md`）+ "一个 Section 一个 Source" + 数据驱动配置 |
| **缓存友好** | 显式区分稳定 prefix / 动态 overlay / ephemeral suffix；与 LLM provider cache marker 对齐 | `PromptLayer` 显式分层 + `PromptBlock + CacheMarker` 输出契约 |
| **长度可控** | system prompt 在极端工作区下不会无限膨胀 | 全局 + per-section 预算 + 按 Layer 优先级驱逐 |
| **多 Surface 复用** | 主代理、Helper、压缩、标题共享一套 Section 仓库 | `PromptSurface` 维度选择 + 共享 Section 库 |

---

## 三、目标架构

### 3.1 整体分层

```
┌─────────────────────────────────────────────────────────────┐
│ 调用方 (agent_session / subagent / compaction / title)       │
└───────────────────────┬─────────────────────────────────────┘
                        │ build(surface, BuildCx)
                        ▼
┌─────────────────────────────────────────────────────────────┐
│              PromptComposer (装配引擎)                       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │ Surface 适配 │→ │ 依赖解析+排序 │→ │ Layer 分桶渲染│       │
│  └──────────────┘  └──────────────┘  └──────┬───────┘       │
│                                              ▼              │
│                                   预算检查 / 驱逐 / 截断     │
│                                              ▼              │
│                                ComposedPrompt {              │
│                                  text,                       │
│                                  blocks: [PromptBlock],      │
│                                  schema_version,             │
│                                  audit: SectionAudit[],      │
│                                  warnings,                   │
│                                }                             │
└───────────────────────┬─────────────────────────────────────┘
                        │ 注册查询
                        ▼
┌─────────────────────────────────────────────────────────────┐
│  SectionRegistry (静态 + 动态 Source 注册表)                  │
│   Role | BehavioralGuidelines | FinalResponseStructure       │
│   ProjectContext | Skills | ProfileInstructions              │
│   SystemEnvironmentStatic | SandboxPermissions | RunMode     │
│   ShellToolingGuide | RuntimeContext | ActiveGoal            │
│   ActivePlanCheckpoint | … (新增 Section 在此挂载)            │
└─────────────────────────────────────────────────────────────┘
                        ▲
                        │ include_str! / dev hot-reload
┌───────────────────────┴─────────────────────────────────────┐
│  prompt/templates/*.md （静态文案）                            │
│   role.md | behavioral_guidelines.md                          │
│   final_response_structure.md | run_mode.plan.md             │
│   run_mode.default.md | shell_tooling_guide.md | …           │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 核心新概念

#### 3.2.1 `PromptSurface`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PromptSurface {
    /// 主代理 system prompt（含 plan / default 两种 run_mode）
    MainAgent { run_mode: RunMode },
    /// 内置 explore helper
    SubagentExplore,
    /// 内置 review helper
    SubagentReview,
    /// 用户自定义子代理（使用 slug 标识）
    SubagentCustom { slug: String },
    /// 上下文压缩
    Compaction { kind: CompactionKind }, // Compact | Merge
    /// 会话标题生成
    Title,
}
```

每个 Section Source 自己声明匹配规则（见 § 3.2.6 `SurfaceMatcher`），由 Composer 在装配时筛选——**Surface 不再是 Provider 列表的隐式产物，而是一等公民**。

#### 3.2.2 `PromptLayer`（缓存友好分层）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PromptLayer {
    /// 跨会话稳定。任何与 thread/run/timestamp 相关的内容都禁止出现在这一层。
    /// 决定 LLM provider 端 prompt-prefix cache 的命中率。
    StablePrefix,
    /// 工作区/线程级稳定。同一线程内、不重置上下文之前不变。
    /// 例：Project Context、Profile Instructions、Run Mode、Skills 列表（快照）。
    SessionStable,
    /// 每次构建都可能变化的运行时数据。
    /// 例：Sandbox Policy、Workspace Path（无日期）。
    RuntimeOverlay,
    /// 一次性、随状态变化注入的瞬态。
    /// 例：Active Goal、Active Plan Checkpoint、Active Task Board 提示。
    Ephemeral,
}
```

> **关键决策**：原 `system_environment` 中的 `current_date` 必须从 `StablePrefix` 移除，改为 **runtime context message**（每个 turn 的 user/system 消息体），这与 `providers.rs:257` 注释意图一致，但目前实现是不一致的，本次重构修正。详见 § 3.7。

#### 3.2.3 输出契约：`ComposedPrompt` / `PromptBlock` / `CacheMarker`

为了与 Anthropic / Bedrock 等支持 prefix cache 的 LLM provider 对齐（cache 通过 content block 上的 `cache_control: { type: "ephemeral" }` 标记，**单请求最多 4 个 breakpoints**），Composer 输出 provider-agnostic 的内容块结构而非裸字节偏移：

```rust
pub struct ComposedPrompt {
    /// system prompt 完整文本（fallback：不支持 cache 的 provider 直接用此值）
    pub text: String,
    /// 内容块视图，按 Layer 切分；至多 4 个 cache marker
    pub blocks: Vec<PromptBlock>,
    /// 整体 schema 版本（结构变化即 bump），section 级版本见 audit
    pub schema_version: u32,
    pub audit: Vec<SectionAudit>,
    pub warnings: Vec<SectionWarning>,
}

pub struct PromptBlock {
    pub layer: PromptLayer,
    pub text: String,
    /// 是否在该块末尾设置 cache breakpoint
    pub cache_marker: Option<CacheMarker>,
}

pub enum CacheMarker {
    /// 对应 Anthropic `cache_control: { type: "ephemeral" }`
    Ephemeral,
    /// 留作未来扩展（持久化 / 会话级 cache）
    Persistent,
}
```

LLM provider 适配层负责把 `PromptBlock[]` 翻译为目标 API 格式：

| Provider | 翻译策略 |
|---|---|
| Anthropic Messages API | `system: [{type:"text", text, cache_control?}, …]` |
| Bedrock Anthropic | 同上 |
| OpenAI / 其他 | 拼接 `text` 字段，丢弃 `cache_marker` |

Composer 默认在 `StablePrefix` 末尾、`SessionStable` 末尾打 `Ephemeral` marker（共 2 个），余 2 个预算留给消息层（如 RAG 文档块、长 user message 前缀）。

#### 3.2.4 `SectionId`

类型化枚举（替换原 `&'static str` key）：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionId {
    Role,
    BehavioralGuidelines,
    FinalResponseStructure,
    ShellToolingGuide,
    Skills,
    SystemEnvironment,
    SandboxPermissions,
    ProjectContext,
    ProfileInstructions,
    RunMode,
    WorkspaceLocation,
    ActiveGoal,
    ActivePlan,
    SubagentOutputContract,
    /// Custom 子代理用户提供的 system prompt
    CustomSubagentBody,
    /// 任意第三方扩展通过 SectionId::Extension(&'static str) 接入
    Extension(&'static str),
}
```

类型化的好处：

- 编译期防止 typo
- 子代理"继承哪些 Section"用枚举集合表达，**不再依赖字符串标题匹配**
- 监控/审计字段可结构化导出

#### 3.2.5 `SectionSpec` & `SectionBody`

```rust
pub struct SectionSpec {
    pub id: SectionId,
    pub title: Cow<'static, str>,        // 渲染用，可 i18n
    /// 大多数 Section 全 Surface 同一 Layer，使用 LayerResolver::Fixed 即可；
    /// 跨 Surface 缓存语义不同的 Section 用 PerSurface（如 ProfileInstructions
    /// 在 Compaction 是 StablePrefix，在 MainAgent 是 SessionStable）
    pub layer: LayerResolver,
    /// 同 Layer 内排序；推荐使用 enum-based stable order，参见 § 3.4
    pub order_hint: SectionOrder,
    pub surfaces: SurfaceMatcher,
    /// 内容/结构变更必须 bump 此值；写入 ComposedPrompt.audit 与 agent_runs 审计表，
    /// 便于线上事故复盘与回放
    pub version: u32,
    /// 单 Section 长度上限（字符）；None 时使用 PromptBudget.per_section_default_chars
    pub max_chars: Option<usize>,
    pub source: Box<dyn SectionSource>,
}

pub enum LayerResolver {
    Fixed(PromptLayer),
    PerSurface(fn(&PromptSurface) -> PromptLayer),
}

pub struct SectionBody {
    /// 已渲染好的 Markdown 正文（不含 H2 标题）
    pub markdown: String,
    /// 可选元数据：估算 token 数、源文件路径等
    pub meta: SectionMeta,
}
```

#### 3.2.6 `SectionSource` trait（替代 `PromptSectionProvider`）

`build` 返回单一 `SectionOutcome` 枚举，避免 `Result<Option<…>, SoftError>` 的三值语义混乱：

```rust
#[async_trait]
pub trait SectionSource: Send + Sync {
    /// 该 Source 是否在当前 Surface + 上下文下可启用。
    /// 默认实现读取 SectionSpec.surfaces。
    fn enabled_for(&self, surface: &PromptSurface, cx: &BuildCx<'_>) -> bool { … }

    /// 声明依赖的"信号"。Composer 用它做并发调度与 dry-run。
    fn required_signals(&self) -> &'static [BuildSignal] { &[] }

    /// 真正的构建入口。
    /// 灾难性错误走 Result::Err（极少使用，例如 SQLite 连接致命断开）；
    /// 其他四种语义全部表达在 SectionOutcome 内。
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError>;
}

pub enum SectionOutcome {
    /// 不适用本次构建，无 warning（如 ActiveGoal 在没有 thread 时）
    Skip,
    /// 正常输出
    Produced(SectionBody),
    /// 部分降级仍输出（如 Skills 列表读取部分失败但有兜底）
    Degraded { body: SectionBody, warning: SectionWarning },
    /// 跳过 + warning（如 ProjectContext 读 AGENTS.md IO 失败）
    SoftFailed { code: &'static str, error: AppError },
}
```

**单一职责**：一个 Source 只产出**一个** Section。原 `BaseProvider` 产出 3 个 Section 的设计被拆成 `RoleSource`、`BehavioralGuidelinesSource`、`FinalResponseStructureSource`。

#### 3.2.7 `SurfaceMatcher` 与 `SurfacePattern`

由于 `PromptSurface::SubagentCustom { slug: String }` 每个用户自定义子代理都是独立 surface，简单的 `Only(Vec<PromptSurface>)` 无法表达"所有子代理"或"所有 custom 子代理"的通配——引入 `SurfacePattern`：

```rust
pub enum SurfacePattern {
    AnyMainAgent,
    MainAgent(RunMode),
    AnySubagent,
    BuiltinSubagent,        // Explore + Review
    CustomSubagent,         // 任意 slug
    Compaction(CompactionKind),
    AnyCompaction,
    Title,
}

pub enum SurfaceMatcher {
    All,
    Any(Vec<SurfacePattern>),
    Excluding(Vec<SurfacePattern>),
    /// 仅在前三种无法表达时使用；预期罕见
    Predicate(fn(&PromptSurface) -> bool),
}
```

例：

- `Role` → `All`（每个 Surface 都要）
- `BehavioralGuidelines` → `Any(vec![SurfacePattern::AnyMainAgent])`
- `Skills` → `Any(vec![SurfacePattern::MainAgent(RunMode::Default)])`（plan 模式下不暴露 skill 调用约定）
- `ActiveGoal` → `Any(vec![SurfacePattern::MainAgent(RunMode::Default)])`
- `SubagentOutputContract` → `Any(vec![SurfacePattern::AnySubagent])`
- 子代理继承的"系统环境/工作区指令/响应风格"由这些 Section 各自声明 `Any(vec![..., AnySubagent])`，而不是子代理端字符串解析

### 3.3 装配流程

```rust
pub async fn build(
    surface: PromptSurface,
    cx: BuildCx<'_>,
    registry: &SectionRegistry,
    budget: &PromptBudget,
) -> Result<ComposedPrompt, AppError> {
    // 1. 拣选
    let candidates: Vec<&SectionSpec> = registry
        .iter()
        .filter(|spec| spec.surfaces.matches(&surface))
        .collect();

    // 2. 并发构建（同 Layer 内并发，跨 Layer 顺序保留 deterministic ordering）
    //    SectionOutcome::Skip / SoftFailed → 不进入下一步；Degraded / Produced → 进入
    let mut bodies: Vec<RenderedSection> =
        join_all_collecting_outcomes(candidates, &cx).await;

    // 3. 解析每个 Section 的 Layer（PerSurface 在此处求值）
    bodies.iter_mut().for_each(|s| s.layer = s.spec.layer.resolve(&surface));

    // 4. per-section 长度检查 → 超限即截断 + warning
    enforce_per_section_budget(&mut bodies, budget);

    // 5. 排序：(Layer, SectionOrder, SectionId 字典序作为 tie-breaker，保证可重现)
    bodies.sort_by_key(|s| (s.layer, s.spec.order_hint, s.spec.id.clone()));

    // 6. 全局长度检查 → 按 budget.eviction_order 驱逐 / 截断关键 Section
    enforce_total_budget(&mut bodies, budget);

    // 7. 渲染为 PromptBlock[] + 在 StablePrefix / SessionStable 末尾打 cache marker
    render_blocks(bodies, surface, registry.schema_version())
}
```

### 3.4 排序：`SectionOrder` 取代裸 `u16`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionOrder {
    First,                  // 锚定头部
    Anchored(SectionAnchor),// 相对锚点定位（before/after 某 SectionId）
    Default,                // 默认槽
    Last,                   // 锚定尾部
}
```

`SectionAnchor::After(SectionId::Role)` 比裸 `order_in_phase = 20` 更具语义；新增 Section 不需要"猜数字"。

### 3.5 Layer × Surface 决策矩阵（默认）

| Section | MainAgent | Subagent* | Compaction | Title | LayerResolver |
|---|---|---|---|---|---|
| Role | ✓ | ✓ (按需重写) | – | – | `Fixed(StablePrefix)` |
| BehavioralGuidelines | ✓ | – | – | – | `Fixed(StablePrefix)` |
| FinalResponseStructure | ✓ | – | – | – | `Fixed(StablePrefix)` |
| ShellToolingGuide | ✓ | ✓（按 helper 重写） | – | – | `Fixed(StablePrefix)` |
| SystemEnvironment（无日期） | ✓ | ✓（继承） | – | – | `Fixed(StablePrefix)` |
| Skills | ✓ (default mode) | – | – | – | `Fixed(SessionStable)` |
| ProfileInstructions | ✓ | ✓（继承） | ✓ | ✓ | `PerSurface`：MainAgent/Subagent → `SessionStable`；Compaction/Title → `StablePrefix` |
| ProjectContext | ✓ | ✓（继承） | – | – | `Fixed(SessionStable)` |
| RunMode | ✓ | – | – | – | `Fixed(SessionStable)` |
| SandboxPermissions | ✓ | – | – | – | `Fixed(RuntimeOverlay)` |
| WorkspaceLocation | ✓ | ✓ | – | – | `Fixed(RuntimeOverlay)` |
| ActiveGoal | ✓ (default mode) | – | – | – | `Fixed(Ephemeral)` |
| ActivePlan | ✓ | – | – | – | `Fixed(Ephemeral)` |
| SubagentOutputContract | – | ✓ | – | – | `Fixed(StablePrefix)` |
| CustomSubagentBody | – | ✓ (Custom) | – | – | `Fixed(SessionStable)`，profile 声明 `cache_stability: stable` 时升至 `StablePrefix` |
| CompactionContract | – | – | ✓ | – | `Fixed(StablePrefix)` |
| TitleContract | – | – | – | ✓ | `Fixed(StablePrefix)` |

> **当前日期** 不再是任何 Section 的一部分。它通过 `RuntimeMessageInjector`（参见 § 3.7）作为**消息层**注入，每轮 turn 才更新一次。
>
> **CustomSubagentBody 默认 SessionStable**：用户自定义 prompt 可能含日期、冲刺名、动态指令，强行标记 StablePrefix 会让缓存命中率长期低位震荡。profile YAML 增加 `cache_stability: stable` 字段，让用户**主动承诺**该 prompt 不含瞬态内容，由 Composer 据此提升 Layer。

### 3.6 BuildCx：上下文聚合 + 软依赖 + 信号缓存

```rust
pub struct BuildCx<'a> {
    pub pool: &'a SqlitePool,
    pub workspace_path: &'a str,
    pub thread_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub raw_plan: Option<&'a RuntimeModelPlan>,
    pub run_mode: RunMode,
    pub helper_profile: Option<&'a SubagentProfile>,
    /// 信号缓存：Source 通过 cx.signal::<T>() 查询并自动 memoize；
    /// 同一 signal 并发请求共享一个 Shared<Future>，避免重复 DB 查询
    pub signals: SignalCache,
    /// 软配置：feature flag、A/B 实验、按模型 capability 切换；
    /// 通过 BuildCx 注入而非修改 registry，hot-path 无锁
    pub features: PromptFeatureSet,
}

pub struct SignalCache {
    /// TypeId → Shared future。生命周期同 BuildCx（一次 build），不跨 build 共享，避免脏读
    inner: Arc<Mutex<HashMap<TypeId, Shared<BoxFuture<'static, Arc<dyn Any + Send + Sync>>>>>>,
}
```

`Composer` 进程内单例 `Arc<Composer>`，registry 不可变；`PromptFeatureSet` 走 `BuildCx` 而非 registry，便于 A/B 实验热切换。

### 3.7 后处理：`RuntimeMessageInjector` 与压缩交互

原 `inject_goal_context` 改为 `ActiveGoalSource`（Ephemeral Layer）。真正不能进 system prompt 的运行时变量（**当前日期、当前时间戳、活跃 PR 状态**）改为 **runtime user/system message** 注入：

```rust
pub trait RuntimeMessageInjector: Send + Sync {
    fn applies_to(&self, surface: &PromptSurface) -> bool;
    async fn build_message(&self, cx: &BuildCx<'_>) -> Option<RuntimeMessage>;
}

pub struct RuntimeMessage {
    pub text: String,
    pub kind: RuntimeMessageKind,
    pub compaction_policy: CompactionPolicy,
}

pub enum CompactionPolicy {
    /// 默认：可被压缩链吞掉，下次 turn 重新注入
    AbsorbAndReinject,
    /// 排除在压缩窗口外（如当前日期、当前 PR 状态）；
    /// 防止 summary-of-summary 把它卷入摘要后下次又重新注入造成"双份"
    PinOutsideWindow,
}
```

例：`CurrentDateInjector` 在每个 turn 启动前向 messages 列表头部插一条形如：

```
<runtime_context turn_started_at="2026-06-05T03:21:11Z">
Current date: 2026-06-05
</runtime_context>
```

且使用 `PinOutsideWindow`，由消息序列化层标记该消息为不可压缩。

`CurrentDateInjector.applies_to` 默认覆盖**所有需要时间感知的 surface**（MainAgent + Subagent*），review 子代理审 PR 时间敏感场景同样需要。

这样 system prompt 完全稳定，prompt-prefix cache 命中率最大化。

### 3.8 子代理构建（关键修复）

```rust
let composed = composer.build(
    PromptSurface::SubagentExplore,
    BuildCx::derive_for_helper(parent_cx, &helper_profile),
    &registry,
    &budget,
).await?;
```

子代理**直接调用 Composer**，不再字符串解析父 prompt。继承通过 `SurfaceMatcher` 在 `SystemEnvironment`、`ProjectContext`、`ProfileInstructions` 等 Section 上声明：

```rust
SectionSpec {
    id: SectionId::ProfileInstructions,
    surfaces: SurfaceMatcher::All, // 主代理、所有子代理、压缩、标题都需要
    …
}
```

子代理特有的 `SubagentOutputContract`、helper 版 `ShellToolingGuide` 通过 `SurfaceMatcher::Any(vec![SurfacePattern::AnySubagent])` 加入。

`SubagentProfile::system_prompt()` 这种"硬编码巨型字符串"也外置到 `templates/subagent/explore.md`、`templates/subagent/review.md`，由 `SubagentBodySource` 加载。

> **迁移安全网**：因 LLM 对 system prompt 微小变化敏感，子代理切换分 2a / 2b 两步，详见 § 4 阶段 2。

### 3.9 静态文案外置

新建：

```
src-tauri/src/core/prompt/templates/
    role.md
    behavioral_guidelines.md
    final_response_structure.md
    shell_tooling_guide.md
    run_mode.plan.md
    run_mode.default.md
    skills_usage.md
    sandbox_permissions.tpl.md          # 含 {{approval_policy}} 等占位符
    active_goal.tpl.md
    subagent/explore.md
    subagent/review.md
    subagent/output_contract.explore.md
    subagent/output_contract.review.md
    compaction/compact.md
    compaction/merge.md
    title/contract.md
```

加载方式：

```rust
fn load_template(rel_path: &str, embedded: &'static str) -> Cow<'static, str> {
    // dev-only 热重载：未命中时回退到 include_str! 编译期常量
    #[cfg(debug_assertions)]
    if let Ok(s) = std::fs::read_to_string(template_root().join(rel_path)) {
        return Cow::Owned(s);
    }
    Cow::Borrowed(embedded)
}

// 调用点：
let tpl = load_template("role.md", include_str!("templates/role.md"));
```

带占位符的模板走**严格模式**：

```rust
pub fn render_template_strict(
    tpl: &str,
    declared_keys: &[&'static str],
    vars: &TemplateVars,
) -> Result<String, TemplateError>;
```

- 渲染时缺键 → `Err(TemplateError::MissingKey)`，由 SectionSource 转为 `SectionOutcome::SoftFailed`，**不静默拼接残缺文本**
- 启动期 lint 测试扫描 `templates/**/*.md`，提取所有 `{{key}}`，与代码端 `declared_keys` 比对，杜绝模板新增占位符忘记声明：

```rust
#[cfg(test)]
mod template_lints {
    #[test]
    fn templates_have_no_undeclared_keys() { … }
    #[test]
    fn declared_keys_have_no_dead_entries() { … }
}
```

> **不引入 handlebars/tera**——避免运行时模板错误风险与依赖膨胀；仅做"双花括号占位符"替换即可覆盖现有需求。

收益：

- 文案 diff 直接可读（`git diff templates/behavioral_guidelines.md` 行级清晰）
- 非工程同事可在 IDE 中直接编辑（grammarly、CSpell、PR review）
- 长度变化能在 PR 审计中显式看到
- 编译期常量保留（`include_str!` 不增加运行时开销），dev 模式下额外支持热重载

### 3.10 失败软降级

错误语义统一在 `SectionOutcome` 内（见 § 3.2.6）：

| 状态 | Composer 行为 | 何时使用 |
|---|---|---|
| `Skip` | 静默丢弃 | 不适用本次构建（如 ActiveGoal 在没有 thread 时） |
| `Produced(body)` | 入列 | 正常 |
| `Degraded { body, warning }` | 入列 + 记录 warning | 部分降级仍可用（如 Skills 部分加载失败但有兜底） |
| `SoftFailed { code, error }` | 跳过 + warning + audit `fallback_used = true` | 整段无法生成（如 ProjectContext IO 失败） |
| `Result::Err(FatalError)` | 整体 build 失败 | 极少使用：例如 Role 模板加载失败、SQLite 致命断开 |

关键 Section（Role、BehavioralGuidelines）若失败必须 `FatalError`；非关键（Skills、ProjectContext、ActiveGoal、CustomSubagentBody）默认走 `SoftFailed` / `Degraded`。

### 3.11 可观测性

`ComposedPrompt` 输出审计：

```rust
pub struct ComposedPrompt {
    pub text: String,
    pub blocks: Vec<PromptBlock>,
    pub schema_version: u32,
    pub audit: Vec<SectionAudit>,
    pub warnings: Vec<SectionWarning>,
}

pub struct SectionAudit {
    pub id: SectionId,
    pub layer: PromptLayer,
    pub version: u32,
    pub bytes: usize,
    pub estimated_tokens: usize,
    pub source_kind: &'static str,
    pub elapsed: Duration,
    pub fallback_used: bool,
    pub truncated: bool,
}
```

埋点输出到现有 `tracing`，所有字段过 `Redactor` 脱敏（替换 `$HOME` 为 `~`、用户名片段、token 字面量、绝对工作区路径）：

```rust
pub trait Redactor: Send + Sync {
    fn redact(&self, raw: &str) -> Cow<'_, str>;
}

tracing::info!(
    target = "prompt.compose",
    surface = %surface,
    schema_version = composed.schema_version,
    sections = audit.len(),
    bytes = composed.text.len(),
    estimated_tokens = audit.iter().map(|a| a.estimated_tokens).sum::<usize>(),
    warnings = composed.warnings.len(),
    truncated_sections = audit.iter().filter(|a| a.truncated).count(),
    fallback_sections = audit.iter().filter(|a| a.fallback_used).count(),
    "system prompt composed",
);
```

`schema_version` + 每 Section 的 `version` 写入 `agent_runs` 表的审计字段，便于线上事故复盘"这次 run 用的是哪个版本的 system prompt"。

可选 `#[cfg(debug_assertions)]` 下额外 `dry_run()` 接口用于本地预览/测试。

### 3.12 长度预算 `PromptBudget`

防止极端工作区下 system prompt 无限膨胀吃光 user message 上下文窗口：

```rust
pub struct PromptBudget {
    /// 全局上限（字符数；按 model context window 安全占比计算，默认 ~30%）
    pub total_chars: usize,
    pub per_section_default_chars: usize,
    pub per_section_overrides: BTreeMap<SectionId, usize>,
    /// 超额时按此顺序逐 Layer 回收 Section
    pub eviction_order: Vec<PromptLayer>,
    // 默认：[Ephemeral, RuntimeOverlay, SessionStable, StablePrefix]
}
```

Composer 行为：

1. **per-section 检查**：每个 Source 返回后，若 `body.markdown.len()` 超出 `per_section_overrides` 或 `per_section_default_chars` → `body.truncate_with_marker()`（保留头/尾 + `… [truncated N chars] …`），写 `SectionWarning::Truncated`，audit `truncated = true`
2. **全局检查**：所有 Section 渲染完后若 total 超限 → 按 `eviction_order` 删 Section（先丢 Ephemeral 中 `order_hint` 最大的；同 Layer 内按 size 降序选择）
3. **底线保护**：仍超限 → StablePrefix 内的 Section 截断而非删除（删除会破坏行为契约）
4. 全程审计落 `ComposedPrompt.warnings`，触发 `prompt.budget.truncated` / `prompt.budget.evicted` metric，超阈值告警

### 3.13 StablePrefix 纯净性 lint

新增 `cargo test prompt::cache_purity` 强制 StablePrefix 内不出现瞬态字面量：

1. 用 fixture（含已知日期、thread_id、run_id、用户名）调用 Composer 渲染所有 Surface
2. 提取 `PromptBlock { layer: StablePrefix, .. }` 拼接文本
3. 正则禁词集匹配：
   - `\b\d{4}-\d{2}-\d{2}\b`（ISO date）
   - `\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}`（timestamp）
   - fixture 注入的 thread_id / run_id 字面量（防回归到把 ID 写进 Role/SystemEnvironment）
   - fixture 注入的用户名 / `$HOME` 路径片段
4. 命中即测试失败；失败信息打印命中 Section + 具体片段

CI 强制此测试，保证 LLM provider 端 prefix cache 命中率不被悄悄破坏。

---

## 四、迁移步骤（增量、可灰度）

### 阶段 0：脚手架（不改语义）

1. 在 `prompt/` 下新增模块：`layer.rs`、`surface.rs`、`section_id.rs`、`registry.rs`、`composer.rs`、`signals.rs`、`templates.rs`、`budget.rs`、`runtime_message.rs`，但**不接通**到 `agent_session`
2. 引入新类型：`SectionOutcome`、`SurfacePattern`/`SurfaceMatcher`、`LayerResolver`、`PromptBlock`/`CacheMarker`、`PromptBudget`、`schema_version`，仅在适配层使用，不影响行为
3. 新增 `prompt/templates/*.md` 目录，仅复制（不修改）现有字面量；模板严格模式 + 启动期 lint 测试上线
4. 新增 `SectionSource` trait 与适配器 `LegacyProviderAdapter`，把现有 5 个 `*Provider` 包成 `SectionSource`，但仍允许旧路径并存

### 阶段 1：装配器双轨（主代理 byte-equal 切换）

1. 实现 `Composer::build_main_agent_legacy_compat()`，输出**与现状 byte-equal**（含 phase / order_in_phase 的兼容映射）
2. 加入快照测试：`assert_eq!(legacy_build_system_prompt(...), composer.build_main_agent_legacy_compat(...))`，覆盖：
   - `run_mode = "default"` × 有/无 AGENTS.md × 有/无 Skills × 有/无 Profile × Sandbox 4 种 policy
   - `run_mode = "plan"` 同上
3. 校验 `ComposedPrompt.schema_version` 与每 Section `version` 被正确写入 audit 表
4. 切换 `agent_session::build_system_prompt` 调用到 Composer，保留旧实现一周作为 fallback

### 阶段 2：Surface 化子代理（拆 2a / 2b）

**2a — 双轨观测**：

1. 新增 `SubagentOutputContract`、`ShellToolingGuide(helper)` 等 Section 进入 Registry
2. 保留 `build_helper_system_prompt` 作为生产路径；同时调用 Composer 生成对照版本，**仅记录 hash + length 差异**到 metrics（`prompt.subagent.hash_match`、`prompt.subagent.diff_bytes`）
3. 灰度 7 天，观察 hash_match ≥ 99 % 后进入 2b；不达标 → 回查差异、修补 Source、继续观测

**2b — 切换**：

1. `SubagentProfile::system_prompt` 改为通过 `Composer::build(SubagentExplore, …)` 渲染
2. **删除** `orchestrator.rs::collect_prompt_sections` + `inherited_helper_prompt_sections` + `is_helper_inherited_section`（字符串解析反模式）
3. 子代理快照测试改为对比 Composer 输出
4. CustomSubagent 切换最后进行：profile 配置文件迁移加 `cache_stability` 字段

### 阶段 3：缓存边界与日期外移

1. 把 `current_date` 从 `SystemEnvironment` 移除；新增 `CurrentDateInjector` 注入到消息层（带 `CompactionPolicy::PinOutsideWindow`）
2. 启用 `PromptBlock` + `CacheMarker`；下游 LLM provider 适配层完成（Anthropic：StablePrefix 末尾 + SessionStable 末尾各一个 `cache_control: ephemeral`；不支持的 provider 忽略）
3. 上线 `cache_purity` lint，CI 强制
4. 监控指标：上线前后对比相同会话的 system prompt 字节哈希分布——稳定 prefix 比例应显著上升；prompt-prefix cache 命中率应显著上升

### 阶段 4：Goal 等 Ephemeral 归位

1. `inject_goal_context` 删除；改为 `ActiveGoalSource: SectionSource`，layer = `Fixed(Ephemeral)`
2. 随后接入 `ActivePlanSource`、`ActiveTaskBoardHintSource`，验证扩展性
3. 此时新增 Ephemeral Section 应**只动一个文件**（`sources/active_xxx.rs`）+ 一行 registry.register

### 阶段 5：模板外置 & 文案治理

1. 把 `behavioral_guidelines.md`、`final_response_structure.md`、`run_mode.*.md` 实际从 `.rs` 移到 `.md`
2. 启用模板严格模式：缺键直接走 `SoftFailed`，禁止 prod 静默拼接残缺文本
3. 引入 `prompt-snapshot` 测试套件：每个 Surface × 关键 fixture 输出一份 `.snap`，PR 阶段任何改动都会显式 diff

### 阶段 6：散落入口归并

1. `agent_run_summary::build_compact_summary_system_prompt` 改为 `Composer::build(Compaction { kind: Compact }, …)`
2. 同样处理 `build_merge_summary_system_prompt`、`build_title_prompt_from_messages`
3. 删除重复的 `response_language` / `response_style` 拼接逻辑——统一在 `ProfileInstructionsSource` 内

### 阶段 7：可观测、灰度与告警

1. 接通 `tracing` 与现有 metrics 通道；为 PromptComposer 添加 dashboards 字段
2. 引入 `PromptFeatureSet`：用于 A/B 控制（例如 `enable_skills_brief: bool`），便于线上灰度新文案而无需立即下线旧版本
3. 上线核心告警阈值：
   - `prompt.budget.evicted_ratio > 0.5%` → P2
   - `prompt.budget.truncated_ratio > 1%` → P2
   - `prompt.subagent.hash_match < 99%`（双轨期）→ P1
   - `prompt.section.fallback{…} > 1%` → P2
   - `prompt.cache_purity_violations > 0`（CI 拦截）→ P0

---

## 五、目录结构（重构后）

```
src-tauri/src/core/prompt/
├── mod.rs                     # pub use composer::*; pub use surface::*; …
├── composer.rs                # PromptComposer + ComposedPrompt + 渲染逻辑
├── registry.rs                # SectionRegistry + 默认注册函数 + schema_version
├── surface.rs                 # PromptSurface, SurfacePattern, SurfaceMatcher
├── layer.rs                   # PromptLayer, LayerResolver, SectionOrder, SectionAnchor
├── section.rs                 # SectionId, SectionSpec, SectionBody, SectionOutcome, SectionAudit
├── source.rs                  # SectionSource trait, BuildCx, BuildSignal, FatalError
├── signals.rs                 # SignalCache + 内置 signal（policy / writable_roots / …）
├── templates.rs               # 占位符渲染器（严格模式 + dev 热重载 + lint）
├── budget.rs                  # PromptBudget + 截断/驱逐策略
├── runtime_message.rs         # RuntimeMessageInjector + CompactionPolicy + CurrentDateInjector
├── redactor.rs                # PII 脱敏（tracing 字段 + warning 落库前过滤）
├── sources/
│   ├── mod.rs
│   ├── role.rs
│   ├── behavioral_guidelines.rs
│   ├── final_response_structure.rs
│   ├── shell_tooling_guide.rs
│   ├── system_environment.rs
│   ├── sandbox_permissions.rs
│   ├── project_context.rs
│   ├── skills.rs
│   ├── profile_instructions.rs
│   ├── run_mode.rs
│   ├── workspace_location.rs
│   ├── active_goal.rs
│   ├── active_plan.rs
│   ├── subagent_output_contract.rs
│   ├── custom_subagent_body.rs
│   ├── compaction_contract.rs
│   └── title_contract.rs
└── templates/
    ├── role.md
    ├── behavioral_guidelines.md
    ├── final_response_structure.md
    ├── shell_tooling_guide.md
    ├── run_mode.plan.md
    ├── run_mode.default.md
    ├── sandbox_permissions.tpl.md
    ├── skills_usage.md
    ├── active_goal.tpl.md
    ├── subagent/
    │   ├── explore.md
    │   ├── review.md
    │   ├── output_contract.explore.md
    │   └── output_contract.review.md
    ├── compaction/
    │   ├── compact.md
    │   └── merge.md
    └── title/
        └── contract.md
```

---

## 六、典型用法示例

### 6.1 主代理

```rust
let composer = composer::default();
let composed = composer
    .build(
        PromptSurface::MainAgent { run_mode: RunMode::Default },
        BuildCx::for_main_agent(pool, &raw_plan, workspace_path, thread_id),
        &budget,
    )
    .await?;

// 后续传给 LLM provider 适配层；适配层根据 provider 决定如何下发：
//   Anthropic: composed.blocks → system: [{type:"text", text, cache_control?}, …]
//   其他: composed.text 整段下发
agent.set_system_prompt_blocks(composed.blocks);
```

### 6.2 Subagent

```rust
let composed = composer
    .build(
        PromptSurface::SubagentExplore,
        BuildCx::for_helper(parent_cx, &helper_profile),
        &budget,
    )
    .await?;
agent.set_system_prompt_blocks(composed.blocks);
```

### 6.3 新增一个 Section（只动一个文件）

```rust
// src-tauri/src/core/prompt/sources/active_plan.rs
pub struct ActivePlanSource;

#[async_trait]
impl SectionSource for ActivePlanSource {
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let Some(thread_id) = cx.thread_id else { return Ok(SectionOutcome::Skip) };
        let plan = match plan_checkpoint::load(cx.pool, thread_id).await {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(SectionOutcome::Skip),
            Err(e) => return Ok(SectionOutcome::SoftFailed {
                code: "plan.load_failed",
                error: e,
            }),
        };

        let body = render_template_strict(
            include_str!("../templates/active_plan.tpl.md"),
            &["plan_revision", "plan_summary"],
            &TemplateVars::new()
                .insert("plan_revision", plan.revision)
                .insert("plan_summary", &plan.summary),
        ).map_err(|e| FatalError::Template(e))?;

        Ok(SectionOutcome::Produced(SectionBody::markdown(body)))
    }
}

// 在 registry.rs::default_registry() 末尾追加：
registry.register(SectionSpec {
    id: SectionId::ActivePlan,
    title: Cow::Borrowed("Active Plan"),
    layer: LayerResolver::Fixed(PromptLayer::Ephemeral),
    order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::ActiveGoal)),
    surfaces: SurfaceMatcher::Any(vec![SurfacePattern::MainAgent(RunMode::Default)]),
    version: 1,
    max_chars: Some(2_000),
    source: Box::new(ActivePlanSource),
});
```

新增一项**不需要触碰 composer / 不需要改其他 Section / 不需要分配魔法数字**。

---

## 七、测试策略

| 层 | 工具 | 覆盖目标 |
|---|---|---|
| 单元（Source） | `tokio::test` + 内存 SQLite fixture | 每个 Source 的 `Skip / Produced / Degraded / SoftFailed` 四态 |
| 单元（Composer） | mock Source 列表 | Layer 排序、SurfaceMatcher、依赖循环检测、并发软失败聚合、budget 截断/驱逐 |
| 模板 lint | `cargo test prompt::templates::lints` | 模板 `{{key}}` ↔ 代码 `declared_keys` 双向一致；无遗漏、无死键 |
| 缓存纯净性 | `cargo test prompt::cache_purity` | StablePrefix 内禁止出现 `\d{4}-\d{2}-\d{2}` / thread_id / run_id / 用户名 字面量 |
| 快照 | `insta` 或自研 `.snap` | 每个 Surface × 关键 fixture 的完整渲染；任何文案变更都触发 diff |
| 兼容（阶段 1） | byte-equal 双轨对比 | 旧 `build_system_prompt` ↔ 新 `Composer::build_main_agent_legacy_compat` |
| 兼容（阶段 2a） | hash 观测指标 | 子代理新旧 prompt 的 hash_match ≥ 99 % 才进入 2b |
| 子代理 | 现有 `helper_system_prompt_*` 测试改写 | 验证不再依赖父 prompt 字符串解析 |
| 性能 | `criterion` | 单次 build 总耗时 < 5 ms（命中 SignalCache 时） |
| 预算 | 单测 + fuzzing | 制造 100 KB Skills 输出 → 验证 truncate 后总长 ≤ budget；驱逐顺序符合 `eviction_order` |

---

## 八、风险与回滚

| 风险 | 缓解 |
|---|---|
| 文案语义在迁移过程中出现微小漂移 | 阶段 1 强制主代理 byte-equal；阶段 2a 强制子代理 hash 观测 ≥ 7 天；任何 diff 必须显式批准 |
| Layer 划分错误导致缓存命中率下降 | `cache_purity` 测试 + 上线灰度 5% → 50% → 100%；监控 prompt 字节哈希集合大小 |
| 子代理继承遗漏导致行为退化 | 子代理 `.snap` 全量比对 + 2a 双轨观测；首批仅切换 `SubagentExplore`，验证一周再切 `Review` / `Custom` |
| 软失败掩盖真问题 | `tracing::warn!` + 计数器；超阈值（例如 `prompt.section.fallback{...} > 1%`）告警 |
| 模板加载错误（路径错） | `include_str!` 编译期失败，零运行时风险；dev 模式热重载失败回退到编译期常量 |
| 模板缺占位符 | 严格模式 → `SoftFailed`，绝不静默拼接；启动期 lint 测试拦截 |
| Budget 误删关键 Section | StablePrefix 走截断而非删除；`eviction_order` 默认末位是 StablePrefix |
| RuntimeMessage 与压缩链双份注入 | `CompactionPolicy::PinOutsideWindow` 标记，消息序列化层强制不压缩 |
| schema 升级导致回放失败 | `ComposedPrompt.schema_version` + 每 Section `version` 写审计表，回放时按版本号选 source 实现 |
| 新增依赖引入复杂度 | 仅引入 `async-trait`（已有）+ 一个 ~50 行的占位符渲染器；不引入 handlebars / tera |

回滚路径：阶段 1 完成前可整体回退到旧 `build_system_prompt`；阶段 1 之后通过 feature flag `PROMPT_COMPOSER_V2 = false` 走兼容分支，保留至少 1 个版本。

---

## 九、收益总结

| 维度 | 现状 | 重构后 |
|---|---|---|
| 新增 Section | 改 `assembler` + `providers.rs` + 选 phase + 选 order + 写测试 | 新建一个 `sources/xxx.rs` + 一行 `registry.register` |
| 新增 Surface | 复制粘贴整套 prompt 构建逻辑 | 在 `PromptSurface` 枚举加一个变体 + 标注现有 Section 的 `SurfaceMatcher` |
| 文案修改 | 改 .rs 大字符串字面量，diff 噪音大 | 改 `templates/*.md`，行级 diff，非工程也可改 |
| 子代理继承 | 字符串解析反模式，格式微调即破坏 | 类型化 `SectionId` + `SurfaceMatcher`，编译期保证 |
| 缓存命中率 | StablePrefix 中混入 `current_date`，每天命中率清零 | 显式 4 层 + RuntimeMessageInjector + cache marker，prefix 跨日跨会话稳定；`cache_purity` 测试守底 |
| Goal / Plan / Board 注入 | 各自字符串拼接 | 统一为 `Ephemeral` Layer 的 Section |
| 失败处理 | 任意 Provider 抛错 → system prompt 构建失败 → 整次 run 失败 | `SectionOutcome` 四态语义清晰；软失败保留主代理可用；warning 上报 |
| 长度控制 | 无 | `PromptBudget` 全局 + per-section 限额 + 按 Layer 驱逐/截断 |
| 缓存契约 | 无 | `PromptBlock + CacheMarker`，与 Anthropic / Bedrock API 对齐 |
| 可观测 | 无 | `SectionAudit`（含 version / truncated / fallback_used）+ tracing + Redactor 脱敏 + 告警阈值 |
| 多 Surface 公用原语 | summary / title / subagent 各写各的"响应语言/风格" | 同一 `ProfileInstructionsSource` 在所有 Surface 复用；`LayerResolver::PerSurface` 处理跨 Surface 缓存语义差异 |
| 测试覆盖 | 2 个零碎单测 | 每个 Source 四态单测 + 全 Surface 快照 + 兼容双轨 + 缓存纯净性 + 模板 lint + 预算 fuzz |
| 事故复盘 | 无版本信息 | `schema_version` + 每 Section `version` 写 `agent_runs`，按版本回放 |

---

## 十、附录：与现有代码的对照表

| 现有符号 | 重构后映射 |
|---|---|
| `prompt::build_system_prompt` | `Composer::build(PromptSurface::MainAgent { .. }, …)` |
| `PromptSection { key, title, body, phase, order_in_phase }` | `SectionSpec { id, title, layer: LayerResolver, order_hint, surfaces, version, max_chars, source }` + `SectionBody` |
| `PromptSectionProvider::collect` | `SectionSource::build`（一对多 → 一对一拆分；返回 `SectionOutcome` 四态） |
| `PromptPhase::Core/Capability/WorkspacePreference/RuntimeContext` | `PromptLayer::StablePrefix/SessionStable/RuntimeOverlay/Ephemeral`（语义更聚焦于"缓存 + 变化频率"） |
| `BaseProvider`（产 3 个 Section） | `RoleSource` + `BehavioralGuidelinesSource` + `FinalResponseStructureSource`（单一职责） |
| `WorkspaceProvider` | `ProjectContextSource` |
| `EnvironmentProvider`（产 3 个 Section） | `SystemEnvironmentSource`（去掉 current_date）+ `SandboxPermissionsSource` + `ShellToolingGuideSource` |
| `SkillsProvider` | `SkillsSource` |
| `ProfileProvider`（产 3 个 Section） | `ProfileInstructionsSource` + `RunModeSource` + `WorkspaceLocationSource` |
| `inject_goal_context`（事后字符串拼接） | `ActiveGoalSource`（Ephemeral Layer） |
| `system_environment.current_date` | `CurrentDateInjector`（RuntimeMessage，`PinOutsideWindow`） |
| `build_helper_system_prompt`（字符串解析继承） | `Composer::build(PromptSurface::SubagentExplore, …)` |
| `collect_prompt_sections`（按 `## ` 解析） | **删除** |
| `SubagentProfile::system_prompt`（硬编码字符串） | `templates/subagent/{explore,review}.md` + `SubagentBodySource` |
| `build_compact_summary_system_prompt` | `Composer::build(PromptSurface::Compaction { kind: Compact }, …)` |
| `build_merge_summary_system_prompt` | `Composer::build(PromptSurface::Compaction { kind: Merge }, …)` |
| `build_title_prompt_from_messages` 中的 `system_prompt` | `Composer::build(PromptSurface::Title, …)` |
