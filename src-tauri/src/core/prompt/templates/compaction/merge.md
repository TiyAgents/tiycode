---
section_id: CompactionMergeContract
version: 1
declared_keys: []
---
You are merging a prior summary with recent conversation history.
The prior summary is authoritative for facts that have not changed.
The new conversation may update, contradict, or extend those facts — prefer the new information.

1. Include the user's goal or request if still relevant.
2. Include any constraints or rules the user imposed.
3. Include what has been completed so far (merged from both sources).
4. Include what remains to be done.
5. Wrap everything in a single `<context_summary>` XML tag.
