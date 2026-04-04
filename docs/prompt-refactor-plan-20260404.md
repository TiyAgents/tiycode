# 当前项目 Prompt 重构方案

## 背景与目标

当前项目已经具备较完整的 Prompt 工程基础。主 Agent 的 System Prompt 在 `src-tauri/src/core/agent_session.rs` 中由 `build_system_prompt` 统一拼装，已经覆盖了角色定义、行为准则、回复结构、项目上下文、系统环境、权限与沙箱、Shell 指南、Profile 指令、运行模式与运行时上下文等内容。Subagent 侧也在 `src-tauri/src/core/subagent/orchestrator.rs` 中通过 `build_helper_system_prompt` 继承了父 Prompt 的部分能力。

这套实现已经能支撑当前产品，但随着工具、运行模式、 Profile、Helper、未来插件或 MCP 能力逐步增加，当前“单函数线性拼接字符串”的方式会越来越难维护。它的主要问题不是功能缺失，而是扩展成本和可观测性不足。每增加一类能力，系统都更倾向于继续向一个大函数里堆规则，长期会降低 Prompt 的清晰度、稳定性和测试可控性。

本方案的目标不是重写 Prompt 文案，而是把当前 Prompt 系统从“集中式字符串拼装”重构为“分层、可裁剪、按场景注入、可测试、可调试”的 Prompt 组装机制，在尽量不改变现有行为的前提下提升长期可维护性。

本设计文档面向当前项目团队协作，使用中文描述；实际 Prompt 文案、代码标识和测试断言仍以现有英文实现为主。

## 当前实现概览

当前实现的核心入口是 `build_system_prompt(pool, raw_plan, workspace_path, run_mode)`。它按固定顺序拼装多个 section，最终返回一个完整的 System Prompt 字符串。当前已有的 section 大致包括：

1. `Role`，用于定义 Agent 身份与能力边界。
2. `Behavioral Guidelines`，用于规定工具使用、沟通方式、澄清与计划流程、委派时机、总结方式等。
3. `Final Response Structure`，用于约束最终回答格式。
4. `Project Context`，用于注入仓库中的 AGENTS.md 或其他工作区指令。
5. `System Environment`，用于说明系统环境与工具可用性。
6. `Sandbox & Permissions`，用于说明工作区、审批策略和可写边界。
7. `Shell Tooling Guide`，用于指导 shell 与文件工具的选择。
8. `Profile Instructions`，用于合并 profile 自定义指令、语言与回答风格。
9. `Run Mode`，用于区分 default 与 plan 模式。
10. `Runtime Context`，用于注入日期与工作目录。

这一结构已经具备 section 化雏形，但 section 仍然由单个函数集中决定，缺少更明确的分层模型、能力开关、继承策略和调试手段。

## 重构目标

本次重构建议围绕以下五个目标推进。

1. 让 Prompt 具备清晰的分层结构，区分稳定规则与动态上下文。
2. 让不同能力、模式和会话状态只注入真正需要的规则，避免无关 Prompt 干扰。
3. 让主 Agent 与 Helper 的 Prompt 继承关系更明确，减少不必要的信息复制。
4. 让 Prompt 具备更好的可测试性、可观测性和差异分析能力。
5. 让未来新增工具、插件、MCP 或运行模式时，不需要继续扩展单个大函数。

## 设计原则

### 保持行为兼容优先

重构初期应以“结构重组、不改变默认行为”为原则。也就是说，优先把已有规则迁移到新的 section provider 体系中，先保证生成结果的语义接近当前版本，再逐步引入新的约束和裁剪逻辑。Phase 1 不应同步引入新的 Prompt 策略，以避免结构重构与行为变更耦合，增加回归风险。

### 区分静态规则与动态上下文

Prompt 中有些内容几乎总是稳定的，例如角色定义、沟通原则、输出结构；也有些内容会随 session、profile、workspace、run mode 变化，例如语言、风格、权限、路径、日期与工具能力。两者应使用不同的数据来源和组装方式，避免它们在同一段长字符串中混杂。

### 条件注入优于全量注入

只有当某个能力、模式或上下文真实存在时，才应注入相应 section。例如没有启用 review helper 时，不需要在主 Prompt 中反复强调 review helper 规则；没有终端面板能力时，也不需要加过多 terminal session 说明。

### Prompt 本身应被视为产品运行时配置

Prompt 不是一段不可见文案，而是一个可调试、可测试、可演进的运行时策略系统。应为它提供可枚举的 section、可比较的输出、以及必要的调试信息。

## 目标架构

建议将 Prompt 系统重构为“Prompt Context + Prompt Section Provider + Prompt Assembler”的三层结构。

### 一、Prompt Context

首先定义一个统一的上下文对象，集中承载构建 Prompt 所需的所有信息，避免各个函数自行查询和拼接。这个上下文可以命名为 `PromptBuildContext`。它应尽量暴露已经解析和归一化后的字段，而不是让 provider 面对多个原始数据源自行决定读取哪一个。

建议 `PromptBuildContext` 至少包括以下字段：

- `workspace_path`，用于注入工作区路径与工作区指令。
- `run_mode`，用于区分 default、plan 或未来新增模式。
- `resolved_profile`，用于承载已完成解析的 profile 信息，包括语言、风格和自定义指令。
- `tool_capabilities`，用于表达当前会话实际启用的工具或 helper 能力。
- `sandbox_context`，用于表达审批策略、边界路径、只读或可写状态等。
- `environment_context`，用于表达操作系统、架构、shell、常见命令可用性等。
- `workspace_instructions`，用于承载 AGENTS.md 等仓库约束。
- `runtime_metadata`，用于承载当前日期、线程级上下文或其他元数据。

如果构建 Prompt 的过程中仍需要读取 `runtime_plan` 等原始对象，应在 context 构建阶段完成归一化，再把结果暴露给 provider。Provider 应尽量只依赖 `PromptBuildContext` 的标准化字段，避免出现 `runtime_plan` 与 `resolved_profile` 并存且职责不清的情况。

### 二、Prompt Section Provider

在上下文之上，引入可扩展的 section provider 机制。建议定义一个统一结构，例如：

```rust
enum PromptPhase {
    Core,
    Capability,
    WorkspacePreference,
    RuntimeContext,
}

struct PromptSection {
    key: &'static str,
    title: &'static str,
    body: String,
    phase: PromptPhase,
    order_in_phase: u16,
}
```

同时定义一个 provider 接口，例如：

```rust
trait PromptSectionProvider {
    fn collect(&self, ctx: &PromptBuildContext) -> Result<Vec<PromptSection>, AppError>;
}
```

其中，`PromptPhase` 用于表达四层固定语义顺序，`order_in_phase` 用于处理同一层内的稳定排序。初期不建议引入任意整数 `priority`，以避免 provider 数量增多后排序难以推理和调试。初期也不建议在 `PromptSection` 中加入用途尚不明确的 `dynamic` 字段；如果后续需要为缓存、差异比较或调试视图引入更多元数据，应在明确 assembler 消费方式后再补充。

不同模块分别产出自己的 section，最后由统一装配器排序、过滤并拼接。建议初期按较粗粒度拆分 provider，避免在 Phase 1 过早引入大量文件。可先使用以下逻辑分组：

1. `BaseProvider`，用于输出 `Role`、基础行为准则与最终回答结构。
2. `WorkspaceProvider`，用于输出项目上下文与仓库指令。
3. `EnvironmentProvider`，用于输出系统环境、沙箱与 shell 能力说明。
4. `ProfileProvider`，用于输出语言、风格、run mode 与其他会话级动态信息。
5. `CapabilityProvider`，用于根据 helper、terminal panel、未来 MCP 或插件能力补充场景化约束。

这样做以后，`build_system_prompt` 不再直接负责所有文案细节，而是负责收集上下文、调用 provider、排序和拼接。

### 三、Prompt Assembler

Assembler 的职责应尽量单一，只做以下几件事：

1. 收集 `PromptBuildContext`。
2. 调用一组 provider。
3. 根据条件过滤掉空 section 或未启用能力对应的 section。
4. 按 phase 与固定顺序排序。
5. 输出最终字符串。
6. 在开发或调试模式下，保留 section 清单与中间结果用于观测。

最终 `build_system_prompt` 可以退化为一个很薄的入口，而不再承载大部分 Prompt 业务逻辑。

## 推荐的 Prompt 分层

建议在语义上把 Prompt 分为四层，而不是把所有 section 看成同一层级。

### 第一层：稳定核心层

这一层包含不依赖具体会话的长期稳定规则，通常变化频率最低。建议包括：

1. Agent 身份与职责边界。
2. 通用行为准则，例如先说明下一步、读后改、风险先提示、总结不要伪造过程等。
3. 最终回答结构要求。

这一层适合做成常量或模板函数，保持高稳定性。

### 第二层：能力与工具策略层

这一层根据当前产品能力决定注入哪些规则，主要负责“如何使用工具”和“何时使用某能力”。建议包括：

1. 文件工具与 shell 的优先级矩阵。
2. term panel 与 one-shot shell 的边界。
3. helper 的使用时机。
4. verify/review helper 的职责边界。
5. 未来 plugin 或 MCP 的专用行为约束。

这一层应强依赖 capability，而不是默认总是全量注入。

### 第三层：工作区与用户偏好层

这一层负责承载与当前仓库和当前用户偏好相关的内容。建议包括：

1. AGENTS.md、项目结构与协作规范。
2. profile 自定义指令。
3. 回答语言。
4. 回答风格。
5. 其他线程级或用户级偏好。

这一层应支持明确的优先级覆盖规则，例如 runtime plan 覆盖 profile，profile 覆盖默认值；但具体解析应在 context 构建阶段完成，provider 只消费归一化结果。

### 第四层：会话动态上下文层

这一层承载本次运行中变化最频繁的信息，建议始终放在 Prompt 尾部。建议包括：

1. 当前日期。
2. workspace path。
3. 当前 run mode。
4. 当前系统环境。
5. 当前审批与沙箱状态。

这层内容应尽量结构化和精简，不应与核心行为规则混写。

## 结构重构项与策略增强项的边界

为避免范围失控，本方案明确区分“结构重构项”和“策略增强项”。

结构重构项的目标是把现有 Prompt 从单函数拼接迁移到 provider 体系中，同时尽量保持现有行为不变。它包括：

1. 引入 `PromptBuildContext`、`PromptSection` 与 assembler。
2. 建立 phase 化的 section 排序机制。
3. 将现有 section 文案迁移到粗粒度 provider 中。
4. 建立 snapshot、关键内容断言和基础调试能力。

策略增强项的目标是优化 Prompt 内容本身的行为约束。它包括：

1. 工具替代矩阵强化。
2. clarify、update_plan 与风险确认边界细化。
3. 验证结果如实汇报策略强化。
4. Helper Prompt 的受控继承优化。

策略增强项应在结构迁移稳定后逐步引入，不应与 Phase 1 结构迁移同时进行。

## 后续值得增强的策略能力

以下能力仍然值得做，但应在结构重构完成并稳定后分阶段引入。

### 一、工具替代矩阵

当前 Prompt 已经表达了“优先使用 read/search/find/edit”，但还不够具体。建议把这部分升级为更明确的工具选择规则，减少模型误用 shell 的概率。建议新增独立 section，明确说明：

1. 读取文件内容时优先使用 `read`，不要用 shell 执行 `cat`、`head`、`tail` 或 `sed` 读取文件。
2. 搜索文本内容时优先使用 `search`，不要先用 shell 的 `grep` 类方案。
3. 搜索文件路径时优先使用 `find`。
4. 编辑已有文件时优先使用 `edit`，只在新建文件或整文件重写时使用 `write`。
5. 运行一次性的非交互命令时使用 `shell`。
6. 需要与桌面端持久终端会话交互时，只使用 `term_*` 工具，不要把它与 `shell` 混用。

### 二、确认、澄清与自主推进边界

当前 Prompt 已经包含 clarify 与 update_plan 的基本规则，但建议补上更细的“何时该自己做、何时该问”的策略。建议增加独立 section，明确：

1. 目标明确、范围小、风险低且可逆的动作可直接执行。
2. 跨文件、架构性、影响范围不清的改动应先分析，必要时先 `update_plan`。
3. 需求存在多个同样合理的方向时，应先 `clarify` 而不是任意选择。
4. 删除、覆盖、迁移、外部系统写入、共享状态变更等不可逆或高风险操作应先确认。
5. 一次尝试失败后，应先诊断原因，再决定是否需要用户输入，而不是立即把选择抛回给用户。

### 三、验证结果如实汇报策略

当前系统已引入 `agent_review`，这是很好的基础。建议进一步增强 Prompt 中关于验证状态汇报的约束，明确：

1. 未运行的验证不能暗示为已运行。
2. 失败的验证不能被弱化为“看起来问题不大”。
3. 如果验证由 `agent_review` 完成，应把其结果视为默认事实来源。
4. 如果 helper 已经运行了 typecheck 或测试，主 Agent 不应无意义重复运行同一批命令，除非 helper 结果不完整、不可用，或者用户明确要求复核。
5. 汇报修改结果时，应区分“已修改完成”“已验证通过”“尚未验证”三个层次。

## 主 Agent 与 Helper 的 Prompt 继承策略

当前 `build_helper_system_prompt(parent_system_prompt, helper_profile)` 的策略是把父 Prompt 与 helper profile 直接串接，再补充 helper 输出要求。这种方案简单有效，但长期会让 helper 看到过多与其任务无关的规则。

建议后续将 helper Prompt 调整为“受控继承”，而不是完整继承。可以把父 Prompt 信息分为三类：

1. 必须继承的信息，包括语言、风格、项目工作区约束、沙箱与审批边界。
2. 可选继承的信息，包括与当前 helper 任务相关的工具策略。
3. 不应继承的信息，包括面向最终用户的输出结构约束、与 helper 无关的行为细节，以及只适用于主 Agent 的流程说明。

可以考虑为 helper 也定义单独的 `HelperPromptBuildContext` 和 provider 集合。Explore helper、Review helper 和未来其他 helper 应该有各自更聚焦的 section。例如：

- Explore helper 应重点关注事实收集、路径定位、依赖关系与证据陈述。
- Review helper 应重点关注回归风险、验证命令执行、结论级判断与后续建议。

这样可以减少 helper 输出被主 Prompt 稀释的问题。

## 建议的模块拆分

为了降低 `agent_session.rs` 的复杂度，建议在 `src-tauri/src/core/` 下引入新的 prompt 模块，但初期应保持较粗粒度拆分，避免在结构迁移阶段制造过多文件和 review 成本。

建议 Phase 1 先采用如下结构：

- `prompt/mod.rs`，作为总入口。
- `prompt/context.rs`，定义 `PromptBuildContext` 与辅助类型。
- `prompt/section.rs`，定义 `PromptSection`、`PromptPhase`、provider trait 与排序逻辑。
- `prompt/providers.rs`，集中放置初期的粗粒度 provider 实现。
- `prompt/assembler.rs`，负责 provider 汇总、过滤、排序和最终拼装。

当 provider 数量或依赖复杂度明显增长后，再考虑把 `providers.rs` 继续拆分为 `base.rs`、`workspace.rs`、`environment.rs`、`profile.rs`、`helper.rs` 等更细模块。

## 迁移步骤

建议分四个阶段推进，每个阶段都尽量保持可回滚。

### 阶段一：结构抽离但保持行为不变

这一阶段的目标是只重构结构，不主动修改文案语义。

1. 引入 `PromptBuildContext`、`PromptPhase` 与 `PromptSection`。
2. 将当前 `build_system_prompt` 中已有 section 搬迁到一组粗粒度 provider 中。
3. 保持 section 顺序与当前版本尽量一致。
4. 为一组代表性上下文生成当前 Prompt 输出快照，作为迁移前基线。
5. 用新 assembler 生成 Prompt，并与快照做 diff，确保输出保持等价或仅出现明确可接受的差异。

这一阶段不建议长期维护新旧两套 prompt 实现并通过 feature flag 切换；对于本项目而言，基于代表性上下文的 snapshot/diff 更简单，也更容易控制复杂度。

### 阶段二：补齐回归测试与基础调试能力

这一阶段重点补强工程保障，但仍尽量不改 Prompt 文案策略。

1. 为关键安全规则增加内容断言测试。
2. 为典型上下文补充 snapshot 测试。
3. 为 phase 排序、条件命中和最终长度增加开发态日志。
4. 为 helper Prompt 建立基础继承回归测试。

### 阶段三：引入条件注入与策略增强

这一阶段开始做真正的策略升级。

1. 为 term panel、helper、review、未来 MCP 等能力定义 capability 标识。
2. 让相关 section 根据 capability 条件注入，而不是默认总是存在。
3. 引入更明确的工具替代矩阵。
4. 细化 clarify、update_plan 与风险确认边界。
5. 强化验证结果如实汇报的 Prompt 约束。

这一阶段的目标是减少无关规则，缩短有效 Prompt，并让核心行为约束更稳定。

### 阶段四：重构 Helper Prompt 继承机制

这一阶段重点优化 subagent。

1. 将 `build_helper_system_prompt` 从简单字符串拼接升级为受控继承。
2. 为 explore 和 review helper 提供更专用的 provider。
3. 明确 helper 输出协议，减少 parent 在消费 helper 输出时的不确定性。

## 测试与验证建议

Prompt 重构非常容易发生“文案看似没问题，但行为悄悄改变”的问题，因此测试优先级应首先覆盖关键内容安全回归，再覆盖结构与条件细节。

### 一、关键内容回归测试

这是优先级最高的一组测试，应首先建立。重点验证高风险规则在迁移后仍然存在，例如：

1. `read files before editing` 是否仍保留。
2. 风险动作是否仍要求先提示或先确认。
3. 未执行的验证是否不会被描述为已执行。
4. `agent_review` 的职责边界是否仍然清晰。
5. 默认语言与风格指令是否仍能正确覆盖。

### 二、快照测试

在一组代表性上下文下，对最终 Prompt 输出建立 snapshot，用于保障结构迁移过程中的整体兼容性。例如：

1. default mode 下的典型输出。
2. plan mode 下的典型输出。
3. 有 profile 与无 profile 时的差异。
4. 有 AGENTS.md 与无 AGENTS.md 时的差异。

### 三、条件注入测试

验证 capability 驱动的 section 是否只在启用时出现。例如：

1. 没有 helper 能力时，不应注入 helper 专属策略。
2. 没有 term panel 时，不应加入过多 term panel 专属说明。
3. plan mode 下不应出现允许修改文件的误导性表述。

### 四、Helper Prompt 测试

验证 helper 是否只继承必要信息。例如：

1. 语言和风格是否被继承。
2. 工作区约束是否被继承。
3. 面向最终用户的格式要求是否被过滤。
4. review helper 是否具备明确的验证职责指令。

### 五、结构与排序测试

在完成上述关键测试后，可补充更偏内部实现保障的结构测试，例如：

1. section 是否按 phase 与 phase 内顺序稳定输出。
2. 空 section 是否被正确过滤。
3. provider 返回顺序是否不会影响最终输出顺序。

## 调试与可观测性建议

Prompt 一旦做成 section 化，就应充分利用这一结构提升可观测性。建议在开发环境下提供以下能力：

1. 输出最终 Prompt 的 section 清单，包括 section key、标题和是否命中。
2. 记录哪些 provider 被启用，哪些 provider 因条件不满足被跳过。
3. 在调试日志中输出最终 Prompt 总长度与各 section 长度。
4. 未来在桌面端调试视图中展示本次会话的 effective system prompt。

为避免 Prompt 随 provider 增多而持续膨胀，建议从 Phase 1 起就记录最终 Prompt 总长度和各 section 长度。初期不强制实施 token budget 裁剪，但应先建立长度观测能力，后续再根据模型上下文窗口和真实使用情况决定是否引入更细的 token 预算或 section 级长度控制。

## 风险与注意事项

本次重构虽然主要是工程结构优化，但仍然存在以下风险。

1. 如果 section 顺序变化过大，可能造成模型对某些规则的遵循优先级发生改变。
2. 如果条件注入过于激进，可能让一些原本依赖的行为规则在特定场景中消失。
3. 如果 helper 继承裁剪过度，可能导致子代理缺失必要上下文。
4. 如果测试覆盖不足，可能在未察觉的情况下引入 Prompt 行为回归。
5. 如果在结构迁移阶段同步引入策略增强，容易导致问题来源难以定位。

因此建议优先使用 snapshot 基线和关键内容断言来控制迁移风险，而不是长期维护双轨 Prompt 实现。

## 版本与元数据建议

如果后续需要记录 Prompt 调试日志、行为回归样本或缓存键，建议在 assembler 输出中补充轻量元数据，而不是只返回单个字符串。可选元数据包括：

1. `prompt_version`，用于标识结构方案版本。
2. `prompt_fingerprint`，用于基于最终内容或 section 列表生成哈希。
3. `section_keys`，用于记录本次实际命中的 section 集合。

这些元数据不是 Phase 1 的必需项，但对后续排查“某次行为是由哪版 Prompt 产生的”会很有帮助。

## 建议优先级

如果只做一轮小步快跑式优化，建议优先级如下：

1. 首先完成 `PromptBuildContext`、`PromptPhase`、`PromptSection` 与 assembler 的结构抽离，并用 snapshot 基线保证迁移安全。
2. 然后补齐关键内容回归测试、快照测试和长度观测能力。
3. 接着再引入工具替代矩阵、确认边界和验证汇报等策略增强项。
4. 最后处理 helper Prompt 的受控继承与更细的调试可视化。

## 结论

当前项目的 Prompt 系统已经有良好的工程基础，真正需要提升的不是规则数量，而是结构化程度与运行时可控性。建议将现有 `build_system_prompt` 从“单函数组装大字符串”逐步重构为“基于上下文的 section provider 体系”，并在结构稳定后，再分阶段补强工具矩阵、确认边界、验证汇报与 helper 继承策略。

这样做的收益是明确的。短期内，它可以降低 Prompt 膨胀带来的维护成本和行为漂移风险；中期内，它能让不同能力和运行模式拥有更清晰的 Prompt 策略边界；长期看，它会为未来的插件、MCP、更多 helper 与调试能力提供稳定的扩展基座。
