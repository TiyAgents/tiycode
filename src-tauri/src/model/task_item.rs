use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TaskStage — stage of a task item
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStage {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl TaskStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            other => {
                tracing::warn!(
                    value = other,
                    "Unknown TaskStage from DB, defaulting to Pending"
                );
                Self::Pending
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TaskItem — individual step within a task board
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TaskItemRecord {
    pub id: String,
    pub task_board_id: String,
    pub description: String,
    pub stage: TaskStage,
    pub sort_order: i32,
    pub error_detail: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// DTO for task item exposed to frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItemDto {
    pub id: String,
    pub task_board_id: String,
    pub description: String,
    pub stage: TaskStage,
    pub sort_order: i32,
    pub error_detail: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<TaskItemRecord> for TaskItemDto {
    fn from(r: TaskItemRecord) -> Self {
        Self {
            id: r.id,
            task_board_id: r.task_board_id,
            description: r.description,
            stage: r.stage,
            sort_order: r.sort_order,
            error_detail: r.error_detail,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}
