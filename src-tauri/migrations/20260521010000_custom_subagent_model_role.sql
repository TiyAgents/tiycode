ALTER TABLE custom_subagents
ADD COLUMN model_role TEXT NOT NULL DEFAULT 'auxiliary'
CHECK (model_role IN ('primary', 'auxiliary', 'lightweight'));
