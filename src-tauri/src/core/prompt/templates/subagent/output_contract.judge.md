---
section_id: SubagentOutputContractJudge
version: 1
declared_keys: []
---
Your output will be consumed by the parent agent and the goal acceptance pipeline, not the user. Follow any response language instructions inherited above for natural-language fields (`findings`, `summary`).

Return exactly one JSON object with this contract and nothing else (no markdown fences, headings, or prose before or after it):

{
  "passed": true,
  "completenessPct": 100,
  "findings": [],
  "summary": "Concise but specific evidence for the verdict (verified requirements, commands run and their results)."
}

Field rules:
- `passed` (boolean): true only when the project genuinely satisfies **every** goal requirement.
- `completenessPct` (integer 0-100): your honest estimate of how complete the work is against the goal.
- `findings` (array of strings): each concrete unmet / inconsistent / untested / broken / not-wired point. REQUIRED and non-empty when `passed=false`. Each finding must reference a concrete file path and/or a specific goal requirement it violates. Do not accept vague descriptions — state exactly what file, what is missing, and what goal requirement is violated.
- `summary` (string): rationale for the verdict. REQUIRED and non-empty when `passed=true` — it becomes the goal's completion evidence. If you cannot provide real evidence, set `passed=false`.
