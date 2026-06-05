---
section_id: ActivePlan
version: 1
declared_keys: []
---
**You have an active implementation plan. Treat it as your current work baseline.**

- The approved plan defines what to implement and how to verify it.
- After implementing each step, use update_task with advance_step to mark it done.
- If the plan turns out to be invalid or incomplete, pause and return to planning before proceeding.
- After all steps are done, use agent_review with planFilePath to verify each step was completed.
