---
section_id: ImplementationHandoff
version: 1
declared_keys: [action_note, plan_revision, plan_file_note]
---
Implementation handoff:
- {{action_note}}
- Plan revision: {{plan_revision}}{{plan_file_note}}
- The reset context already includes a historical summary and the approved plan.
- Treat the approved plan in context as the implementation baseline.
- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.
- After implementation, use agent_review with planFilePath to verify each plan step was completed.
