---
section_id: ImplementationHandoff
version: 1
declared_keys: [action_note, plan_revision, plan_file_note, plan_markdown]
---
Implementation handoff:
- {{action_note}}
- Plan revision: {{plan_revision}}{{plan_file_note}}
- Treat the approved plan below as the implementation baseline.
- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.
- After implementation, use agent_review with planFilePath to verify each plan step was completed.

Approved plan:
{{plan_markdown}}
