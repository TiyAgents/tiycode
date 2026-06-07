# Goal 评估与续行重构方案：引入 Judge 验收 Agent

> 状态：设计方案（待评审）
> 关联模块：`src-tauri/src/core/goal_manager.rs`、`src-tauri/src/core/subagent/`、`src-tauri/src/core/agent_run_event_handler.rs`、`src-tauri/src/model/goal.rs`
> 决策基线（已澄清）：
> 1. **保留全部现有护栏**（idle 空转、clarify/update_plan 暂停、token/turn 预算上限），仅把“是否完成”的判定从自主声明改为 Judge 验收。
> 2. **复用 `GoalStatus::Complete` 状态** 表达“通过验收”，并在 `goals` 表新增 Judge 评估字段持久化最近一次裁决；迁移需把存量 `status='complete'` goal 回填为 `judge_passed=1`。
> 3. **由主 agent 主动调用 `agent_judge`**，系统在 run 终止后通过续行 prompt 引导主 agent 先验收、未通过则修复后重验。
> 4. **`agent_judge` 是主 agent 专属工具**：只在有未完成 goal 时注入主 agent，且运行时必须硬性拒绝任何 subagent 递归调用 Judge，即使工具名被 `RuntimeOrchestrationTool::parse()` 解析出来也不能放行。
> 5. **Judge 使用诊断型 shell 软约束**：Judge 的文件工具保持只读；允许 `shell` 仅用于测试、type-check、lint、只读检查等诊断验证，并通过 Judge prompt 明确禁止用 shell 修改文件、删除数据、安装依赖或改变全局状态。首版不新增受限 shell 沙箱。
> 6. **Judge 默认使用 primary 模型角色**，优先保证验收质量；首版不把 Judge/subagent 的 token 单独计入 goal token budget，也不新增 Judge 专属硬超时，沿用现有 helper run 的 turn/取消机制。
> 7. **删除失效的自主完成路径**：移除 `goal_scored`、`GoalVerdict::Complete` 的旧自证语义，以及由 `goal_scored` 空 evidence 触发的 `NoEvidence` / `MISSING_EVIDENCE_PROMPT` 分支。

---

## 1. 背景与问题

当前 goal 的"完成"判定依赖主 agent 自主调用 `goal_scored(status, evidence, pledge)` 工具来声明达成。这是一种**自证式（self-attestation）**设计：

- 工具内部只校验 `status == "complete"`、`pledge` 文本逐字匹配、`evidence` 非空（见 `agent_session_execution.rs` 的 `execute_goal_tool()`）。
- 它**无法验证 evidence 的真伪**，也无法核对结果是否真的满足 goal 的一致性与完整性。

实测发现部分模型即便明知仍有未完成项，也会照抄 pledge 文本、编造 evidence 来调用 `goal_scored` 并提前结束任务。pledge + evidence 非空这类形式化护栏对"不诚实声明"无效，这是自主声明方式的**设计缺陷**。

**核心思路**：把"完成判定权"从被评估者（主 agent）手中移交给独立的评估者（Judge Agent）。主 agent 不能再自己宣布通过；只有 Judge 基于 goal 内容对项目当前状态做出"通过"裁决，goal 记录才会扭转为通过验收状态。续行监督也随之改为以"是否通过验收"为准。

---

## 2. 现状梳理（已确认事实）

### 2.1 Goal 数据模型与持久化

- `GoalStatus`（`src-tauri/src/model/goal.rs`）：`Active` / `Paused` / `BudgetLimited` / `Complete` 四态。
- `goals` 表（`migrations/20260530000000_goals.sql` 及后续迁移）：每 `thread_id` 唯一一条 goal；含 `status`、`evidence`、`tokens_used`、`turns_used`、`max_turns`、`pause_reason`、`last_evaluated_run_id` 等列。
- `GoalManager`（`src-tauri/src/core/goal_manager.rs`）封装 CRUD + 评估 + prompt 生成。关键方法：`mark_complete(goal_id, evidence)`、`evaluate_after_turn(response, goal) -> GoalVerdict`（同步 CPU 启发式）、`evaluate_after_run(run_id, response) -> GoalEvaluationOutcome`（异步、含去重 CAS）。

### 2.2 `goal_scored` 工具链路

- 工具定义在 `agent_session_tools.rs` 的 `runtime_tools_for_profile()`，常量 `GOAL_SCORED_TOOL_NAME` / `GOAL_SCORED_PLEDGE` 在 `goal_manager.rs`。
- 调用分派在 `agent_session_execution.rs::execute_tool_call()` → `execute_goal_tool()`：校验 status/pledge/evidence → `mark_complete()` → 发送 `GoalCompleted` + `GoalStateUpdated` 事件。

### 2.3 续行监督逻辑

- run 终止后，`agent_run_event_handler.rs::maybe_continue_goal_after_terminal_run()` 是入口。
- 前置条件：`goal_continuation_enabled == true`、`final_status ∈ {Completed, Interrupted}`。
- 调用 `evaluate_after_run()` 内部走 `evaluate_after_turn()` 分层启发式：
  - **Layer 1** 工具阻塞：`clarify` → `Paused(ClarifyPending)`；`update_plan` → `Paused(PlanPending)`；`goal_scored` 放行。
  - **Layer 2** idle/完成声明：连续 idle ≥ `MAX_IDLE_TURNS(3)` → `Paused(IdleBlocked)`；检测到完成关键词但未调工具 → `ChallengeEvidence`（反复声称达上限 → `IdleBlocked`）。
  - **Layer 3** 预算：tokens 超 budget → `BudgetLimited`；turns 超 `max_turns` → `Paused(BudgetExhausted)`。
  - 默认 → `Continue`。
- verdict 为 `Continue` / `ChallengeEvidence` 时，用 continuation prompt 启动新 run；`Paused` / `BudgetLimited` / `skipped` 时不续行。
- **关键现状**：续行从不查询 goal 的 `Complete` 状态。它实际依靠"模型没有再触发任何阻塞/完成信号 + goal 仍 `Active`"间接推断。一旦 `goal_scored` 被调用，`mark_complete()` 把 status 写成 `Complete`，下一轮 `evaluate_after_run()` 因 goal 非 `Active` 返回 `skipped`，从而停止续行。

### 2.4 Subagent 机制

- 内建 subagent：`Explore`、`Review`、`Parallel`，定义在 `subagent/runtime_orchestration.rs` 的 `RuntimeOrchestrationTool` / `SubagentProfile`。
- 深度模型：主 agent = depth 1；主 agent 直接子代理 = depth 2（`MAIN_AGENT_CHILD_DEPTH`）；`GLOBAL_MAX_DELEGATION_DEPTH = 5`；内建默认 `BUILTIN_DEFAULT_MAX_DELEGATION_DEPTH = 3`。
- 委派校验：`orchestrator.rs::validate_delegation_capability(caller, target_tool, target_profile, child_depth)`，三重检查（调用方 `can_delegate`、全局上限、目标 `max_delegation_depth`）。
- 权限模型：`Explore` 只读（read/list/find/search/web_search，`can_delegate=false`）；`Review` 只读 + 诊断 shell + git/term 只读（`can_delegate=true`）；`Custom` 按 `allowed_tools` 白名单。
- 工具注入：主 agent 在 `agent_session_tools.rs::runtime_tools_for_profile()` 中 `tools.extend(runtime_orchestration_tools())`；自定义在 `agent_session.rs::build_session_spec()` 注入。
- Prompt 注入：`build_helper_system_prompt()` 按 `PromptSurface`（`prompt/surface.rs`）选择 section；task 通过 `agent.prompt(request.task)` 注入为 user message。

---

## 3. 设计目标

1. 新增内建 **Judge** subagent，对项目当前状态做 goal 达成度评估，结构化返回：通过与否（bool）、完整度百分比、判定依据（未达成/不符合点描述）。
2. Judge 通过时**扭转 goal 记录为通过验收状态**（复用 `Complete` + 持久化 Judge 字段）。
3. Judge 上下文注入 goal 内容，评估重点是 goal 要求的**一致性**与**完整性**。
4. Judge 文件工具保持**只读**，允许 `read` / `list` / `find` / `search` / `web_search`；允许 `shell` 但仅作为诊断型软约束工具用于测试、type-check、lint、只读检查；允许再发起 subagent（含并行，如 explore/review 协助），**自身最大被委派深度为 2**。
5. **删除 `goal_scored` 工具**。完成判定不再由主 agent 自证。
6. 续行监督改为：判定 goal 记录是否“通过验收”；未通过且 goal 仍 Active 则续行，并在 continuation prompt 中明确要求主 agent 调用 `agent_judge` 验收并遵循验收结果。
7. **按需注入**：仅当 thread 有未通过验收的 goal 时，才向**主 agent**注入 `agent_judge` 工具；所有 subagent 均不注入且运行时拒绝递归调用 `agent_judge`；无 goal 或已验收通过时不注入。

---

## 4. 总体设计

### 4.1 角色与职责重划

| 角色 | 重构前 | 重构后 |
|------|--------|--------|
| 主 agent | 自己调 `goal_scored` 声明完成 | 干活 + 自认为完成后调 `agent_judge` 申请验收；不能自证完成 |
| Judge agent | 不存在 | 独立验收者，文件工具只读且 shell 仅诊断软约束，基于 goal 评估项目当前状态，产出结构化裁决；通过则扭转 goal 状态 |
| 续行监督 | 间接依赖 goal 非 Active 停续行 | 显式以"goal 是否通过验收（Complete + judge_passed）"为停续行依据 |

### 4.2 端到端数据流

```
用户 /goal <objective>
  └─ goal_set() → create_goal(status=Active)
     └─ 注入 ActiveGoalSource 到主 agent system prompt（更新文案：完成须经 agent_judge 验收）
     └─ 按需向主 agent 注入 agent_judge 工具（goal 存在且尚未通过验收）

主 agent run：工作 → 自认为达成 → 调用 agent_judge(task)
  └─ execute_tool_call() 路由到 Judge 编排
     └─ HelperAgentOrchestrator::run_helper(SubagentProfile::Judge)
        ├─ build_helper_system_prompt(PromptSurface::SubagentJudge) + 注入 goal objective 到上下文
        ├─ Judge 工具集：read/list/find/search/web_search/shell(仅诊断软约束) + （depth 允许时）agent_explore/agent_review/agent_parallel
        ├─ Judge 调研验证：读代码、搜索、运行测试/type-check/lint 等诊断命令、并行 explore/review
        └─ 产出结构化 JudgeReport { passed, completeness_pct, findings, summary }
     └─ Judge 编排回写 goal 记录：
        ├─ 总是：persist 最近一次 judge_passed / judge_completeness / judge_findings / judge_summary / judge_evaluated_run_id
        └─ passed == true：事务写入 status=Complete + judge_passed=true + evidence=summary
                            发送 GoalCompleted + GoalStateUpdated 事件
     └─ agent_judge 工具结果（JudgeReport 文本）返回给主 agent

run 终止
  └─ maybe_continue_goal_after_terminal_run()
     └─ evaluate_after_run()
        ├─ 若 goal.status == Complete && goal.judge_passed == true（已通过验收）→ skipped（停续行）✅
        ├─ 若 goal.status != Active → skipped（非活跃 goal 不自动续行，保留现有暂停/预算语义）
        ├─ 否则保留现有护栏：clarify/update_plan/idle/预算 → Paused/BudgetLimited
        └─ 否则 → Continue：注入新版 continuation prompt
                   "你尚未通过验收。请先用 agent_judge 验收；若上次验收未通过，
                    按 findings 修复后再次调用 agent_judge。"
     └─ Continue → 启动新 run（回到主 agent run）
```

### 4.3 为什么选择这套方案（与备选对比）

- **复用 `Complete` 而非新增 `Verified` 枚举**：`Complete` 在 DDL CHECK 约束、`GoalStatus` 枚举、前端状态条、gateway 文案中均已铺开。新增枚举值需要同步迁移、前端、序列化多处，收益有限。改为复用 `Complete` 并以 `judge_passed` 布尔列区分"是否经 Judge 验收"，改动面最小且语义清晰（通过验收 = `Complete` 且 `judge_passed=true`）。
- **保留全部护栏**：Judge 解决的是"完成判定的可信度"，而 idle 空转、clarify/update_plan 暂停、预算上限解决的是"防止无限续行/资源失控/阻塞等待"。两者正交，移除护栏会让无 goal 评估能力时的兜底消失，引入失控风险。
- **主 agent 主动调用 + 续行引导**（而非系统自动发起 Judge）：保持与现有 subagent 调用模型一致（主 agent 通过工具调用委派），实现侵入小；系统侧只需在续行 prompt 中“催”主 agent 去验收，无需在 run 终止后再隐式拉起一个评估 run 改变运行时调度。续行 prompt 会持续施压，直到 goal 被 Judge 标记通过，规避了“主 agent 不调 Judge 就永远不验收”的死角。
- **Judge 作为主 agent 专属内建工具**：虽然 `agent_judge` 会加入 `RuntimeOrchestrationTool::parse()`，但它不进入 `builtin_all()` 和 `delegation_tools_for_helper()`，也不允许 subagent 递归调用。这样保留统一工具解析与 helper 编排复用，同时避免 explore/review/custom/Judge 自己绕过“主 agent 申请验收”的职责边界。
- **诊断型 shell 软约束而非新沙箱**：Judge 需要能运行测试、type-check、lint 等验证命令，因此首版复用现有 `shell` 工具；但该工具能力本身不是硬只读，必须在 Judge prompt 中明确限制为诊断用途，禁止修改文件、删除数据、安装依赖、启动交互式长进程或改变全局状态。新建受限 shell/test-runner 工具会扩大改动面，首版暂不引入。
- **Judge 使用 primary 模型角色**：验收质量优先于成本，Judge 默认走 `model_plan.primary`。Explore/Review 继续保持现有模型策略，Judge 内部再委派时由各子代理自己的模型映射决定。

### 4.4 首版范围边界

首版目标是打通后端 Judge 验收闭环：工具注入、subagent 运行、结构化解析、goal 回写、续行停止、迁移兼容和测试覆盖。前端仅同步类型并在现有状态条显示“已验收通过”这一最小信息；`judge_completeness` 的精细 UI、额外事件、ACP/gateway 的详细状态展示、Judge token 单独计入 goal budget、Judge 专属超时或受限 shell 沙箱均作为后续增强，不进入首版。

---

## 5. 详细实现

### 5.1 Judge subagent profile（`subagent/runtime_orchestration.rs`）

- `RuntimeOrchestrationTool` 新增变体 `Judge`，工具名映射 `agent_judge`；`parse("agent_judge") -> Some(Judge)`。同时补齐 `tool_name()`、`title()`、`description()`、`profile()`、`as_agent_tool()` 的 match 分支，`as_agent_tool()` 的 schema 只需要 `task: string`。
- `SubagentProfile` 新增 `Judge` 变体，并补齐 `helper_kind()`（固定返回 `helper_judge`）、`system_prompt()`、`can_delegate()`、`max_delegation_depth()`、`helper_tools()` 等 match 分支。
- `resolve_helper_profile()` 增加 `RuntimeOrchestrationTool::Judge => Some(SubagentProfile::Judge)`；`resolve_helper_model_role()` 增加 Judge 分支，默认使用 `model_plan.primary`，不要复用 Explore/Review 的 auxiliary 映射。
- `helper_tools()` for `Judge`：`read` / `list` / `find` / `search` / `web_search`（条件启用）/ `shell`（仅诊断验证）。**不含** `edit` / `write` / `term_write` / `term_restart` / `term_close`。需要在工具描述和 Judge prompt 中明确：`shell` 只能运行测试、type-check、lint、只读检查等诊断命令，不能修改文件、删除数据、安装依赖、启动交互式长进程或改变全局状态。这是 prompt 软约束，不是硬沙箱。
- `can_delegate()` for `Judge`：`true`（允许 explore/review/parallel 协助）。
- `max_delegation_depth()` for `Judge`：`2`（即 Judge **自身最大被委派深度为 2**——主 agent depth 1 直接委派 Judge 得到 depth 2，符合 `MAIN_AGENT_CHILD_DEPTH=2`；同时这意味着 Judge 内部委派的子级会是 depth 3，需在 `delegation_tools_for_helper()` 中据此过滤）。
  > 注意：需求所述“自身最大被委派深度为2”指 Judge 作为被委派目标时允许出现在 depth ≤ 2。为了让 Judge 仍能发起 explore/review/parallel（depth 3 子级），`delegation_tools_for_helper(child_depth)` 对内建目标的过滤阈值需复核：Judge 在 depth 2 调用子级时 `child_depth=3`，仍 ≤ `GLOBAL_MAX_DELEGATION_DEPTH(5)` 且 ≤ explore/review 的 `max_delegation_depth(3)`，故可注入。实现时确保 `validate_delegation_capability` 对 Judge→explore/review 放行。
- `delegation_tools_for_helper()` 仍只注入 Explore / Review / Custom / Parallel，**不得注入 Judge**。这使 Judge 可以委派其他 helper，但任何 helper 不能委派 Judge。
- `RESERVED_SUBAGENT_SLUGS` 增加 `"judge"`，防止自定义 subagent 占用该 slug。由于 `RuntimeOrchestrationTool::parse()` 对 `agent_{slug}` 有通配解析，保留 slug 能避免 `agent_judge` 与自定义工具名冲突。
- `runtime_orchestration_tools()` **不无条件包含 Judge**：Judge 改为按需注入（见 5.6），`builtin_all()` 保持仅含 explore/review/parallel，Judge 单独由主 agent 工具组装处按 goal 条件 push。

### 5.2 Judge 结构化协议（新增 `subagent/judge_contract.rs`）

参照 `review_contract.rs` / `parallel_contract.rs` 模式新增：

```rust
/// agent_judge 工具的入参（主 agent 传入）。
pub struct JudgeRequest {
    pub task: String,           // 主 agent 对"为何认为达成"的说明 / 关注点
}

/// Judge 评估结构化产出。
#[derive(Serialize, Deserialize)]
pub struct JudgeReport {
    pub passed: bool,                 // 是否通过验收
    pub completeness_pct: u8,         // 0-100 完整度百分比
    pub findings: Vec<String>,        // 未达成 / 不符合 goal 的具体点（passed=false 时必填）
    pub summary: String,              // 判定依据总述，作为通过时的 evidence
}
```

- Judge 的 system prompt（模板 `prompt/templates/subagent/judge.md`）强制要求最终以可解析的结构化形式（JSON 块或约定字段）返回上述四项。
- `passed=true` 时 `summary` 必须非空，作为 `mark_complete()` 的 evidence；如果 Judge 输出 `passed=true` 但 `summary` 为空，解析层必须降级为 `passed=false`，避免无证据完成。
- `completeness_pct` 解析后必须 clamp 到 0-100；`passed=false` 时 `findings` 必须非空，若模型未给出 findings，则把原始输出或“Judge did not provide actionable findings”写入 findings。
- Judge 编排在拿到 Judge 文本输出后解析为 `JudgeReport`；解析失败按 `passed=false` 处理并把原始文本塞入 `findings`，避免误判通过。

### 5.3 Judge prompt surface 与上下文注入

- `prompt/surface.rs::PromptSurface` 新增 `SubagentJudge { inherited_run_mode }`。
- `SurfacePattern::matches()` 同步更新：`AnySubagent` 必须匹配 `SubagentJudge`；`BuiltinSubagent` 也必须匹配 `SubagentJudge`，因为 Judge 是内建 subagent。若某些 prompt section 只应给 Explore/Review 而不应给 Judge，应改用更精确的 matcher 或新增 pattern，避免误注入。
- `build_helper_system_prompt()` 增加 `SubagentProfile::Judge` → `PromptSurface::SubagentJudge { inherited_run_mode }` 映射。
- `prompt/sources/custom_subagent_body.rs` 增加 Judge 模板映射：Judge → `templates/subagent/judge.md`。
- `prompt/templates/subagent/judge.md`：定义 Judge 角色——独立验收员，只读评估，重点核对 goal 的一致性与完整性；说明可用工具（含诊断型 `shell`、可委派 explore/review/parallel）；要求输出结构化 `JudgeReport`；明确禁止修改文件。`shell` 约束必须写成硬性行为指令：只能运行测试、type-check、lint、只读检查；不得通过 shell 编辑/删除文件、安装依赖、改变全局状态、启动交互式或长期驻留进程。
- `prompt/sources/subagent_output_contract.rs` 增加 Judge 的输出契约 `output_contract.judge.md`，并在 contract 中重复 `passed` / `completeness_pct` / `findings` / `summary` 的字段要求和失败兜底规则。
- **goal 内容注入采用 task 前缀方案**：Judge 上下文必须包含 goal objective，且由 `agent_session_execution.rs` 的 Judge 分支在构造 helper task 时注入，不新增 DB 读取型 prompt source。
- task 前缀必须包含：objective、当前 goal id/status、最近一次 Judge findings/summary（若有）、主 agent 传入的 `task` 说明。这样 Judge 不依赖主 agent 自述即可核对目标。

### 5.4 Judge 编排与 goal 回写（`agent_session_execution.rs` + `goal_manager.rs`）

- `execute_tool_call()`：`RuntimeOrchestrationTool::parse()` 命中 `Judge` 时进入 Judge 专用分支，不直接走普通 `execute_helper_tool()` 返回路径。该分支可复用 `resolve_helper_delegate()` / `HelperAgentOrchestrator::run_helper()`，但必须在 helper 完成后追加 JudgeReport 解析和 goal 回写。
- Judge 分支额外步骤：
  1. 调用前从 DB 加载当前 thread 的未完成 goal；无 goal 或 goal 已 `Complete && judge_passed=true` 则返回错误（agent_judge 仅在有 goal 时可用，理论上不会被注入）。
  2. 把 `goal.objective`、goal id/status、最近一次 judge findings/summary、主 agent 传入的 `task` 拼成 Judge task 上下文。
  3. 以 `SubagentProfile::Judge`、`RuntimeOrchestrationTool::Judge`、depth 2 启动 helper run；模型角色使用 `model_plan.primary`。
  4. Judge run 结束后解析 `JudgeReport`；解析失败或字段非法按 `passed=false` 处理。
  5. 调用新增 `GoalManager::record_judge_verdict(goal_id, run_id, &report)` 持久化最近裁决；若 `report.passed`，该方法在同一事务内写入 `status=complete`、`evidence=report.summary` 与 `judge_passed=true`。
  6. 若通过验收，发送 `GoalCompleted` + `GoalStateUpdated` 事件；若未通过，也发送 `GoalStateUpdated`，让前端/后续续行能拿到最新 findings。
  7. 把 `JudgeReport` 文本作为工具结果返回主 agent；通过时结果中明确提示“goal 已通过验收，请停止修改并总结”，降低同一 run 后续继续改动的风险。
- `GoalManager` 新增方法：
  - `record_judge_verdict(&self, goal_id: &str, run_id: &str, report: &JudgeReport) -> Result<GoalRecord>`：写 `judge_passed` / `judge_completeness` / `judge_findings`(JSON) / `judge_summary` / `judge_evaluated_run_id`，并返回更新后的 record 供事件 payload 使用；passed 时同一事务同步写 `status=complete` 与 `evidence=report.summary`。
- 原子性要求：`goal_repo.rs` 增加 `record_judge_verdict()` repo 方法，在事务内更新 judge_* 字段；passed 时同事务写 `status='complete'` 与 `evidence=summary`，确保 `status=complete` 与 `judge_passed=1` 不出现半更新；未通过时保持原 status（通常 Active）不变。
- 预算边界：首版 Judge helper run 的 token 不单独计入 goal `tokens_used`。这是明确取舍；后续若要计入，需要扩展 `HelperRunResult` 携带 usage 并在 Judge 分支回写。
- 同轮继续修改边界：系统不强行锁定 goal 后的写工具，因为主 agent 仍处于同一 run；通过验收后的工具结果和 `active_goal.tpl.md` prompt 必须要求停止修改。若未来需要硬约束，可在 `execute_tool_call()` 中对 `Complete && judge_passed` 后的 mutating tools 增加拒绝策略，首版不做。

### 5.5 删除 `goal_scored` 工具

- 删除工具定义（`agent_session_tools.rs` 中的 `goal_scored` `AgentTool::new(...)`）。
- 删除分派分支与 `execute_goal_tool()`（`agent_session_execution.rs`）。
- 移除常量 `GOAL_SCORED_TOOL_NAME` / `GOAL_SCORED_PLEDGE`（`goal_manager.rs`），以及 `evaluate_after_turn()` 中 `detect_tool_based_blocking` 对 `goal_scored` 的放行分支。
- 删除旧自证语义：`GoalVerdict::Complete { evidence }` 当前没有有效生产者，删除 `goal_scored` 后一并移除，并删除 `evaluate_after_run()` 中的旧 match 分支，减少死代码。
- 删除 `ChallengePromptVariant::NoEvidence` 与 `MISSING_EVIDENCE_PROMPT`，因为它们只服务于“调用 `goal_scored` 但 evidence 为空”的旧路径；保留 completion-claim 检测对应的 `ChallengeEvidence` / `NoTool` 语义，并把文案改为“声称完成但尚未调用 `agent_judge` 验收”。
- 护栏保留但需改写文案：`ChallengeEvidence` 与 completion-claim 检测仍作为“提醒主 agent 去验收”的软提示，引导语从“调用 goal_scored”改为“调用 agent_judge 验收”。`GUIDANCE_PROMPT` 同步更新。
- `agent_judge` 会被 `record_tool_call()` 记录到 goal runtime tool calls；`detect_tool_based_blocking()` 不应把它视为阻塞工具，也不应触发 pause。它与普通工具调用一样表示 agent 有行动，能重置 idle 倾向。
- 全局检索并清理 `goal_scored` 引用：系统 prompt、`active_goal.tpl.md`、gateway 文案、前端 hardcoded kickoff prompt、测试（`tests/goal_lifecycle.rs`）等。

### 5.6 按需注入 `agent_judge`（仅主 agent，仅有未完成 goal 时）

- 注入点在主 agent 工具组装处。`runtime_tools_for_profile()` 当前是纯 profile 函数，不知道 thread goal 状态；推荐在其调用方 `build_session_spec()`（`agent_session.rs`）查询并追加 Judge 工具，避免把 DB 依赖塞进纯工具构造函数。
  - 在 `build_session_spec()` 已能访问 `pool` 与 `thread_id`，查询 `goal_repo::find_by_thread_id`，若存在且尚未通过验收，则 push `RuntimeOrchestrationTool::Judge.as_agent_tool()`。
  - “尚未通过验收”的判定为：goal 存在且不是 `status == Complete && judge_passed == true`。实际自动续行仍只对 `Active` 生效；但工具注入可允许用户在恢复/继续场景中对 `Paused` 或 `BudgetLimited` goal 重新申请验收。
  - goal 不存在或已 `Complete && judge_passed`（已验收）则不注入。
- `runtime_tools_with_custom_subagents()` 与 extension tool 合并时需维持内建工具名优先级，防止 extension/custom 工具覆盖 `agent_judge`。
- **subagent 不注入**：Judge 工具只在主 agent 工具集 push，不进入 `delegation_tools_for_helper()` 的候选；任何 subagent（含 Judge 自身、explore/review/custom）的可委派目标列表都不包含 `agent_judge`。
- **运行时硬门禁**：仅“不注入”不足够，因为模型或测试仍可能构造 `agent_judge` 调用，且 `RuntimeOrchestrationTool::parse()` 会命中。必须在 subagent 递归委派路径（例如 `HelperDelegationContext::handle_delegation()` / `resolve_delegation()`）中显式拒绝 `RuntimeOrchestrationTool::Judge`，返回“agent_judge can only be called by the main agent for the current goal”之类错误。
- `agent_parallel` 的任务列表也必须拒绝 `agent_judge`。`validate_parallel_delegate_safety()` 或解析 parallel task 的位置应把 Judge 视为非法 batch target，避免通过 parallel 间接调用 Judge。
- 主 agent 侧 `execute_tool_call()` 的 Judge 分支也要重新查询 goal 状态，不能只依赖工具注入时的状态；这是防止 race / stale tool set 的后端 backstop。

### 5.7 续行监督改造（`agent_run_event_handler.rs` + `goal_manager.rs`）

- `evaluate_after_run()` / `evaluate_after_turn()` 开头新增**显式终止判定**：若 goal 已“通过验收”（`status == Complete && judge_passed == true`）→ 返回 `skipped`（停续行）。这是停续行的**主依据**。
- 存量兼容依赖迁移回填：迁移后不应出现旧路径产生的 `status=Complete && judge_passed=false`。如果运行时遇到该组合，按异常兼容处理并停续行或记录 warning；不要把旧 complete goal 重新拉起续行。
- 对 `Paused` / `BudgetLimited` 仍按现有语义返回 skipped，不自动续行。只有 `Active` goal 会继续进入护栏评估。
- 其余护栏（clarify/update_plan/idle/预算）保留，作用不变。
- `Continue` / `ChallengeEvidence` verdict 的 continuation prompt 改写为新模板（替换 `CONTINUATION_PROMPT_TEMPLATE`）：

```
[Goal continuation — turns {turns_used}/{max_turns}]

**Objective:** {objective}

继续推进该目标，执行下一个具体步骤。

⚠️ 完成判定已改为独立验收：当你认为目标已达成时，必须调用
  agent_judge(task="说明为何认为已达成 / 需重点核对的点")
由 Judge 评估项目是否满足目标的一致性与完整性。
- 仅当 Judge 裁决 passed=true 时，目标才会被标记为通过验收并停止续行。
- 若上一次 Judge 验收未通过，请阅读其 findings，逐项修复后再次调用 agent_judge。
你无法自行声明完成；只有通过 Judge 验收才算达成。

如果你被阻塞、需要用户输入，请使用 clarify 工具。
```

- 若最近一次 Judge 未通过，必须把 `judge_findings` 摘要拼接进 continuation prompt，提升修复指向性；摘要可限制长度，避免 prompt 过长。

### 5.8 数据库迁移

新增迁移 `migrations/2026XXXXXXXXXX_goal_judge_fields.sql`：

```sql
ALTER TABLE goals ADD COLUMN judge_passed INTEGER NOT NULL DEFAULT 0;       -- bool
ALTER TABLE goals ADD COLUMN judge_completeness INTEGER;                    -- 0-100, nullable
ALTER TABLE goals ADD COLUMN judge_findings TEXT;                           -- JSON array, nullable
ALTER TABLE goals ADD COLUMN judge_summary TEXT;                            -- nullable
ALTER TABLE goals ADD COLUMN judge_evaluated_run_id TEXT;                   -- nullable

-- 兼容旧版本 goal_scored 已完成的 goal，避免升级后被误判为未验收。
UPDATE goals
SET judge_passed = 1,
    judge_summary = COALESCE(judge_summary, evidence),
    judge_completeness = COALESCE(judge_completeness, 100)
WHERE status = 'complete';
```

- `GoalRecord` / `GoalDto` / `GoalPayload`（`model/goal.rs`）同步新增字段：`judge_passed: bool`、`judge_completeness: Option<i64>`（DB 读写时校验 0-100）、`judge_findings: Option<String>`（JSON 文本，DTO 透传字符串，前端按 string/null 接收）、`judge_summary: Option<String>`、`judge_evaluated_run_id: Option<String>`。
- `goal_repo.rs` 同步更新 `SELECT_COLUMNS`、`GoalRow`、`into_record()`、`insert()`。新增 `record_judge_verdict()` repo 方法，负责写 judge_* 字段；passed 时同一事务同步写 `status='complete'` 与 `evidence=summary`。
- 若 `judge_findings` 以 JSON array 字符串存储，写入前由 `serde_json::to_string(&report.findings)` 生成；读取失败时不要 panic，DTO 可原样返回或置为 `None` 并记录 warning。

### 5.9 前端、IPC、gateway 与 ACP

- `ThreadStreamEvent` 首版复用现有 `GoalCompleted` / `GoalStateUpdated`，不新增 Judge 专属事件。`GoalPayload` 增加 judge 字段后，现有事件 payload 即可携带最新裁决。
- 前端 `GoalPayload` 类型（如 `src/services/bridge/agent-commands.ts`）与 store 类型（如 `src/modules/workbench-shell/model/thread-store.ts`）补充 judge 字段；状态条在 `Complete && judgePassed` 时显示“已验收通过”。`judge_completeness` 的进度/百分比 UI 为二阶段增强。
- `goal-status-bar.tsx` 只做最小展示；若未实现详细展示，也必须保证新增字段不会破坏类型检查。
- gateway / ACP 首版只要求文案与行为不再引用 `goal_scored`，并确保这些入口启动主 agent 时使用同一 `build_session_spec()` 注入逻辑，因此有未完成 goal 时也能拿到 `agent_judge`。详细展示 Judge findings/completeness 可后续增强。

---

## 6. 影响文件清单

| 文件 | 改动 |
|------|------|
| `src-tauri/src/model/goal.rs` | `GoalRecord`/`GoalDto`/`GoalPayload` 新增 judge_* 字段；删除 `GoalVerdict::Complete` 旧自证变体 |
| `src-tauri/src/core/goal_manager.rs` | 删除 `GOAL_SCORED_*` 常量与放行分支；删除 `MISSING_EVIDENCE_PROMPT` / `NoEvidence` 旧路径；新增 `record_judge_verdict()`；续行终止判定改为 `Complete && judge_passed`；改写 continuation/guidance 文案并拼接最近 findings |
| `src-tauri/src/core/subagent/runtime_orchestration.rs` | `RuntimeOrchestrationTool::Judge` + `SubagentProfile::Judge`（工具集/can_delegate/max_delegation_depth=2）；`parse`/`profile`/`as_agent_tool`/`helper_kind` 等 match 补齐；保留 slug；`builtin_all()` 不含 Judge |
| `src-tauri/src/core/subagent/judge_contract.rs`（新增） | `JudgeRequest` / `JudgeReport` 结构化协议、JSON 解析、字段校验、失败兜底 |
| `src-tauri/src/core/subagent/orchestrator.rs` | `build_helper_system_prompt()` 支持 Judge surface；subagent 递归委派路径硬性拒绝 `agent_judge`；保持 Judge→explore/review/parallel 放行 |
| `src-tauri/src/core/subagent/parallel_contract.rs` / 相关 parallel 校验 | `agent_parallel` task 拒绝 `agent_judge` 作为子任务 |
| `src-tauri/src/core/agent_session_execution.rs` | 删除 `goal_scored` 分派与 `execute_goal_tool()`；新增 Judge 专用分支（加载 goal → task 前缀注入 → helper run → 解析 JudgeReport → 回写 goal → 发送事件） |
| `src-tauri/src/core/agent_session_tools.rs` | 删除 `goal_scored` 工具定义；保持基础 runtime tools 不含 Judge；如新增 helper 函数则提供 `agent_judge` 工具构造 |
| `src-tauri/src/core/agent_session.rs` | `build_session_spec()` 查询 goal，按“未通过验收”条件向主 agent 追加 `agent_judge`；`resolve_helper_model_role()` 将 Judge 映射到 primary |
| `src-tauri/src/core/prompt/surface.rs` | `PromptSurface::SubagentJudge`；`SurfacePattern::AnySubagent` / `BuiltinSubagent` 匹配 Judge |
| `src-tauri/src/core/prompt/sources/custom_subagent_body.rs` | Judge → `templates/subagent/judge.md` |
| `src-tauri/src/core/prompt/sources/subagent_output_contract.rs` | Judge 输出契约 |
| `src-tauri/src/core/prompt/templates/subagent/judge.md`（新增） | Judge 角色、诊断型 shell 软约束、委派说明与结构化输出要求 |
| `src-tauri/src/core/prompt/templates/active_goal.tpl.md` | 完成判定改为经 agent_judge 验收，并提示通过后停止修改 |
| `src-tauri/src/core/prompt/sources/active_goal.rs` | 文案同步（如有引用） |
| `src-tauri/src/persistence/repo/goal_repo.rs` | judge_* 列读写；新增 `record_judge_verdict()`；passed 时原子写 status/evidence/judge_* |
| `src-tauri/migrations/2026XXXXXXXXXX_goal_judge_fields.sql`（新增） | judge_* 列迁移，并回填旧 `status='complete'` 为 `judge_passed=1` |
| `src-tauri/src/gateway/gateway_runner.rs` | 移除 `goal_scored` 引导文案，改为 agent_judge 验收说明 |
| `src-tauri/src/acp/**`（如有 goal 文案/事件映射） | 确认不引用 `goal_scored`；复用 GoalStateUpdated payload 的 judge 字段 |
| `src-tauri/tests/goal_lifecycle.rs` | 重写：覆盖 Judge 通过→Complete+judge_passed→停续行；未通过→续行；旧 complete 回填兼容 |
| `src-tauri/src/core/agent_session_tests.rs` / subagent tests | 覆盖 Judge profile、模型角色、工具注入、递归拒绝、parallel 拒绝、prompt surface 匹配 |
| `src/services/bridge/agent-commands.ts` | 前端 `GoalPayload` 类型新增 judge 字段 |
| `src/modules/workbench-shell/model/thread-store.ts` | `GoalStoreState` 新增 judge 字段 |
| `src/modules/workbench-shell/ui/goal-status-bar.tsx` | 最小展示 `Complete && judgePassed` 为“已验收通过” |
| `src/modules/workbench-shell/ui/runtime-thread-surface.tsx` | 清理 goal kickoff prompt 中的 `goal_scored` 示例，改为 agent_judge 验收说明 |

---

## 7. 验证计划

- **Rust 格式**：`cargo fmt --check --manifest-path src-tauri/Cargo.toml`。
- **Rust 行为**：`cargo test --locked --manifest-path src-tauri/Cargo.toml`，重点 `goal_lifecycle`、subagent 委派、prompt surface 与迁移相关测试。新增/重写用例：
  - Judge `passed=true` → goal 变 `Complete` 且 `judge_passed=true`，`judge_summary/evidence` 非空，下一轮 `evaluate_after_run` 返回 skipped（停续行）。
  - Judge `passed=false` → goal 仍进行中，写入 `judge_findings`，`evaluate_after_run` 返回 `Continue` 且 continuation prompt 包含最近 findings 并引导调用 `agent_judge`。
  - 存量 `status='complete'` 迁移后 `judge_passed=1`、`judge_completeness=100`，不会被新续行逻辑重新拉起。
  - `agent_judge` 仅在有未通过验收 goal 时注入主 agent；无 goal 或已验收通过时主 agent 工具集不含 `agent_judge`；任何 subagent 工具集不含 `agent_judge`。
  - 运行时门禁：subagent 直接调用 `agent_judge` 被拒绝；`agent_parallel` task 使用 `agent_judge` 被拒绝；主 agent→Judge 合法（depth 2）；Judge→explore/review 合法（depth 3）。
  - Judge 模型角色使用 primary；Explore/Review 仍保持既有模型映射。
  - Prompt surface：`SubagentJudge` 能构建 system prompt；`AnySubagent` / `BuiltinSubagent` 匹配 Judge；Judge 模板包含诊断型 shell 软约束和结构化输出契约。
  - JudgeReport 解析失败、`passed=true` 但 summary 空、completeness 越界、`passed=false` findings 空 → 均视为未通过或安全兜底，不误标完成。
  - `goal_scored` 工具与常量已删除（编译期 + 检索为 0 个非历史设计文档引用）。
- **前端**：`npm run typecheck`；若改动前端测试则 `npm run test:unit`。重点验证 `GoalPayload` / `GoalStoreState` 新字段不会破坏事件处理，`goal-status-bar.tsx` 能显示已验收通过。
- **文案检索**：全局搜索 `goal_scored`，除历史文档/迁移注释外不应有运行时 prompt、前端提示或 gateway 文案引用。
- **手动冒烟**：创建 goal → 主 agent 工作 → 调 agent_judge 未通过（findings）→ 续行修复 → 再次 agent_judge 通过 → goal 状态条显示已验收、续行停止。

---

## 8. 风险与边界

1. **主 agent 始终不调用 `agent_judge`**：goal 永远不被验收，续行会持续注入 prompt 直至护栏触发（idle/预算上限）。这正是护栏保留的价值——兜底防止无限续行。需在 prompt 中强力引导主 agent 调用 agent_judge。
2. **Judge 误判**：Judge 也是 LLM，可能误通过或误拒。误通过风险通过“独立上下文 + 文件工具只读 + primary 模型 + 重点核对一致性/完整性 + 可跑诊断验证”降低；误拒会触发续行修复，代价是额外轮次。
3. **诊断型 shell 不是硬只读**：Judge 可用 `shell` 意味着理论上能执行修改性命令。首版通过 Judge prompt 进行软约束，要求只运行测试、type-check、lint、只读检查，并禁止修改文件、删除数据、安装依赖、改变全局状态。若后续发现模型不稳定，应新增受限 test-runner 或 shell allowlist。
4. **Judge 成本**：每次验收会拉起一个可委派的 subagent run，可能再并行 explore/review，token/时间开销不小。首版不把 Judge/subagent token 单独计入 goal budget，也不新增 Judge 专属硬超时；需在 continuation prompt 中提示主 agent“仅在确有把握达成时再申请验收”，避免频繁空验收。
5. **深度语义边界**：Judge `max_delegation_depth=2` 必须与 `MAIN_AGENT_CHILD_DEPTH=2` 一致，且要确保 Judge 在 depth 2 仍能委派 depth 3 的 explore/review（受 `GLOBAL_MAX_DELEGATION_DEPTH=5` 与 explore/review 自身上限 3 约束，合法）。同时必须在递归委派和 parallel 路径拒绝任何 helper→Judge 调用，避免职责边界被绕过。
6. **迁移兼容**：迁移必须回填 `UPDATE goals SET judge_passed=1, judge_completeness=100 ... WHERE status='complete'`。运行时若遇到 `Complete && !judge_passed`，应记录 warning 并停续行，不能把存量已完成 goal 重新拉起。
7. **gateway / ACP 路径**：微信/企微与 ACP 同样依赖 goal 续行，首版需确认这些入口创建主 agent run 时走同一 `build_session_spec()` 注入逻辑，且 prompt/gateway 文案不再提 `goal_scored`。
8. **同轮继续修改**：Judge 通过后主 agent 仍可能在同一 run 继续调用其他工具。首版不做写工具硬锁，通过 Judge 工具结果和 `active_goal.tpl.md` prompt 要求停止修改；若后续发现问题，再加 `Complete && judge_passed` 后 mutating tools 拒绝策略。
9. **跨平台**：主体为 Rust/SQLite/prompt/TypeScript 类型改动，应保持跨平台兼容；shell 诊断命令由 Judge 根据项目现有命令选择，prompt 中需提醒避免平台特定假设。
