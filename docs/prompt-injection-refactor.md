# Prompt 注入逻辑重构方案

> 目标：在保留现有功能的前提下，重构 Prompt 注入链路，使其**更稳健（可降级、可观测、可测试）**、**更可扩展（新增章节/子代理/Surface 不需要改装配器）**、**更易维护（静态文案外置、配置即数据、单一职责）**。
>
> 范围：`src-tauri/src/core/prompt/**`、`agent_session.rs::build_system_prompt + inject_goal_context`、`subagent/orchestrator.rs::build_helper_system_prompt`、`agent_run_summary.rs` / `agent_run_title.rs` 内的 system prompt 构造。

---

## 零、设计支柱与边界

### 0.1 设计支柱

- **Layer × Surface 双轴分离 + `SurfaceMatcher`**：Section 是可独立演进的最小单元；新增 Surface 不需要修改装配器
- **类型化数据流取代字符串解析**：消除 `inject_goal_context` 字符串拼接与 `build_helper_system_prompt` 按 `## ` 反解析两个反模式
- **`SectionOutcome` 四态 + Layer 驱逐 + 模板严格模式**：在设计层收敛"软失败 / 长度失控 / 文案污染"三类事故
- **`PromptBlock` + `CacheMarker`**：把 prompt-prefix cache 作为一等公民对待，与 Anthropic / Bedrock API 契约对齐
- **禁止 inter-section 依赖**：Section 之间只通过 `BuildSignal` 共享数据，Composer 调度退化为扁平并发 + Layer 排序
- **运行时数据外移到消息层**：`current_date` 等瞬态变量通过 `RuntimeMessageInjector` 注入到 user/system 消息，system prompt 永久稳定

### 0.2 设计边界（不在本设计范围）

- LLM provider 适配层（Anthropic / Bedrock / OpenAI 的具体下发）：本设计只产出 `PromptBlock[]` 契约
- 工具调用提示（tool descriptions）注入链路
- RAG 文档块的 cache marker 配额管理：本设计预留 2 个 marker，剩余 2 个由消息层规约
- skills 注册中心本身的存储/分发：本设计只消费

### 0.3 关键约定一览

| 约定 | 章节 |
|---|---|
| Section 间禁止依赖，仅通过 `BuildSignal` 共享 | § 3.2.6 |
| `SignalCache` 双层结构（短临界 `Mutex` + 跨 await `OnceCell`） | § 3.6 |
| `RuntimeMessage` 注入位置 + 与压缩链交互协议 | § 3.7 |
| `BuildCx::derive_for_helper` 派生规则 | § 3.8.1 |
| `schema_version` 仅用于事故复盘可读性，不承诺自动回放 | § 3.11 |
| `estimated_tokens` 通过 `Tokenizer` trait 产出，默认 chars/4 启发式 | § 3.11 |
| Section 渲染抽象 `SectionRenderer`（Markdown / XML 等） | § 3.14 |
| `SectionOrder::Anchored` 解析规则 + 启动期 lint | § 3.4 |
| 模板用户文本不二次展开占位符 | § 3.9 |
| 子代理 surface 携带 `inherited_run_mode` | § 3.2.1 |
| Compaction 输入预过滤 RuntimeMessage | § 3.7 |
| Section 标题 v1 不做运行时 i18n | § 3.2.5 |
| 子代理切换的允许差异白名单 | § 4 阶段 2a |
| Source 执行模型：超时 / 并发上限 / 背压 / 重入 | § 3.6.1 |
| Cache marker 全局仲裁（≤ 4 个，跨 system + 消息层） | § 3.7.1 |
| Surface 扩展点：闭包枚举 + 单点新增 | § 3.16 |
| Source 副作用约束：只读、幂等、可重放 | § 3.18 |
| `schema_version` vs Section `version` 的 bump 规则 | § 3.19 |
| 模板 front-matter `version` 与 Section `version` 绑定 | § 3.20 |
| 散落入口归并：含 `build_implementation_handoff_prompt` | § 3.21 |
| 子代理继承的 Section 默认清单 | § 3.22 |
| `SignalCache` 循环检测与失败重试（不永久 poison） | § 3.6 |
| Layer 被预算掏空时 `CacheMarker` 滑动规则 | § 3.7.1 |
| `PromptBudget::for_model` 按 model context window 计算 | § 3.12 |
| `CustomSubagent` 的 `cache_stability` 进入 `PromptSurface`（非 profile） | § 3.2.1 |
| `BuildCx` 完整字段（含 `custom_subagent_slug` / `target_model` / `clock`） | § 3.6 |
| `SectionRenderer` 灰度切换路径（与 schema_version 协同） | § 3.14 |
| `Composer::render_section_only` 隔离 BuildCx | § 3.21 |
| `Composer` 入口签名：registry 在构造时注入，`build` 不传 | § 3.3 / § 6 |

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
    SubagentExplore { inherited_run_mode: RunMode },
    /// 内置 review helper
    SubagentReview { inherited_run_mode: RunMode },
    /// 用户自定义子代理
    SubagentCustom {
        slug: String,
        inherited_run_mode: RunMode,
        /// 用户在 profile YAML 中显式声明该 prompt 不含瞬态内容（日期 / 冲刺名 / PR ID 等）
        /// 时设为 true，Composer 会把 `CustomSubagentBody` 提升至 StablePrefix Layer。
        /// 默认 false（SessionStable）。
        ///
        /// 字段进入 PromptSurface（而非 profile 单独传入），是为了让 LayerResolver
        /// 仅依赖 surface 即可决策，避免通过 BuildCx 注入"会改变 Layer 的隐藏参数"，
        /// 进而保持 surface 的 Hash/Eq 与缓存语义自洽。
        cache_stability: SubagentCacheStability,
    },
    /// 上下文压缩
    Compaction { kind: CompactionKind }, // Compact | Merge
    /// 会话标题生成
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubagentCacheStability {
    /// 默认；用户自定义 prompt 视为可能含瞬态内容
    Volatile,
    /// 用户主动承诺 prompt 内容跨会话稳定
    Stable,
}
```

> **`inherited_run_mode` 语义**：子代理 surface 携带父代理 `run_mode`。`Plan` 模式下父代理派生子代理时，子代理 prompt 中所有"修改文件 / 执行命令"类指令必须自动屏蔽（通过 `RunMode::Plan` 在 `BehavioralGuidelines` 子代理变体上启用约束分支表达，而非在 Source 内做 ad-hoc 字符串拼接）。`SubagentCustom` 默认 `inherited_run_mode = Default`，profile YAML 可声明 `inherit_run_mode: true` 改为继承父态。

每个 Section Source 自己声明匹配规则（见 § 3.2.7 `SurfaceMatcher`），由 Composer 在装配时筛选——**Surface 不再是 Provider 列表的隐式产物，而是一等公民**。

**Surface 等价类**：`Hash`/`Eq` 用于 `SurfaceMatcher::Any` 的快速匹配；`SurfacePattern::AnySubagent` 等"通配模式"在 § 3.2.7 的 `matches()` 中**忽略 `inherited_run_mode` / `cache_stability`**，仅匹配 surface kind。同 slug 的 `SubagentCustom` 在 `cache_stability` 切换时**视为不同 surface**——因为缓存语义改变，schema_version 必须 bump（见 § 3.19）。

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

> **不变量**：`current_date` 等瞬态变量不进入 system prompt。它们通过 **runtime context message**（每个 turn 的 user/system 消息体）注入。详见 § 3.7。

#### 3.2.3 输出契约：`ComposedPrompt` / `PromptBlock` / `CacheMarker`

为了与 Anthropic / Bedrock 等支持 prefix cache 的 LLM provider 对齐（cache 通过 content block 上的 `cache_control: { type: "ephemeral" }` 标记，**单请求最多 4 个 breakpoints**），Composer 输出 provider-agnostic 的内容块结构而非裸字节偏移：

```rust
pub struct ComposedPrompt {
    /// system prompt 完整文本（不支持 cache 的 provider 可直接使用此值）
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
    /// 便于线上事故复盘（不承诺按版本回放，详见 § 3.11）
    pub version: u32,
    /// 单 Section 长度上限（字符）；None 时使用 PromptBudget.per_section_default_chars
    pub max_chars: Option<usize>,
    pub source: Box<dyn SectionSource>,
}
```

> **i18n 范围**：`title: Cow<'static, str>` 仅是为了同时支持 `&'static str` 字面量和静态拼接结果，**v1 不做运行时多语言**。响应语言由 `ProfileInstructionsSource` 在正文内表达（"respond in zh-CN" 之类指令），而非通过翻译 Section 标题。i18n 扩展点为 `pub title: TitleResolver`，在不破坏现有 API 的前提下后续启用。

```rust
pub enum LayerResolver {
    Fixed(PromptLayer),
    PerSurface(fn(&PromptSurface) -> PromptLayer),
}

pub struct SectionBody {
    /// 已渲染好的 Markdown 正文（不含 H2 标题；Renderer 决定如何包装）
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
    /// 部分降级仍输出（如 Skills 列表读取部分失败）
    Degraded { body: SectionBody, warning: SectionWarning },
    /// 跳过 + warning（如 ProjectContext 读 AGENTS.md IO 失败）
    SoftFailed { code: &'static str, error: AppError },
}
```

**单一职责**：一个 Source 只产出**一个** Section。原 `BaseProvider` 产出 3 个 Section 的设计被拆成 `RoleSource`、`BehavioralGuidelinesSource`、`FinalResponseStructureSource`。

> **禁止 inter-section 依赖**：
>
> Source 之间**不允许**互相读对方的 `SectionBody` / `SectionOutcome`。"我的 Section 仅在另一个 Section 存在时启用" 这类需求一律通过共享 `BuildSignal` 表达（信号是无副作用的、可被多个 Source 同时消费的纯查询）。
>
> 例：`ActivePlanSource` 想"仅在 Goal 存在时启用"——不是去 query `ActiveGoalSource` 的输出，而是两者都消费 `BuildSignal::ActiveGoal`，由各自的 `enabled_for` / `build` 独立判定。
>
> 这条约束让 Composer 调度退化为"扁平并发 + Layer 排序"，无需做拓扑排序、循环检测、重算传播。任何看似需要 inter-section 依赖的需求，**先抽 signal**。
>
> 仅有的合法跨 Section 关系是**排序锚点**（§ 3.4 `SectionAnchor`）——锚点只影响顺序，不影响语义存在与否。

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

`Composer` 在进程启动时由 `default_registry()` 注入构造，运行时不可变；`registry` 不出现在 `build()` 签名中——避免调用方误用不一致的 registry，也保证 schema_version 单一。

```rust
pub struct Composer {
    registry: Arc<SectionRegistry>,
    exec_policy: SourceExecPolicy,
    default_renderer: Arc<dyn SectionRenderer>,
}

impl Composer {
    pub fn new(registry: Arc<SectionRegistry>, exec_policy: SourceExecPolicy) -> Self { … }

    pub async fn build(
        &self,
        surface: PromptSurface,
        cx: BuildCx<'_>,
        budget: &PromptBudget,
    ) -> Result<ComposedPrompt, AppError> {
        // 1. 拣选
        let candidates: Vec<&SectionSpec> = self.registry
            .iter()
            .filter(|spec| spec.surfaces.matches(&surface))
            .collect();

        // 2. 并发构建（同 Layer 内并发，跨 Layer 顺序保留 deterministic ordering）
        //    SectionOutcome::Skip / SoftFailed → 不进入下一步；Degraded / Produced → 进入
        let mut bodies: Vec<RenderedSection> =
            join_all_collecting_outcomes(candidates, &cx, &self.exec_policy).await;

        // 3. 解析每个 Section 的 Layer（PerSurface 在此处求值）
        bodies.iter_mut().for_each(|s| s.layer = s.spec.layer.resolve(&surface));

        // 4. per-section 长度检查 → 超限即截断 + warning
        enforce_per_section_budget(&mut bodies, budget);

        // 5. 排序：(Layer, SectionOrder, SectionId 字典序作为 tie-breaker，保证可重现)
        bodies.sort_by_key(|s| (s.layer, s.spec.order_hint, s.spec.id.clone()));

        // 6. 全局长度检查 → 按 budget.eviction_order 驱逐 / 截断关键 Section
        enforce_total_budget(&mut bodies, budget);

        // 7. 渲染为 PromptBlock[] + 在剩余 Layer 末尾打 cache marker（滑动规则见 § 3.7.1）
        render_blocks(bodies, surface, self.registry.schema_version())
    }

    /// 单 Section 渲染——给 `build_implementation_handoff_prompt` 等"借用 Section 文本拼 user message" 的路径使用。
    /// 不打 cache marker、不进入 audit、不参与 budget、**不触发 RuntimeMessageInjector**。
    /// 内部使用裁剪过的 `BuildCx`：丢弃 `signals` 改用一次性 `SignalCache::standalone()`，
    /// 防止污染调用方主路径的 SignalCache 与并发计数。
    pub async fn render_section_only(
        &self,
        id: SectionId,
        surface: &PromptSurface,
        cx: &BuildCx<'_>,
    ) -> Option<SectionBody> { … }
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

pub enum SectionAnchor {
    Before(SectionId),
    After(SectionId),
}
```

`SectionAnchor::After(SectionId::Role)` 比裸 `order_in_phase = 20` 更具语义；新增 Section 不需要"猜数字"。

**锚点解析规则**：

1. 锚点解析在 § 3.3 步骤 5 之前完成（同 Layer 内）：
   - 先把 `First / Default / Last` 三段稳定段落用 `SectionId` 字典序排好
   - 再把 `Anchored(Before|After(target))` 的 Section 插入到 `target` 的相邻位置；多个 Section 锚到同一目标时，按 `SectionId` 字典序确定相对次序
2. **锚点目标缺失**（target 在当前 Surface 被过滤掉 / 不在 registry / 自身 SoftFailed 被丢弃）→ 退化为 `SectionOrder::Default`，发 `SectionWarning::AnchorMissing`，不报错
3. **跨 Layer 锚点不允许**：若 `target` 与 anchor 不在同一 Layer，启动期 `cargo test prompt::registry::lints` 失败
4. **环形锚点不允许**：A.After(B) 且 B.After(A) → 启动期 lint 失败
5. 启动期 lint 测试覆盖：所有 `Anchored` 的 target 必须在 registry 中存在；同 Layer；非自指；非环

```rust
#[cfg(test)]
mod registry_lints {
    #[test]
    fn anchors_are_well_formed() { … }
    #[test]
    fn anchors_do_not_form_cycles() { … }
    #[test]
    fn anchors_target_same_layer() { … }
}
```

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
    /// 子代理 surface 时，CustomSubagentBody Source 通过它查到要渲染哪条 prompt；
    /// MainAgent / Compaction / Title Surface 下为 None
    pub custom_subagent_slug: Option<&'a str>,
    /// 目标 LLM 标识；用于 `PromptBudget::for_model` 求值（context window）
    /// 与 `SectionRenderer` 的 model-aware 选择
    pub target_model: ModelTarget,
    /// 时间相关数据 Source 必须从此读，禁止 `Utc::now()` / `SystemTime::now()`
    /// （§ 3.18 副作用约束）；CurrentDateInjector 也走同一 Clock
    pub clock: Arc<dyn Clock>,
    /// 信号缓存：Source 通过 cx.signal::<T>(key) 查询并自动 memoize；
    /// 同一 (TypeId, key) 并发请求共享一个 OnceCell，避免重复 DB 查询
    pub signals: Arc<SignalCache>,
    /// 软配置：feature flag、A/B 实验、按模型 capability 切换；
    /// 通过 BuildCx 注入而非修改 registry，hot-path 无锁
    /// 渲染器（§ 3.14）：由调用方根据目标 LLM provider 选择
    pub renderer: Arc<dyn SectionRenderer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModelTarget {
    AnthropicClaude { context_window: usize, supports_cache_control: bool },
    OpenAiCompat { context_window: usize },
    Local { context_window: usize },
}

pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
}
```

**SignalCache 锁与键设计**：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SignalKey {
    /// 默认 "global"；按 workspace / thread 区分时改为 hash(workspace_path) / thread_id
    pub scope: Cow<'static, str>,
}

pub struct SignalCache {
    /// (TypeId, SignalKey) → Slot；用 tokio::sync::OnceCell 跨 await 不持锁；
    /// 索引表本身用短临界区的 std::sync::Mutex 保护（不跨 await）
    inner: Mutex<HashMap<(TypeId, SignalKey), Arc<SignalSlot>>>,
}

struct SignalSlot {
    cell: OnceCell<SignalResult>,
    /// 当前 init 是否在执行中——用于循环依赖检测
    in_flight: AtomicBool,
}

#[derive(Clone)]
enum SignalResult {
    Ready(Arc<dyn Any + Send + Sync>),
    /// init 失败：缓存"失败标记"而非 panic OnceCell；下次同 cx 内的查询直接返回 Err，
    /// **不重试**（保证幂等），但允许在新 BuildCx 中重新尝试。
    /// 这避免了 OnceCell 一旦 set 永远 poison 的问题——init 抛错时 OnceCell 仍未 set，
    /// 我们手动写入 Failed 标记代替之。
    Failed(SignalFailure),
}
```

要点：

- **锁粒度收敛到表索引**：跨 `await` 不持有 `Mutex`，杜绝异步死锁
- **复合键**：`(TypeId, SignalKey)` 让同一信号可以按 workspace / thread 分别缓存（例：`SkillsSignal` 在 workspace A 与 B 不共享）
- **生命周期**：`SignalCache` 同 `BuildCx`，**一次 build 内** memoize；不跨 build 共享，避免脏读 / TTL 设计
- **类型安全**：`downcast` 失败说明同一 `TypeId` 被两处用作不同类型，是 bug，应 panic（启动期单测覆盖）
- **失败缓存**：init 失败时写入 `Failed(SignalFailure)` 而非让 `OnceCell` 永久 poison。同一 cx 内不重试，但下一次 build（新 cache）可重新尝试——避免一次瞬时 IO 抖动让整次 build 永远不可恢复
- **循环依赖检测**：`SignalSlot::in_flight = true` 进入 init；若同一 cx 内同一 (TypeId, SignalKey) 在 in_flight 时再次被请求 → 返回 `Failed(SignalFailure::Cycle { chain })`，由消费方决定走 SoftFailed 还是 FatalError；`cargo test prompt::signal_cycle_detected` 覆盖


#### 3.6.1 Source 执行模型（超时 / 并发 / 背压 / 重入）

`SectionSource::build` 是 async + 可能触达 SQLite / 文件系统的代码。如果不约束执行模型，单次 build 可能因为某个 Source 阻塞而拖慢整条 LLM 调用链路。

```rust
pub struct SourceExecPolicy {
    /// 单 Source 软超时；超时则返回 SectionOutcome::SoftFailed { code: "source.timeout" }
    /// 默认 250 ms
    pub per_source_timeout: Duration,
    /// 单次 build 内同 Layer 并发上限；防止一次 build fan-out 数十个 SQLite 查询
    pub layer_concurrency: usize,           // 默认 8
    /// 整次 build 硬上限；超时则整体 build 失败
    pub overall_build_timeout: Duration,    // 默认 800 ms
    /// 同一 Source 在 SignalCache miss 时是否允许并发执行；
    /// 默认 false（OnceCell 自然串行），罕见场景可放开
    pub allow_concurrent_signal_init: bool,
}
```

**Composer 调度规则**：

1. 同 Layer 内 Source 通过 `tokio::task::JoinSet` + `Semaphore(layer_concurrency)` 调度；不同 Layer 之间天然串行（Layer 之间的语义顺序在 § 3.3 第 5 步已经依赖前置结果）
2. 每个 Source 由 `tokio::time::timeout(per_source_timeout, source.build(cx))` 包裹；超时记 `prompt.source.timeout{id=...}` metric + `SectionOutcome::SoftFailed`，不阻塞兄弟 Source
3. `overall_build_timeout` 用 `tokio::select!` 与整体 build future 竞速：超时后未完成的 Source 一律记 `SoftFailed`
4. **重入安全**：Composer 不持有可变状态；同一 `Composer` 实例可被多个 thread 同时 build；`SignalCache` 与 `BuildCx` 一一对应，跨 build 不复用，从根上消除竞争
5. **背压**：`Composer::build` 不直接生成新 task，全部走 `JoinSet`；调用方层面通过外部 `Semaphore` 控制并发 build 数（如压缩链路高峰期可能并发 100+），避免 SQLite 连接池被打满

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
    /// 注入位置——决定该消息落在消息序列的哪里
    pub placement: RuntimeMessagePlacement,
    /// 当 PinOutsideWindow 时使用；同 id 的消息每轮替换而非追加
    pub dedup_id: Option<&'static str>,
}

pub enum RuntimeMessagePlacement {
    /// 紧邻 system prompt 之后、最早的 user/assistant 消息之前
    /// 适用于"会话级运行时上下文"（极少见）
    AfterSystem,
    /// 当前 turn 的最后一条 user 消息之前——**默认**
    /// 这样不参与 prompt-prefix cache（cache marker 已经在 system prompt 末尾打上）
    BeforeLatestUser,
}

pub enum CompactionPolicy {
    /// 默认：可被压缩链吞掉，下次 turn 重新注入
    AbsorbAndReinject,
    /// 排除在压缩窗口外（如当前日期、当前 PR 状态）；
    /// 防止 summary-of-summary 把它卷入摘要后下次又重新注入造成"双份"
    PinOutsideWindow,
}
```

**注入点协议 + 压缩协议**：

1. **位置选择**：默认 `BeforeLatestUser`。这样运行时消息位于 cache marker **之后**，不参与 prefix cache 计算——日期变化不影响 cache 命中
2. **dedup**：`dedup_id` 让"每个 turn 替换一次"语义显式化：消息序列化层在注入前，先按 `dedup_id` 移除上一轮注入的同 id 消息
3. **PinOutsideWindow 的实现**：消息携带 `meta.compaction_pinned = true` 持久化到 messages 表；`build_compact_summary_*` 的输入预过滤层（不是 prompt 层）排除 pinned 消息——**这是消息序列化层职责，不是 Composer 职责**
4. **避免双份注入**：进入 Compaction Surface 的输入消息列表，必须**已剔除** `compaction_pinned = true` 的 RuntimeMessage；同时 Composer 在 `Compaction` Surface 下不再触发 `RuntimeMessageInjector`（即压缩输出本身不带运行时消息），由调用方在压缩结果重新进入主循环时由 `CurrentDateInjector` 重新注入
5. **顺序契约**：多个 Injector 同 placement 时，按 `applies_to` 注册顺序 + injector 名字字典序排序，结果可重现

例：`CurrentDateInjector` 在每个 turn 启动前，用 `dedup_id = "current_date"` 注入：

```
<runtime_context turn_started_at="2026-06-05T03:21:11Z">
Current date: 2026-06-05
</runtime_context>
```

`CurrentDateInjector.applies_to` 默认覆盖**所有需要时间感知的 surface**（MainAgent + Subagent*），review 子代理审 PR 时间敏感场景同样需要。

这样 system prompt 完全稳定，prompt-prefix cache 命中率最大化。

#### 3.7.1 Cache marker 全局仲裁

Anthropic 单请求的 `cache_control` breakpoint **全局上限是 4**，跨 system prompt + tools + messages 共享。Composer 默认占用 2 个（`StablePrefix` 末尾、`SessionStable` 末尾），消息层若再无规约地打 marker，极易超限报错或破坏稳定 prefix 的命中。

引入显式仲裁器：

```rust
pub trait CacheMarkerArbiter: Send + Sync {
    /// Composer 渲染完后调用：报告 system prompt 已占用的 marker 数与位置
    fn record_system_markers(&self, markers: &[CacheMarkerSlot]);
    /// 消息层在序列化前调用：申请剩余配额；返回实际可用数量
    fn allocate_for_messages(&self, requested: usize) -> usize;
    /// 一次 LLM 调用结束后必须 reset，避免跨请求泄露
    fn reset(&self);
}

pub struct CacheMarkerSlot {
    pub layer: PromptLayer,
    pub byte_offset_in_text: usize,
    pub block_index: usize,
}
```

**约定**：

1. 一次 LLM 请求生命周期内 `CacheMarkerArbiter` 单例（请求级），由调用方在请求开始时构造、结束时 `reset`
2. **配额**：默认 system 占 2 / 消息层 2；当 system 因 budget 截断只产出 1 个 Block 时，消息层可申请到 3
3. **超额**：消息层 `allocate_for_messages(requested)` 若 `requested > remaining` → 返回 `remaining`，记 `prompt.cache_marker.over_request` metric；消息层必须按返回值裁剪，绝不允许"先发后协商"
4. **审计**：每个 marker 在 `ComposedPrompt.audit` 与消息层日志中均带 `block_index + byte_offset`，事故复盘时可还原 4 个 breakpoint 的真实位置
5. **回归测试**：`cargo test prompt::cache_marker_quota` 制造极端场景（StablePrefix 截断为空、消息层申请 5 个）→ 验证总数 ≤ 4 且优先满足 system 端

**Layer 被掏空时的滑动规则**：

预算驱逐 / 截断后，可能出现"`StablePrefix` 整层为空"或"`SessionStable` 内仅剩 1 个 Section"等情况，原"在 Layer 末尾打 marker" 的天真规则会失效（marker 落在不存在的 block 上 / 落在过短的稳定段上反而降低命中率）。Composer 在渲染阶段按以下次序选择 marker 位置：

| 步骤 | 规则 |
|------|------|
| 1 | 计算每个 Layer 渲染后的 block 字符长度；丢弃长度 = 0 的 Layer |
| 2 | 若剩余非空 Layer 数 ≥ 2 → 在前两个稳定性最高的 Layer（StablePrefix > SessionStable > RuntimeOverlay）末尾各打一个 `Ephemeral` marker |
| 3 | 若仅剩 1 个非空 Layer 且其字符数 ≥ `min_marker_chars`（默认 1 KB）→ 仅打 1 个 marker；记 `prompt.cache_marker.degraded_to_one` metric |
| 4 | 若唯一 Layer 字符数 < `min_marker_chars` → 不打 marker；记 `prompt.cache_marker.skipped` metric（强制不打的目的是避免缓存"碎片化命中"反而拖累整体延迟） |
| 5 | `Ephemeral` Layer **永远不打** marker（按定义就不稳定，缓存会污染下一轮） |
| 6 | `audit.cache_markers` 字段记录最终落点 + 触发滑动的原因（如 `"reason": "stable_prefix_emptied"`） |

`min_marker_chars` 由 `ModelTarget` 决定（Anthropic ≥ 1024 字符 cache 才有显著收益；本地小模型默认 0 即可），通过 `BuildCx::target_model` 求值。

### 3.8 子代理构建

```rust
let composed = composer.build(
    PromptSurface::SubagentExplore { inherited_run_mode: parent_cx.run_mode },
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

> **迁移分两步**：因 LLM 对 system prompt 微小变化敏感，子代理切换分 2a / 2b 两步，详见 § 4 阶段 2。

#### 3.8.1 `BuildCx::derive_for_helper` 派生规则

| 字段 | 派生策略 |
|------|---------|
| `pool` | 直接复用父 cx |
| `workspace_path` | 直接复用 |
| `thread_id` | 复用父 thread_id（helper 与父属于同一 thread） |
| `run_id` | **新建** helper 自己的 run_id（用于审计独立追踪） |
| `raw_plan` | 复用父值；helper 不修改 plan |
| `run_mode` | 由 surface 携带的 `inherited_run_mode` 决定（见 § 3.2.1） |
| `helper_profile` | `Some(&helper_profile)`；主代理路径下为 `None` |
| `signals` | **新建空 `SignalCache`**——隔离父子 build 的缓存，防止父 build 的脏数据泄露到 helper；workspace / project 类查询会被 helper 重新执行（同一 workspace 路径，结果应一致） |
| `renderer` | 由 helper 调用方根据目标模型重新选择（helper 可能用不同 model 与不同 renderer） |

> **隔离 vs 复用的取舍**：`signals` 不复用是为了切断"父侧失败的 SoftFailed 信号污染 helper" 的路径，代价是 helper 可能重复一次 DB 查询——可接受。当某 signal 极昂贵（例如索引整个 workspace），通过 `SignalCache::shareable_for_helper(&parent)` 的白名单复用机制开放复用。

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

> **用户内容不展开占位符**：
>
> 凡是注入到 `TemplateVars` 的**用户来源**字符串（`CustomSubagentBody` 的 user prompt、`AGENTS.md` 的内容、profile 的 user 配置文本、Skills 的 user 描述）必须经过 `vars.insert_user_text(key, value)` 而非 `vars.insert(key, value)`。前者保证：
>
> 1. 注入文本中的 `{{...}}` **不再被二次展开**（防止用户在自定义 prompt 中写 `{{system_password}}` 反向探测变量）
> 2. 文本中的控制字符 / 不可见字符 (`\u{0000}`–`\u{001F}` 除常见空白) 被替换为可见占位
> 3. 不做 HTML/XML 转义（保留 markdown 结构），但渲染层（§ 3.14）若选用 XML renderer 会做 `<` `>` `&` 转义
>
> 实现上 `insert_user_text` 在内部把 value 中的 `{{` 替换为不可冲突的占位符，渲染完成后再换回——保证用户文本字面量原样保留，但渲染引擎只做一遍替换。

收益：

- 文案 diff 直接可读（`git diff templates/behavioral_guidelines.md` 行级清晰）
- 非工程同事可在 IDE 中直接编辑（grammarly、CSpell、PR diff 可读）
- 长度变化能在 PR 审计中显式看到
- 编译期常量保留（`include_str!` 不增加运行时开销），dev 模式下额外支持热重载

### 3.10 失败软降级

错误语义统一在 `SectionOutcome` 内（见 § 3.2.6）：

| 状态 | Composer 行为 | 何时使用 |
|---|---|---|
| `Skip` | 静默丢弃 | 不适用本次构建（如 ActiveGoal 在没有 thread 时） |
| `Produced(body)` | 入列 | 正常 |
| `Degraded { body, warning }` | 入列 + 记录 warning | 部分降级仍可用（如 Skills 部分加载失败） |
| `SoftFailed { code, error }` | 跳过 + warning | 整段无法生成（如 ProjectContext IO 失败） |
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
    pub truncated: bool,
}
```

**`estimated_tokens` 估算实现**：

```rust
pub trait Tokenizer: Send + Sync {
    fn estimate(&self, text: &str) -> usize;
    fn name(&self) -> &'static str; // 写入 audit，便于跨实现对比
}

/// 默认实现：chars / 4，零依赖；适用于英文 markdown，中文/CJK 偏低估
pub struct HeuristicTokenizer;

/// 可选启用：按 Anthropic / OpenAI 分词器精确计数（feature = "tokenizer-tiktoken"）
/// 仅在审计采样路径使用，避免 hot-path 性能损耗
pub struct TiktokenTokenizer { … }
```

- `audit.estimated_tokens` 字段值由 `Composer` 在渲染完成后统一调用 `cx.tokenizer.estimate(&block.text)` 写入
- 默认 `HeuristicTokenizer`，hot-path 无额外依赖
- `audit.tokenizer = "heuristic" | "tiktoken-cl100k_base"`，便于跨版本对比
- 警告：以 estimated_tokens 计算 budget 时，若 tokenizer 估算偏差 ±20%，可能导致截断不到位 → § 3.12 budget 用**字符数**计算，token 仅用于审计

**版本字段语义（不承诺自动回放）**：

`schema_version` + 每 Section `version` 写入 `agent_runs` 审计字段，**仅用于事故复盘的人类可读性**：

- 看到事故 run 的 system prompt schema_version=42，可去 git 找到对应 PR / 模板版本
- **不承诺**按版本回放——回放需要保留所有旧 Source 实现 + 旧模板 + 旧 BuildSignal 实现，工程代价过高
- 审计表只存 `(schema_version, [(section_id, version)])` JSON，不存完整 prompt 文本（隐私 + 体积）
- 必要时可由调用方在事故现场记录完整 prompt 到旁路存储（受 `Redactor` 脱敏）

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
    "system prompt composed",
);
```

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

impl PromptBudget {
    /// Model-aware 构造：把 context window 转成字符预算（启发式 1 token ≈ 4 chars，
    /// 安全裕度 0.3）。调用方应当传入 ModelTarget，避免对不同 context window
    /// 的模型用同一份硬编码上限。
    pub fn for_model(model: &ModelTarget, surface: &PromptSurface) -> Self {
        let ctx = model.context_window();
        let total_chars = (ctx as f32 * 4.0 * 0.30) as usize;
        let per_section_default_chars = (total_chars as f32 * 0.10) as usize;
        let mut per_section_overrides = BTreeMap::new();
        // BehavioralGuidelines / FinalResponseStructure 是大头静态文案，给更大配额
        per_section_overrides.insert(SectionId::BehavioralGuidelines, total_chars / 2);
        per_section_overrides.insert(SectionId::FinalResponseStructure, total_chars / 4);
        // 用户来源 Section 给更紧的配额，防止滥用
        per_section_overrides.insert(SectionId::ProjectContext, total_chars / 8);
        per_section_overrides.insert(SectionId::CustomSubagentBody, total_chars / 4);
        // Compaction / Title Surface 用更紧的总预算
        let total_chars = match surface {
            PromptSurface::Compaction { .. } | PromptSurface::Title => total_chars / 2,
            _ => total_chars,
        };
        Self {
            total_chars,
            per_section_default_chars,
            per_section_overrides,
            eviction_order: vec![
                PromptLayer::Ephemeral,
                PromptLayer::RuntimeOverlay,
                PromptLayer::SessionStable,
                PromptLayer::StablePrefix,
            ],
        }
    }
}
```

Composer 行为：

1. **per-section 检查**：每个 Source 返回后，若 `body.markdown.len()` 超出 `per_section_overrides` 或 `per_section_default_chars` → `body.truncate_with_marker()`（保留头/尾 + `… [truncated N chars] …`），写 `SectionWarning::Truncated`，audit `truncated = true`
2. **全局检查**：所有 Section 渲染完后若 total 超限 → 按 `eviction_order` 删 Section（先丢 Ephemeral 中 `order_hint` 最大的；同 Layer 内按 size 降序选择）
3. **底线保护**：仍超限 → StablePrefix 内的 Section 截断而非删除（删除会破坏行为契约）
4. 全程审计落 `ComposedPrompt.warnings`，触发 `prompt.budget.truncated` / `prompt.budget.evicted` metric，超阈值告警

`PromptBudget` 的实际数值是**运行时配置**，**不进入 schema_version**（§ 3.19）；但调整默认值 / 默认 eviction 顺序需要发版说明 + 灰度。

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

### 3.14 渲染抽象：`SectionRenderer`

不同 LLM 对 system prompt 的"段落标记"敏感度差异大：Anthropic 偏好 XML 标签，OpenAI 系偏好 markdown，部分本地模型对 `## ` 之外的标题响应差。把"如何拼一个 Section 文本"抽离成 trait：

```rust
pub trait SectionRenderer: Send + Sync {
    /// 把 (title, body) 渲染为这个 provider 偏好的段落格式
    fn render_section(&self, title: &str, body: &str) -> String;
    /// Layer 之间的分隔符（默认 "\n\n"）
    fn layer_separator(&self) -> &'static str { "\n\n" }
    /// renderer 名字写入 audit
    fn name(&self) -> &'static str;
}

/// 默认：## title\n\n body，与现状对齐
pub struct MarkdownRenderer;

/// XML：<section name="title">body</section>
/// Anthropic 在长 system prompt 下可显著提升 section recall
pub struct XmlRenderer;
```

- `BuildCx::renderer` 由调用方根据目标 model 选择
- 阶段 1 byte-equal 双轨强制使用 `MarkdownRenderer`
- 阶段 5 之后允许灰度 `XmlRenderer`，但**必须**与 cache_purity / 快照测试套件对齐
- renderer 名字进入 `SectionAudit.renderer` 字段，事故复盘可见

**灰度切换路径**：

`SectionRenderer` 是**全局影响**的开关——切换会让 system prompt 字面 100% 改变，prefix cache 全量失效。因此切换不能简单 PR 合并即生效，必须遵循：

2. **新 renderer 实现先并行存在**：以 `RendererCandidate { name, instance, enabled_models: HashSet<ModelTarget> }` 注册到 `RendererRegistry`，不替换默认
3. **per-model 灰度**：`BuildCx::renderer` 由调用方根据 `ModelTarget` 选取——同进程不同模型可使用不同 renderer，互不影响 cache
5. **schema_version bump**：每次默认 renderer 变更必须 bump `registry.schema_version`（§ 3.19 表格已列出此规则），方便事故复盘按 schema_version 切片
6. **回退**：旧 renderer 至少保留两个发版周期（约 4 周）才允许移除；环境变量 `PROMPT_RENDERER_FORCE = "markdown"` 提供应急回退

### 3.16 Surface 扩展点：闭包枚举 + 单点新增

§ 3.2.1 的 `PromptSurface` 是**封闭枚举**，新增一个 Surface（例如未来的 `Evaluation`、`Replay`）会牵动 § 3.2.7 `SurfacePattern`、§ 3.5 决策矩阵等多处。把"新增 Surface 的展开点"集中显式化，避免开放扩展时漏改：

```rust
/// 单点新增 Surface 的契约清单。Composer 在启动期检查每个 PromptSurface 变体
/// 是否同时在以下四处出现，缺任意一处则启动 lint 失败。
pub trait SurfaceExtension {
    /// 1. 该 Surface 的 SurfacePattern 变体（见 § 3.2.7）
    fn pattern(&self) -> SurfacePattern;
    /// 2. 该 Surface 默认 PromptBudget（见 § 3.12）
    fn default_budget(&self) -> PromptBudget;
    /// 3. 该 Surface 是否参与 RuntimeMessageInjector（见 § 3.7）
    fn runtime_message_enabled(&self) -> bool;
    /// 4. 该 Surface 默认 SectionRenderer（见 § 3.14）
    fn default_renderer(&self) -> Arc<dyn SectionRenderer>;
}
```

启动期 `cargo test prompt::surface_extensions_complete` 用 `strum::EnumIter` 遍历 `PromptSurface` 所有变体，对每个变体解析 `SurfaceExtension` 实现；任意一项缺失 → 测试失败。**新增 Surface 时只需在一个文件 `surface_extensions.rs` 实现该 trait**，无需散落地修改四处。

### 3.18 Source 副作用约束：只读、幂等、可重放

`SectionSource::build` 在并发执行 + SignalCache memoize 的语义下，必须严格遵守如下约束，否则会破坏审计可重放性与并发安全：

| 约束 | 说明 | 违反后果 |
|------|------|---------|
| **只读** | Source 不得通过 `cx.pool` 执行任何 `INSERT/UPDATE/DELETE`；不得写文件、发网络请求、修改进程级全局状态 | 通过自定义 `ReadOnlyPool` wrapper 在 debug build 强制；release build 由 code review + 检查清单守 |
| **幂等** | 同一 `BuildCx` 上同一 Source 多次调用必须返回语义等价结果（允许 `Duration` 字段差异） | `cargo test prompt::source_idempotency` fixture 串行调用 2 次后 diff 正文必须为空 |
| **可重放** | Source 的输出**只能**依赖 `BuildCx` 显式字段 + `SignalCache` + 静态模板 + `cx.features`；禁止读 `std::env`、`SystemTime::now()`、`thread_rng` | `cargo test prompt::source_determinism` 注入 deterministic clock + sealed env，校验输出稳定 |
| **无外部副作用** | 不允许打日志超过 `tracing::trace!`；warning 走 `SectionOutcome::Degraded { warning }` 而非 `tracing::warn!` 直接调用 | 让 `ComposedPrompt.warnings` 成为唯一审计源 |
| **失败可解释** | `SoftFailed.code` 必须在 `prompt::error_codes` 常量集中注册；不允许临时硬编码字符串 | `cargo test prompt::error_codes_registered` 扫源码 |

时间相关数据通过 `BuildCx::clock: Arc<dyn Clock>` 注入，默认实现是 `SystemClock`，测试时替换为 `FixedClock(timestamp)` —— 配合 § 3.7 的 `CurrentDateInjector` 走消息层，Source 内不再有任何 `Utc::now()` 调用。

### 3.19 schema_version vs Section version 的 bump 规则

§ 3.11 提到二者会写入审计表，但何时 bump 哪一个之前未定义。明确规则：

| 变更类型 | bump `SectionSpec.version` | bump `registry.schema_version` |
|---------|---------------------------|-------------------------------|
| Section 模板正文文案修改 | ✅ +1 | ❌ |
| Section 模板新增/移除占位符 | ✅ +1 | ❌ |
| Section 切换 `LayerResolver` | ✅ +1 | ✅ +1（缓存语义改变） |
| Section 新增 / 删除 | 新 Section 从 1 开始 | ✅ +1 |
| `SurfaceMatcher` 调整 | ✅ +1 | ✅ +1（覆盖范围改变） |
| `SectionOrder` / `SectionAnchor` 调整 | ✅ +1 | ❌ |
| 新增 / 删除 `PromptSurface` 变体 | — | ✅ +1 |
| `PromptLayer` 枚举调整 | — | ✅ +1 |
| `RuntimeMessageInjector` 列表调整 | — | ✅ +1 |
| `SectionRenderer` 全局默认切换 | — | ✅ +1 |
| `PromptBudget` 默认值调整（仅数值） | — | ❌（运行时配置，不入 schema） |
| 仅 metric / tracing 字段增减 | — | ❌ |

`schema_version` 是**全局单调整数**，提交者必须在 PR 模板中勾选"已 bump schema_version"复选框。

**CI 工程化降级实现**：自动判定"哪些代码变更必须 bump schema_version" 在工程上不可靠（涉及跨文件语义分析），因此 `cargo test prompt::schema_version_monotonic` 采用三级守门：

| 守门级 | 检查方式 | 失败处理 |
|--------|---------|---------|
| L1 hard（CI 必跑） | base 分支 `schema_version` 与当前分支字面比较；只允许 `cur > base` 或 `cur == base` | 若 `cur < base` → 直接 fail（防止合并冲突时把版本号搞回退） |
| L2 hint（CI 必跑） | 扫描 diff 中是否触及白名单文件（`registry.rs`, `surface.rs`, `layer.rs`, `templates/**/*.md`, `sources/**/*.rs`），且 `schema_version` 未 bump → 输出 `WARN`（非 block） | 输出 GitHub Actions annotation；reviewer 必须在 PR 描述确认"无需 bump"或补 bump |
| L3 soft（dev guideline） | 在 PR 模板提供"是否触发 § 3.19 表格中需 bump 行" 的 self-check checklist；reviewer 在 review checklist 中复核 | 流程性约束 |

**为什么不做"自动决定该 bump 哪个"**：
- 模板文案改 1 字 vs 改整段 vs 切换 Section ID，从 diff 静态分析判定语义影响代价过高
- 跨 Section anchor 调整等隐式影响难以扫描
- 留给开发者 + reviewer 协同决策更稳健；自动化只覆盖"显著漏 bump"

PR 模板增加：

```markdown
## Prompt schema impact
- [ ] 不涉及 `prompt::*` 模块
- [ ] 涉及；已按 § 3.19 规则 bump `schema_version`：__前 → 后__
- [ ] 涉及；按 § 3.19 表格不需要 bump（说明：______________）
```

### 3.20 模板 front-matter 与 Section version 绑定

模板与代码端 Section version 必须双向绑定，否则只改模板不改代码 / 只改代码不改模板都会让审计版本与实际内容脱钩。

每个 `templates/**/*.md` 文件首部加 YAML front-matter：

```markdown
---
section_id: BehavioralGuidelines
version: 7
declared_keys: []           # 显式声明占位符 key（与 § 3.9 strict 模式同源）
---
You are TiyCode, an autonomous coding agent...
```

启动期 `cargo test prompt::template_version_sync` 校验：

1. 每个引用模板的 Source 在 `SectionSpec.version` 与模板 `front-matter.version` 必须**严格相等**
2. 模板 `section_id` 必须与 Source 注册的 `SectionId` 字面量一致
3. 模板 `declared_keys` 必须是 § 3.9 `render_template_strict` 调用处 `declared_keys` 的超集（允许代码端少声明做 graceful degrade，但不允许多声明）

`include_str!` 编译期会读到 front-matter，加载时由 `Template::parse` 剥离 front-matter 后只把正文交给渲染层；front-matter 的 `version` 字段同时作为 `SectionAudit.template_version` 字段写入审计——比代码端 `SectionSpec.version` 更细：模板侧文案修订可单独追踪。

### 3.21 散落入口归并清单（含被遗漏项）

§ 1.5 列出的入口在阶段 6 统一归并；这里完整化清单并明确每个入口的迁移目标，避免遗漏：

| 现有入口 | 迁移目标 | 备注 |
|---------|---------|-----|
| `agent_run_summary::build_compact_summary_system_prompt` | `Composer::build(PromptSurface::Compaction { kind: Compact }, …)` | § 4 阶段 6 |
| `agent_run_summary::build_merge_summary_system_prompt` | `Composer::build(PromptSurface::Compaction { kind: Merge }, …)` | § 4 阶段 6 |
| `agent_run_title::build_title_prompt_from_messages` 中的 system 部分 | `Composer::build(PromptSurface::Title, …)` | § 4 阶段 6；user message 部分仍由调用方拼装 |
| `agent_run_summary::build_implementation_handoff_prompt` | **保留为 user message 构造器**，但其中"角色 / 风格"指令通过 `Composer::build(PromptSurface::Title, …)` 提取 → 拼到 user message | 这是 user message 而非 system prompt；不直接走 Composer，但共享 `ProfileInstructionsSource` 文本片段（通过 `Composer::render_section_only(SectionId::ProfileInstructions, ...)` 暴露的子接口） |
| `subagent::runtime_orchestration::SubagentProfile::system_prompt` | `Composer::build(PromptSurface::SubagentExplore / Review / Custom, …)` | § 4 阶段 2b |
| `agent_session::inject_goal_context` | `ActiveGoalSource`（Ephemeral） | § 4 阶段 4 |

新增的子接口 `Composer::render_section_only(id, surface, cx)`：返回 `Option<SectionBody>`，**绕过装配链路**，仅渲染单个 Section 用于 user message 拼装等场景；该接口不打 cache marker、不进入 audit、不参与 budget——属于"借用 Section 实现，不属于 prompt"。

**BuildCx 隔离**：`render_section_only` 内部用 `BuildCx::for_section_only(parent_cx)` 派生独立子 cx：

| 字段 | 派生策略 |
|------|---------|
| `signals` | **新建** `SignalCache::standalone()`——避免污染调用方主路径的 SignalCache |
| `features` | 复用 |
| `clock` | 复用 |
| 其余 | 复用 |

**禁止规则**：
1. 调用方**不得**在拿到 `SectionBody` 后再调用 `Composer::build` 主路径——分离调用，避免上下文混乱
2. `RuntimeMessageInjector` 在该路径下**不触发**（它是消息层职责，user message 构造器自己决定是否注入运行时上下文）
3. 调用点必须在文档/代码注释中显式说明用途；`tracing::trace!(target="prompt.render_section_only", id=?id)` 强制埋点

### 3.22 子代理继承的 Section 默认清单

子代理 Surface（`SubagentExplore` / `SubagentReview` / `SubagentCustom`）从父主代理"继承"哪些 Section，是行为契约——以前由字符串解析的 `HELPER_INHERITED_SECTION_TITLES` 实现，现在分散到各 Source 的 `surfaces: SurfaceMatcher` 字段上。**散落的真相源容易漏配**，必须集中维护一份对照表 + 启动期 lint：

```rust
/// 真相源：哪些 Section ID 必须出现在每个子代理 Surface 上。
/// 维护方式：增删 Section / 调整 SurfaceMatcher 时**同步**修改此清单；
/// 启动期 lint 强制 (清单 ⊆ registry filter 结果)。
pub const SUBAGENT_INHERITED_SECTIONS: &[(SubagentSurfaceKind, &[SectionId])] = &[
    (SubagentSurfaceKind::Explore, &[
        SectionId::Role,
        SectionId::SystemEnvironment,
        SectionId::ProjectContext,
        SectionId::ProfileInstructions,
        SectionId::WorkspaceLocation,
        SectionId::ShellToolingGuide,
        SectionId::SubagentOutputContract,
    ]),
    (SubagentSurfaceKind::Review, &[
        SectionId::Role,
        SectionId::SystemEnvironment,
        SectionId::ProjectContext,
        SectionId::ProfileInstructions,
        SectionId::WorkspaceLocation,
        SectionId::ShellToolingGuide,
        SectionId::SubagentOutputContract,
    ]),
    (SubagentSurfaceKind::Custom, &[
        SectionId::Role,
        SectionId::SystemEnvironment,
        SectionId::ProjectContext,
        SectionId::ProfileInstructions,
        SectionId::WorkspaceLocation,
        SectionId::CustomSubagentBody,
        SectionId::SubagentOutputContract,
    ]),
];
```

启动期测试 `cargo test prompt::subagent_inheritance_complete`：
1. 对每个 `SubagentSurfaceKind`，构造一个最小 `PromptSurface` 实例
2. 调用 `registry.iter().filter(|s| s.surfaces.matches(&surface))` 得到实际清单
3. 必须满足 `SUBAGENT_INHERITED_SECTIONS[kind] ⊆ 实际清单`——超集允许（增加新 Section），子集不允许（漏继承）
4. **额外不允许**：BehavioralGuidelines / FinalResponseStructure 出现在子代理 Surface 上（这是主代理专属契约）；启动期 lint 强制断言

修改 `SUBAGENT_INHERITED_SECTIONS` 必须 bump `schema_version`（§ 3.19 表格"`SurfaceMatcher` 调整" 行）。

---

## 四、迁移步骤（增量、可灰度）

### 阶段 0：脚手架（不改语义）

1. 在 `prompt/` 下新增模块：`layer.rs`、`surface.rs`、`section_id.rs`、`registry.rs`、`composer.rs`、`signals.rs`、`templates.rs`、`budget.rs`、`runtime_message.rs`、`exec_policy.rs`、`cache_marker.rs`、`surface_extensions.rs`、`error_codes.rs`、`redactor.rs`、`renderer.rs`、`inheritance.rs`、`clock.rs`，但**不接通**到 `agent_session`
2. 引入新类型：`SectionOutcome`、`SurfacePattern`/`SurfaceMatcher`、`SubagentCacheStability`、`LayerResolver`、`PromptBlock`/`CacheMarker`、`PromptBudget`/`ModelTarget`、`schema_version`、`SourceExecPolicy`、`CacheMarkerArbiter`、`SurfaceExtension`、`Clock`，仅在适配层使用，不影响行为
3. 新增 `prompt/templates/*.md` 目录，仅复制（不修改）现有字面量；**模板 front-matter（§ 3.20）+ 严格模式 + 启动期 lint 测试**全部上线
4. 新增 `SectionSource` trait 与适配器 `LegacyProviderAdapter`，把现有 5 个 `*Provider` 包成 `SectionSource`，但仍允许旧路径并存
5. 上线启动期 lint 测试套件（一次性补齐，避免后续阶段受 lint 阻塞）：`anchors_*`、`templates_*`、`surface_extensions_complete`、`error_codes_registered`、`schema_version_monotonic`、`subagent_inheritance_complete`、`signal_cycle_detected`

### 阶段 1：装配器双轨（主代理 byte-equal 切换）

1. 实现 `Composer::build_main_agent_legacy_compat()`，输出**与现状 byte-equal**（含 phase / order_in_phase 的兼容映射）
2. 加入快照测试：`assert_eq!(legacy_build_system_prompt(...), composer.build_main_agent_legacy_compat(...))`，覆盖：
   - `run_mode = "default"` × 有/无 AGENTS.md × 有/无 Skills × 有/无 Profile × Sandbox 4 种 policy
   - `run_mode = "plan"` 同上
3. 校验 `ComposedPrompt.schema_version` 与每 Section `version` 被正确写入 audit 表
4. 切换 `agent_session::build_system_prompt` 调用到 Composer，保留旧实现一周作为回退方案

### 阶段 2：Surface 化子代理（拆 2a / 2b）

**2a — 双轨观测**：

1. 新增 `SubagentOutputContract`、`ShellToolingGuide(helper)` 等 Section 进入 Registry
2. 保留 `build_helper_system_prompt` 作为生产路径；同时调用 Composer 生成对照版本，**仅记录 hash + length 差异**到 metrics（`prompt.subagent.hash_match`、`prompt.subagent.diff_bytes`）
3. 灰度 7 天，观察 hash_match ≥ 99 % 后进入 2b；不达标 → 回查差异、修补 Source、继续观测

**允许的差异白名单**：

hash_match < 100% 时，diff 必须落在以下"已知良性差异"之一才允许进入 2b；其它差异一律阻断切换：

| 良性差异类型 | 示例 | 判定方式 |
|------------|------|---------|
| 行尾空白归一化 | `body \n` → `body\n` | diff 在 `re.sub(r' +\n', '\n', x)` 之后归零 |
| 双换行→三换行（Layer 间分隔） | `\n\n` → `\n\n\n` | diff 在 `re.sub(r'\n{2,}', '\n\n', x)` 之后归零 |
| Section 顺序变化但内容完全一致 | A,B,C → A,C,B | 按 `## ` 切分后 sort + join 之后归零 |
| 标题大小写归一化 | `Sandbox & permissions` → `Sandbox & Permissions` | case-insensitive diff 归零 |

任何**正文字面**差异（即使一字之差）必须**显式批准**——PR 中标注"接受此 diff"才能合入；否则视为破坏继承语义。

观测期产出脚本 `tools/prompt_diff_classifier.py` 自动分类 diff，输出"良性 / 待审 / 破坏性"三类计数到 dashboard。

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
3. `build_implementation_handoff_prompt` **不直接走 Composer**（它是 user message 构造器），但其中复制的"响应风格 / 响应语言"段落改为通过 `Composer::render_section_only(SectionId::ProfileInstructions, …)` 单段渲染拼接，消除重复源
4. 删除重复的 `response_language` / `response_style` 拼接逻辑——统一在 `ProfileInstructionsSource` 内

### 阶段 7：可观测、灰度与告警

1. 接通 `tracing` 与现有 metrics 通道；为 PromptComposer 添加 dashboards 字段
3. 上线核心告警阈值：
   - `prompt.budget.evicted_ratio > 0.5%` → P2
   - `prompt.budget.truncated_ratio > 1%` → P2
   - `prompt.subagent.hash_match < 99%`（双轨期）→ P1
   - `prompt.cache_purity_violations > 0`（CI 拦截）→ P0
   - `prompt.source.timeout{…} > 0.1%` → P2（§ 3.6.1 单 Source 超时）
   - `prompt.cache_marker.over_request > 0` → P2（§ 3.7.1 消息层超额申请）

---

## 五、目录结构（重构后）

```
src-tauri/src/core/prompt/
├── mod.rs                     # pub use composer::*; pub use surface::*; …
├── composer.rs                # PromptComposer + ComposedPrompt + 渲染逻辑（registry 在 new() 注入）
├── registry.rs                # SectionRegistry + 默认注册函数 + schema_version
├── surface.rs                 # PromptSurface, SurfacePattern, SurfaceMatcher, SubagentCacheStability
├── surface_extensions.rs      # SurfaceExtension trait + 启动期完整性 lint（§ 3.16）
├── layer.rs                   # PromptLayer, LayerResolver, SectionOrder, SectionAnchor
├── section.rs                 # SectionId, SectionSpec, SectionBody, SectionOutcome, SectionAudit
├── source.rs                  # SectionSource trait, BuildCx, BuildSignal, FatalError
├── clock.rs                   # Clock trait + SystemClock + FixedClock（测试用）
├── exec_policy.rs             # SourceExecPolicy + Composer 调度（超时/并发/背压）（§ 3.6.1）
├── signals.rs                 # SignalCache + 内置 signal（policy / writable_roots / …）+ 循环检测 + 失败重试
├── templates.rs               # 占位符渲染器（严格模式 + dev 热重载 + lint + front-matter 解析）
├── budget.rs                  # PromptBudget + PromptBudget::for_model + 截断/驱逐策略
├── cache_marker.rs            # CacheMarkerArbiter + 全局配额仲裁 + 滑动规则（§ 3.7.1）
├── runtime_message.rs         # RuntimeMessageInjector + CompactionPolicy + CurrentDateInjector
├── error_codes.rs             # SoftFailed.code 常量集中注册（§ 3.18）
├── redactor.rs                # PII 脱敏（tracing 字段 + warning 落库前过滤）
├── renderer.rs                # SectionRenderer + Markdown/Xml + RendererRegistry（§ 3.14 灰度切换）
├── inheritance.rs             # SUBAGENT_INHERITED_SECTIONS + lint（§ 3.22）
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
    ├── role.md                            # 含 YAML front-matter（§ 3.20）
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
// Composer 在进程启动时由 default_registry() 注入构造，全局单例
let composer: Arc<Composer> = composer_singleton();
let budget = PromptBudget::for_model(&model_target, &surface);
let cx = BuildCx::for_main_agent(pool, &raw_plan, workspace_path, thread_id, &model_target);

let composed = composer
    .build(
        PromptSurface::MainAgent { run_mode: RunMode::Default },
        cx,
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
        PromptSurface::SubagentExplore { inherited_run_mode: parent_cx.run_mode },
        BuildCx::derive_for_helper(parent_cx, &helper_profile),
        &PromptBudget::for_model(&parent_cx.target_model, &subagent_surface),
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
| 单元（Composer） | mock Source 列表 | Layer 排序、SurfaceMatcher、依赖循环检测、并发软失败聚合、budget 截断/驱逐、超时与并发上限 |
| 模板 lint | `cargo test prompt::templates::lints` | 模板 `{{key}}` ↔ 代码 `declared_keys` 双向一致；front-matter `version` ↔ Source `version` 同步；无遗漏、无死键 |
| Schema 守护 | `cargo test prompt::schema_version_monotonic` | 按 § 3.19 规则强制 schema_version / Section version bump |
| Surface 完整性 | `cargo test prompt::surface_extensions_complete` | 每个 `PromptSurface` 变体在 § 3.16 四处展开点齐备 |
| 错误码注册 | `cargo test prompt::error_codes_registered` | `SectionOutcome::SoftFailed.code` 全部在常量集 |
| 缓存纯净性 | `cargo test prompt::cache_purity` | StablePrefix 内禁止出现 `\d{4}-\d{2}-\d{2}` / thread_id / run_id / 用户名 字面量 |
| Cache marker 配额 | `cargo test prompt::cache_marker_quota` | 极端场景下总 marker ≤ 4 且 system 优先满足 |
| Source 幂等 / 可重放 | `cargo test prompt::source_{idempotency,determinism}` | 同 cx 多次调用结果等价；deterministic clock + sealed env 下输出稳定 |
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
| 软失败掩盖真问题 | `tracing::warn!` + 计数器；超阈值告警 |
| 模板加载错误（路径错） | `include_str!` 编译期失败，零运行时风险；dev 模式热重载失败回退到编译期常量 |
| 模板缺占位符 | 严格模式 → `SoftFailed`，绝不静默拼接；启动期 lint 测试拦截 |
| Budget 误删关键 Section | StablePrefix 走截断而非删除；`eviction_order` 默认末位是 StablePrefix |
| RuntimeMessage 与压缩链双份注入 | `CompactionPolicy::PinOutsideWindow` 标记，消息序列化层强制不压缩 |
| schema 升级导致回放失败 | `ComposedPrompt.schema_version` + 每 Section `version` 写审计表，仅用于人类复盘可读性，不承诺自动回放（§ 3.11） |
| Section 间隐式依赖蔓延 | § 3.2.6 显式禁止 inter-section 依赖；共享通过 `BuildSignal`；锚点仅排序、不影响存在 |
| `SignalCache` 跨 await 持锁导致死锁 | § 3.6 拆为 `Mutex<HashMap>`（短临界区） + `Arc<OnceCell>`（跨 await）的双层结构 |
| 用户自定义 prompt 占位符注入 | § 3.9 `vars.insert_user_text()` 不二次展开 `{{...}}`；启动期 lint 拦截 |
| 子代理 build 误用父 cx 缓存 | § 3.8.1 helper 派生新建空 `SignalCache`；features 复用 |
| 多 Injector 顺序不稳定 | § 3.7 同 placement 下按注册顺序 + 名字字典序，结果可重现 |
| 跨模型渲染格式差异 | § 3.14 `SectionRenderer` 抽象；renderer 切换计入 schema_version bump |
| 锚点目标缺失/成环 | § 3.4 启动期 lint 测试 + 运行时退化为 Default + warning |
| 新增依赖引入复杂度 | 仅引入 `async-trait`（已有）+ 一个 ~50 行的占位符渲染器 + `serde_yaml`（front-matter，已存在为可选 dep）；`tiktoken-rs` 仅作为可选 feature；不引入 handlebars / tera |
| 单 Source 慢查询拖垮整次 build | § 3.6.1 `per_source_timeout` 默认 250 ms + `overall_build_timeout` 800 ms；超时记 SoftFailed 而非阻塞 |
| 高并发 build 打满 SQLite 连接池 | § 3.6.1 `layer_concurrency` 默认 8 + 调用方层面外部 `Semaphore` 限制并发 build |
| 消息层与 system prompt 抢 cache marker 配额 | § 3.7.1 `CacheMarkerArbiter` 全局仲裁；超额申请被强制裁剪 + metric 告警 |
| 新增 Surface 漏改展开点 | § 3.16 `SurfaceExtension` trait + `surface_extensions_complete` lint |
| Source 偷偷写库 / 读时间 / 读环境 | § 3.18 副作用约束 + `prompt::source_{idempotency,determinism}` 测试 + debug build `ReadOnlyPool` wrapper |
| 模板与代码 version 脱钩 | § 3.20 模板 front-matter `version` 与 `SectionSpec.version` 启动期强制相等 |
| schema_version 漏 bump | § 3.19 PR 模板复选框 + `schema_version_monotonic` CI lint |
| `build_implementation_handoff_prompt` 在迁移中漏归并 | § 3.21 单独列出；通过 `Composer::render_section_only` 共享 ProfileInstructions 文本 |
| `SignalCache` init 失败永久 poison | § 3.6 OnceCell 不 set 失败值，写 `SignalResult::Failed` 标记；同 cx 不重试，下一次 build（新 cache）可重试 |
| `SignalCache` 出现循环依赖（A→B→A） | § 3.6 `in_flight` 标记 + `Failed(Cycle)` 显式失败；`signal_cycle_detected` 测试 |
| Cache marker 落在已被预算掏空的 Layer | § 3.7.1 Layer 滑动规则：仅向非空 Layer 打 marker，过短 Layer 不打；按 ModelTarget 决定 `min_marker_chars` |
| 不同 model context window 用同一份硬编码上限 | § 3.12 `PromptBudget::for_model(&ModelTarget, &surface)` 派生预算 |
| `cache_stability` 通过 profile 注入但 LayerResolver 拿不到 | § 3.2.1 提升到 `PromptSurface::SubagentCustom { cache_stability }`；surface 自洽 |
| `CustomSubagentBody` 不知该渲染哪条 prompt | § 3.6 `BuildCx::custom_subagent_slug` 显式传入 |
| 切换默认 SectionRenderer 让 prefix cache 全失效 | § 3.14 per-model 选择 + `PROMPT_RENDERER_FORCE` 应急回退；schema_version 强制 bump |
| `Composer::render_section_only` 污染主路径 SignalCache | § 3.21 内部用 `BuildCx::for_section_only` 派生独立 SignalCache；不触发 RuntimeMessageInjector |
| schema_version_monotonic 自动判定不可靠 | § 3.19 三级守门：L1 严格不退步 + L2 改动 hint + L3 PR 模板复选框 |
| 子代理继承清单散落到各 Source 易漏配 | § 3.22 集中维护 `SUBAGENT_INHERITED_SECTIONS` + `subagent_inheritance_complete` 启动期 lint |
| Composer 入口签名不一致（registry 是参数还是构造时注入） | § 3.3 统一：registry 在 `Composer::new` 注入，`build()` 不传 |

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
| 可观测 | 无 | `SectionAudit`（含 version / truncated）+ tracing + Redactor 脱敏 + 告警阈值 |
| 多 Surface 公用原语 | summary / title / subagent 各写各的"响应语言/风格" | 同一 `ProfileInstructionsSource` 在所有 Surface 复用；`LayerResolver::PerSurface` 处理跨 Surface 缓存语义差异 |
| 测试覆盖 | 2 个零碎单测 | 每个 Source 四态单测 + 全 Surface 快照 + 兼容双轨 + 缓存纯净性 + 模板 lint + 预算 fuzz + 超时/并发 + 幂等/可重放 + Surface 完整性 + Schema 守护 |
| 事故复盘 | 无版本信息 | `schema_version` + 每 Section `version`（与模板 front-matter 强绑定）写 `agent_runs`，bump 规则在 § 3.19 显式化 |
| 执行模型 | 无并发/超时控制 | § 3.6.1 per-source 250 ms 超时 + 同 Layer 并发上限 + overall build 超时；§ 3.18 强制只读/幂等/可重放 |
| Cache marker 仲裁 | 由各路径自行打标，易超 4 个上限 | § 3.7.1 `CacheMarkerArbiter` 请求级单例统一配额（默认 system 2 / 消息层 2，可动态再分配） |
| 新增 Surface | 改散落多处（pattern / matcher / 决策矩阵 / renderer） | § 3.16 一个 `SurfaceExtension` 实现 + 启动期完整性 lint 自动校验 |
| Implementation handoff 等 user message 共享 | 各自重复 ProfileInstructions 文案 | § 3.21 `Composer::render_section_only` 子接口，user message 路径单段复用 Section |

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
| `build_implementation_handoff_prompt`（user message 构造器） | 保留入口；其中"响应风格 / 语言"段通过 `Composer::render_section_only(SectionId::ProfileInstructions, …)` 单段复用 |
