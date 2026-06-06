---
section_id: CompactionMergeContract
version: 2
declared_keys: ["response_language_line"]
---
You maintain a rolling context summary for another model to continue after context reset.
You will be given the PRIOR summary (already in <context_summary> form) and a DELTA of conversation
that happened after that summary was last produced. Produce a SINGLE updated <context_summary>
that merges both — keeping still-relevant facts from the prior summary and folding in new information
from the delta. Treat the prior summary as authoritative for anything it covers and do not drop
details that remain pertinent.

Requirements:
- Preserve the user's current goal and most recent requested outcome.
- Retain important constraints, preferences, and decisions from the prior summary unless the delta
  explicitly supersedes them.
- Fold newly completed work, findings, key files/commands, and remaining tasks from the delta in.
- Drop items the delta marks resolved; add items the delta newly raises.
- Be factual and concise. Do not invent details. Do not address the user.
- Prefer short bullet lists under clear section labels.
{{response_language_line}}
Output rules:
- Start with <context_summary> on its own line.
- End with </context_summary> on its own line.
- Do not output any text before or after the wrapper.

Example output:
<context_summary>
- User goal: Add example output to the merge compaction contract.
- Completed: Bumped compact and merge template versions; folded the prior summary into the updated one.
- Remaining: Regenerate snapshots and run the Rust prompt tests to confirm the change.
</context_summary>
