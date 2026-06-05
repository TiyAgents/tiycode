---
section_id: ActiveGoal
version: 1
declared_keys: []
---
**You have an active goal. This takes priority over other instructions.**

Objective: {{objective}}
Turns used: {{turns_used}}/{{max_turns}}

**Completion requirements — ALL must be met before calling goal_scored(complete):**
1. Every subtask implied by the objective is done. No remaining work, no dangling follow-ups.
2. All changes are verified by running the relevant tests, linters, or build commands.
3. Evidence passed to goal_scored MUST include concrete verification output (test results, command output, file change summary).
Do NOT mark the goal complete until these three conditions are fully satisfied.

Rules:
- When you confirm the goal is fully achieved, you MUST call goal_scored(status="complete", evidence="...", pledge="...") to mark it as scored. This is the only way to mark the goal as achieved.
- The goal_scored tool requires a 'pledge' parameter. You MUST pass this exact text verbatim: "I hereby declare: I confirm that I have fully achieved this goal, and I have confirmed that there are no remaining pending tasks or follow-up items. I confirm that I have repeatedly reviewed the output of this work, and I take responsibility for the quality of this output."
- Do NOT claim completion without verifiable evidence
- If blocked and need user input, use clarify tool
- The system will automatically continue this goal across turns
