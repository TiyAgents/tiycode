# Prompt Composition Engine

系统 Prompt 的装配引擎。将 Prompt 构建从硬编码字符串拼接升级为**类型化、分 Layer、可降级、可审计**的组合管线。

## 架构总览

```
调用方 (agent_session / subagent / compaction / title)
                │ build(surface, BuildCx, PromptBudget)
                ▼
┌─────────────────────────────────────────────────────────┐
│                    PromptComposer                       │
│  ① 按 SurfaceMatcher 拣选 Section                       │
│  ② 并发构建 Section（超时 + 软失败）                     │
│  ③ 按 Layer × SectionOrder 排序                         │
│  ④ per-section / 全局 预算检查 + 截断 / 驱逐             │
│  ⑤ 渲染为 PromptBlock[] + 打 CacheMarker               │
│                    ▼                                    │
│              ComposedPrompt {                           │
│                text, blocks: [PromptBlock],             │
│                schema_version, audit, warnings          │
│              }                                          │
└─────────────────────────────────────────────────────────┘
                │ 注册查询
                ▼
┌─────────────────────────────────────────────────────────┐
│  SectionRegistry（17 个 Section，单例）                   │
│  每个 Section 声明：id / title / layer / order /        │
│  surfaces / version / max_chars / source                │
└─────────────────────────────────────────────────────────┘
                ▲
                │ include_str!（debug 模式支持热重载）
┌─────────────────────────────────────────────────────────┐
│  prompt/templates/*.md（静态文案 + YAML front-matter）    │
└─────────────────────────────────────────────────────────┘
```

## 设计支柱

- **Layer × Surface 双轴分离**：Section 是可独立演进的最小单元。新增 Surface 不需要修改装配器。
- **类型化数据流**：`SectionId` 枚举 + `SectionSource` trait + `SectionOutcome` 四态，消除字符串拼接反模式。
- **缓存友好**：`PromptBlock` + `CacheMarker` 显式分层（StablePrefix → SessionStable → RuntimeOverlay → Ephemeral），与 Anthropic `cache_control` 对齐。
- **失败软降级**：非关键 Section 失败走 `SoftFailed` / `Degraded`，不阻塞整体构建。
- **禁止 inter-section 依赖**：Section 之间仅通过 `BuildSignal` 共享数据，Composer 调度退化为扁平并发 + Layer 排序。
- **运行时数据外移**：`current_date` 等瞬态变量通过 `RuntimeMessageInjector` 注入到消息层，system prompt 永久稳定。

## 核心概念

### `PromptSurface` — Prompt 的使用场景

```rust
pub enum PromptSurface {
    MainAgent { run_mode: RunMode },
    SubagentExplore { inherited_run_mode: RunMode },
    SubagentReview { inherited_run_mode: RunMode },
    SubagentCustom { slug: String, inherited_run_mode: RunMode, cache_stability: SubagentCacheStability },
    Compaction { kind: CompactionKind },  // Compact | Merge
    Title,
}
```

每个 Surface 确定需要哪些 Section。新增 Surface 在枚举上加一个变体即可，不需改装配器。

### `PromptLayer` — 缓存稳定性分层

| Layer | 含义 | 示例 Content |
|---|---|---|
| `StablePrefix` | 跨会话稳定 | Role, BehavioralGuidelines, FinalResponseStructure |
| `SessionStable` | 线程级稳定 | Skills, ProjectContext, ProfileInstructions |
| `RuntimeOverlay` | 每次构建可能变 | SystemEnvironment (无日期), RunMode, WorkspaceLocation |
| `Ephemeral` | 一次性瞬态 | ActiveGoal, ActivePlan |

`Ephemeral` 层永远不打 CacheMarker。`current_date` 等瞬态变量不进入任何 Layer，而是通过 `RuntimeMessageInjector` 注入到消息层。

### `SectionId` — 类型化 Section 标识

```rust
pub enum SectionId {
    Role, BehavioralGuidelines, FinalResponseStructure,
    ShellToolingGuide, Skills, SystemEnvironment, SandboxPermissions,
    ProjectContext, ProfileInstructions, RunMode, WorkspaceLocation,
    ActiveGoal, ActivePlan,
    SubagentOutputContract, CustomSubagentBody,
    CompactionContract, TitleContract,
    Extension(&'static str),  // 第三方扩展点
}
```

替换旧 `&'static str` key 模式，编译期防止 typo。

### `SectionSource` trait — 单一职责的内容生产者

```rust
#[async_trait]
pub trait SectionSource: Send + Sync {
    fn source_kind(&self) -> &'static str;
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError>;
}
```

一个 Source 只产出**一个** Section。返回四态枚举：

| 状态 | 含义 | Composer 行为 |
|---|---|---|
| `Skip` | 不适用 | 静默丢弃 |
| `Produced(body)` | 正常 | 入列 |
| `Degraded { body, warning }` | 部分降级 | 入列 + 记录 warning |
| `SoftFailed { code, error }` | 可恢复失败 | 跳过 + warning |
| `Result::Err(FatalError)` | 致命错误 | 整体 build 失败 |

### `SectionSpec` — Section 的完整自描述

```rust
pub struct SectionSpec {
    pub id: SectionId,
    pub title: Cow<'static, str>,
    pub layer: LayerResolver,       // Fixed(PromptLayer) | PerSurface(fn)
    pub order_hint: SectionOrder,   // First | Anchored(After/Before) | Default | Last
    pub surfaces: SurfaceMatcher,   // 哪些 Surface 需要它
    pub version: u32,               // 内容/结构变更时 bump
    pub max_chars: Option<usize>,
    pub criticality: SectionCriticality,  // Critical vs NonCritical
    pub source: Box<dyn SectionSource>,
}
```

### `BuildCx` — 构建上下文

```rust
pub struct BuildCx<'a> {
    pub pool: &'a SqlitePool,
    pub workspace_path: &'a str,
    pub thread_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub raw_plan: Option<&'a RuntimeModelPlan>,
    pub run_mode: RunMode,
    pub helper_profile: Option<&'a SubagentProfile>,
    pub custom_subagent_slug: Option<&'a str>,
    pub target_model: ModelTarget,
    pub clock: Arc<dyn Clock>,           // 时间抽象，禁止 Source 内直接 Utc::now()
    pub signals: Arc<SignalCache>,       // 同一次 build 内 memoize
    pub renderer: Arc<dyn SectionRenderer>,
    pub response_language: Option<&'a str>,
}
```

关键约定：

- Source **禁止**直接调用 `Utc::now()`、`SystemTime::now()`、`std::env`、`thread_rng`。时间走 `cx.clock`。
- 子代理派生用 `BuildCx::derive_for_helper()`，会创建新的 `SignalCache` 防止父 build 污染。
- 单 Section 渲染用 `Composer::render_section_only()`，内部用 `BuildCx::for_section_only()` 隔离。

### `SurfaceMatcher` — Section 适用哪些 Surface

```rust
pub enum SurfaceMatcher {
    All,
    Any(Vec<SurfacePattern>),
    Excluding(Vec<SurfacePattern>),
    Predicate(fn(&PromptSurface) -> bool),  // 罕见
}

pub enum SurfacePattern {
    AnyMainAgent, MainAgent(RunMode),
    AnySubagent, BuiltinSubagent, CustomSubagent,
    Compaction(CompactionKind), AnyCompaction,
    Title,
}
```

示例：`Role` → `Any([AnyMainAgent, AnySubagent])`；`BehavioralGuidelines` → `Any([AnyMainAgent])`。

### `SectionOrder` — 语义化排序

```rust
pub enum SectionOrder {
    First,
    Anchored(SectionAnchor),  // Before(SectionId) | After(SectionId)
    Default,
    Last,
}
```

替换裸 `u16`，新增 Section 不需要猜数字。

**锚点规则**：目标缺失→退化为 Default + warning；跨 Layer 锚点不允许（启动期 lint 拦截）；环形锚点不允许（lint 拦截）。

### `PromptBlock` + `CacheMarker` — 缓存契约

```rust
pub struct PromptBlock {
    pub layer: PromptLayer,
    pub text: String,
    pub cache_marker: Option<CacheMarker>,
}
```

Composer 按规则自动打标：

1. 跳过空 Layer 和 Ephemeral 层
2. 在稳定性最高的非空 Layer 末尾打 `Ephemeral` 标记（最多 2 个）
3. Layer 字符数 < 1024 时不打标

`CacheMarkerArbiter` 全局仲裁 system ↔ 消息层的标记配额（Anthropic ≤ 4 个 breakpoint）。

### `RuntimeMessageInjector` — 运行时变量外移

```rust
pub trait RuntimeMessageInjector: Send + Sync {
    fn applies_to(&self, surface: &PromptSurface) -> bool;
    async fn build_message(&self, cx: &BuildCx<'_>) -> Option<RuntimeMessage>;
}
```

`CurrentDateInjector` 在每个 turn 启动前注入日期到消息层（`PinOutsideWindow`，压缩不吞掉），system prompt 保持稳定。

### 模板系统 — 静态文案外置

`templates/*.md` 存储静态文案，每个文件带 YAML front-matter：

```yaml
---
section_id: BehavioralGuidelines
version: 7
declared_keys: []
---
You are TiyCode, an autonomous coding agent...
```

- **方括号占位符**：`{{key}}`，不引入 handlebars/tera
- **严格模式**：`render_template_strict` 缺键直接报错，不静默拼接残缺文本
- **用户文本不展开**：`vars.insert_user_text()` 防止用户输入中的 `{{...}}` 被二次展开
- **dev 热重载**：debug 模式下从磁盘读取模板，未命中回退到编译期常量
- **版本绑定**：模板 front-matter `version` 与 `SectionSpec::version` 启动期强制一致

### `PromptBudget` — 长度预算

```rust
pub struct PromptBudget {
    pub total_chars: usize,               // 全局上限（model context window × 0.30 × 4）
    pub per_section_default_chars: usize,  // 单 Section 默认上限
    pub per_section_overrides: BTreeMap<SectionId, usize>,
    pub eviction_order: Vec<PromptLayer>,  // 驱逐顺序：Ephemeral → ... → StablePrefix
}
```

- `for_model()` 根据目标模型 context window 自动计算
- 超限时按 `eviction_order` 从最不稳定的 Layer 开始驱逐
- `StablePrefix` 走截断而非删除（删除会破坏行为契约）

## 模块目录

```
src-tauri/src/core/prompt/
├── mod.rs                    # 模块导出
├── composer.rs               # Composer + ComposedPrompt + 渲染管线
├── registry.rs               # SectionRegistry + default_registry()
├── surface.rs                # PromptSurface, SurfacePattern, SurfaceMatcher
├── surface_extensions.rs     # SurfaceExtension trait + 启动期完整性 lint
├── layer.rs                  # PromptLayer, LayerResolver, SectionOrder, SectionAudit
├── section_id.rs             # SectionId 枚举
├── section_source.rs         # SectionSource trait, SectionOutcome, SectionSpec
├── build_context.rs          # BuildCx + ModelTarget
├── signals.rs                # SignalCache + BuildSignal + 循环检测
├── templates.rs              # 模板加载/渲染/热重载 + front-matter 解析 + TemplateSource
├── budget.rs                 # PromptBudget + for_model()
├── cache_marker.rs           # PromptBlock, CacheMarker, CacheMarkerArbiter
├── runtime_message.rs        # RuntimeMessageInjector, CurrentDateInjector
├── exec_policy.rs            # SourceExecPolicy (超时/并发/背压)
├── error_codes.rs            # SoftFailed.code 常量集中注册
├── redactor.rs               # PII 脱敏
├── renderer.rs               # SectionRenderer (Markdown | XML)
├── inheritance.rs            # SUBAGENT_INHERITED_SECTIONS + lint
├── clock.rs                  # Clock trait + SystemClock + FixedClock
├── run_mode.rs               # RunMode 枚举
├── snapshot_tests.rs         # 快照测试
├── sources/                  # 17 个 SectionSource 实现（一个 Section 一个文件）
│   ├── mod.rs
│   ├── role.rs, behavioral_guidelines.rs, final_response_structure.rs
│   ├── shell_tooling_guide.rs, skills.rs, project_context.rs
│   ├── profile_instructions.rs, run_mode.rs, system_environment.rs
│   ├── sandbox_permissions.rs, workspace_location.rs
│   ├── active_goal.rs, active_plan.rs
│   ├── subagent_output_contract.rs, custom_subagent_body.rs
│   ├── compaction_contract.rs, title_contract.rs
│   └── source_tests.rs
└── templates/                # 静态 Markdown 模板
    ├── role.md, behavioral_guidelines.md, final_response_structure.md
    ├── shell_tooling_guide.md, skills_usage.md, project_context.tpl.md
    ├── run_mode.default.md, run_mode.plan.md
    ├── sandbox_permissions.tpl.md, system_environment.tpl.md
    ├── workspace_location.tpl.md, active_goal.tpl.md, active_plan.tpl.md
    ├── subagent/ (explore.md, review.md, output_contract.*.md)
    ├── compaction/ (compact.md, merge.md)
    ├── handoff/
    └── title/ (contract.md)
```

## 典型用法

### 主代理 System Prompt

```rust
let composer: Arc<Composer> = composer_singleton();  // 进程启动时注入 registry
let budget = PromptBudget::for_model(&model_target, &surface);
let cx = BuildCx { pool, workspace_path, thread_id, run_id, raw_plan, run_mode, ... };

let composed = composer
    .build(&PromptSurface::MainAgent { run_mode: RunMode::Default }, &cx, &budget)
    .await?;

// 传递给 LLM provider 适配层：
//   Anthropic: composed.blocks → system: [{type:"text", text, cache_control?}, …]
//   其他: composed.text 整段下发
agent.set_system_prompt_blocks(composed.blocks);
```

### 子代理

```rust
let composed = composer
    .build(
        &PromptSurface::SubagentExplore { inherited_run_mode: parent_cx.run_mode },
        &parent_cx.derive_for_helper(&helper_profile, None),
        &PromptBudget::for_model(&parent_cx.target_model, &subagent_surface),
    )
    .await?;
```

### 上下文压缩 & 标题生成

```rust
composer.build(&PromptSurface::Compaction { kind: CompactionKind::Compact }, &cx, &budget).await?;
composer.build(&PromptSurface::Compaction { kind: CompactionKind::Merge }, &cx, &budget).await?;
composer.build(&PromptSurface::Title, &cx, &budget).await?;
```

### 单 Section 借用（user message 拼装用）

```rust
if let Some(body) = composer.render_section_only(&SectionId::ProfileInstructions, &surface, &cx).await {
    user_message.push_str(&body.markdown);
}
```

不触发 budget、不打 cache marker、不污染主路径 `SignalCache`。

## 扩展指南

### 新增一个 Section

只需做三件事：

1. 新建 `sources/active_task_board.rs`，实现 `SectionSource`：

```rust
pub struct ActiveTaskBoardSource;

#[async_trait]
impl SectionSource for ActiveTaskBoardSource {
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let Some(thread_id) = cx.thread_id else { return Ok(SectionOutcome::Skip) };
        let board = match task_board::load(cx.pool, thread_id).await {
            Ok(Some(b)) => b,
            Ok(None) => return Ok(SectionOutcome::Skip),
            Err(e) => return Ok(SectionOutcome::SoftFailed {
                code: error_codes::TASK_BOARD_LOAD_FAILED,
                error: e.into(),
            }),
        };
        Ok(SectionOutcome::Produced(SectionBody::markdown(format!("Active Task Board: {}", board.title))))
    }
}
```

2. 在 `section_id.rs` 新增 `SectionId::ActiveTaskBoard` 变体（如果是新 SectionId）。

3. 在 `registry.rs::default_registry()` 追加一行 `registry.register(...)`：

```rust
registry.register(SectionSpec {
    id: SectionId::ActiveTaskBoard,
    title: Cow::Borrowed("Active Task Board"),
    layer: LayerResolver::Fixed(PromptLayer::Ephemeral),
    order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::ActivePlan)),
    surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
    version: 1,
    max_chars: None,
    criticality: SectionCriticality::NonCritical,
    source: Box::new(ActiveTaskBoardSource),
});
```

**不需要改 Composer，不需要改其他 Section，不需要分配魔法数字。**

### 新增一个 Surface

1. 在 `surface.rs` 的 `PromptSurface` 枚举新增变体。
2. 在 `SurfacePattern` 新增对应匹配模式。
3. 在 `surface_extensions.rs` 实现 `SurfaceExtension` trait。
4. 在 `PromptBudget::for_model()` 和 `inheritance.rs` 补充对应分支。
5. 启动期 lint 自动校验完整性（`surface_extensions_complete`、`subagent_inheritance_complete`）。

### 新增一个模板

1. 在 `templates/` 下创建 `.md`，写入 front-matter + 正文。
2. 在对应 Source 中通过 `TemplateSource` 或直接 `include_str!` + `load_template` 加载。
3. `cargo test prompt::templates::lints` 自动校验 `{{key}}` ↔ `declared_keys` 双向一致。

## 设计规则与约束

### Section 必须遵守

- **只读**：不得通过 `cx.pool` 执行写操作；不得写文件、发网络请求。
- **幂等**：同一 `BuildCx` 多次调用返回语义等价结果。
- **可重放**：只能依赖 `BuildCx` 显式字段 + `SignalCache` + 静态模板。禁止 `std::env`、`SystemTime::now()`、`thread_rng`。
- **失败可解释**：`SoftFailed.code` 必须在 `error_codes::codes` 中注册。
- **无外部副作用**：日志不超过 `tracing::trace!`，warning 走 `SectionOutcome` 而非 `tracing::warn!`。

### Section 间禁止依赖

Section 之间不允许互相读取对方输出。需要共享状态时通过 `BuildSignal` 表达：

```rust
// ❌ 错误：ActivePlanSource 查询 ActiveGoalSource 的输出
// ✅ 正确：两者都消费 BuildSignal::ActiveGoal，各自独立判定
```

这条约束让 Composer 无需拓扑排序、循环检测、重算传播。

### Schema Version 变更规则

| 变更类型 | bump `SectionSpec.version` | bump `registry.schema_version` |
|---|---|---|
| 模板正文文案修改 | ✅ | ❌ |
| 模板新增/移除占位符 | ✅ | ❌ |
| Section 切换 LayerResolver | ✅ | ✅ |
| Section 新增 / 删除 | 新 Section 从 1 | ✅ |
| SurfaceMatcher 调整 | ✅ | ✅ |
| SectionOrder 调整 | ✅ | ❌ |
| PromptSurface 新增 / 删除 | — | ✅ |
| PromptLayer 枚举调整 | — | ✅ |
| PromptBudget 数值调整（仅数值） | — | ❌ |

### StablePrefix 纯净性

`StablePrefix` 内禁止出现瞬态字面量：ISO 日期 (`\d{4}-\d{2}-\d{2}`)、timestamp、thread_id、run_id、用户名、`$HOME` 路径片段。`cargo test prompt::composer::tests::cache_purity_*` CI 强制。

### 子代理继承清单

`inheritance.rs` 中的 `SUBAGENT_INHERITED_SECTIONS` 是子代理继承哪些 Section 的真相源。修改时需同步更新 registry 中的 `SurfaceMatcher`，启动期 lint (`subagent_inheritance_complete`) 强制一致性。

## 测试覆盖

| 层 | 覆盖目标 | 运行命令 |
|---|---|---|
| 单元（Composer） | Layer 排序、SurfaceMatcher、Budget 截断/驱逐、超时、CacheMarker 配额 | `cargo test --lib prompt::composer` |
| 单元（Sources） | 每个 Source 的 Skip/Produced/Degraded/SoftFailed 四态 | `cargo test --lib prompt::sources` |
| 模板 lint | `{{key}}` ↔ `declared_keys` 双向一致；version 同步 | `cargo test --lib prompt::templates::tests::templates_have_no_undeclared_keys` |
| Schema 守护 | schema_version 单调性；Section version ≥ 1 | `cargo test --lib prompt::registry::tests::schema_version_monotonic` |
| Surface 完整性 | 每个 PromptSurface 都有 Section；子代理继承清单正确 | `cargo test --lib prompt::registry::tests::all_surfaces_have_sections` |
| 缓存纯净性 | StablePrefix 无日期/ID/用户名 | `cargo test --lib prompt::composer::tests::cache_purity_stable_prefix_omits_dates_and_ids` |
| Cache marker | 配额 ≤ 4；短 Layer 不打标 | `cargo test --lib prompt::composer::tests::cache_marker_*` |
| 幂等/可重放 | 同 cx 多次调用等价；FixedClock 下输出稳定 | `cargo test --lib prompt::composer::tests::source_*` |
| 锚点 | 目标存在、无环、同 Layer | `cargo test --lib prompt::layer::tests::anchors_*` |
| 错误码 | 所有 code 在 `ALL_ERROR_CODES` 注册 | `cargo test --lib prompt::error_codes` |
| 快照 | 每个 Surface × 关键 fixture 完整渲染 | `cargo test --lib prompt::snapshot_tests` |

