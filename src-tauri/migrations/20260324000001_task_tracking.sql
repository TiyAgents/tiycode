-- Task tracking tables for thread task tracking
-- Design: docs/superpowers/specs/2026-03-23-thread-task-tracking-design.md

CREATE TABLE IF NOT EXISTS task_boards (
    id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    active_task_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_boards_thread_id ON task_boards(thread_id);
CREATE INDEX IF NOT EXISTS idx_task_boards_status ON task_boards(status);

CREATE TABLE IF NOT EXISTS task_items (
    id TEXT PRIMARY KEY NOT NULL,
    task_board_id TEXT NOT NULL,
    description TEXT NOT NULL,
    stage TEXT NOT NULL DEFAULT 'pending',
    sort_order INTEGER NOT NULL DEFAULT 0,
    error_detail TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (task_board_id) REFERENCES task_boards(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_items_task_board_id ON task_items(task_board_id);
CREATE INDEX IF NOT EXISTS idx_task_items_stage ON task_items(stage);
