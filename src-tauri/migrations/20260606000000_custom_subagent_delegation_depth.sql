ALTER TABLE custom_subagents
ADD COLUMN can_delegate INTEGER NOT NULL DEFAULT 0
CHECK (can_delegate IN (0, 1));

ALTER TABLE custom_subagents
ADD COLUMN max_delegation_depth INTEGER NOT NULL DEFAULT 3
CHECK (max_delegation_depth >= 1 AND max_delegation_depth <= 5);
