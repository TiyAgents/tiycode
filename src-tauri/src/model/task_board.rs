use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TaskBoard — container for a tracked task within a thread
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskBoardStatus {
    Active,
    Completed,
    Abandoned,
}

impl TaskBoardStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "completed" => Self::Completed,
            "abandoned" => Self::Abandoned,
            other => {
                tracing::warn!(
                    value = other,
                    "Unknown TaskBoardStatus from DB, defaulting to Active"
                );
                Self::Active
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskBoardRecord {
    pub id: String,
    pub thread_id: String,
    pub title: String,
    pub status: TaskBoardStatus,
    pub active_task_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// DTO for task board exposed to frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBoardDto {
    pub id: String,
    pub thread_id: String,
    pub title: String,
    pub status: TaskBoardStatus,
    pub active_task_id: Option<String>,
    pub tasks: Vec<super::task_item::TaskItemDto>,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Input types for create_task tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub title: String,
    pub steps: Vec<CreateTaskStep>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskStep {
    pub description: String,
}

// ---------------------------------------------------------------------------
// Input types for update_task tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskInput {
    pub task_board_id: String,
    #[serde(flatten)]
    pub action: UpdateTaskAction,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum UpdateTaskAction {
    #[serde(rename_all = "camelCase")]
    StartStep {
        step_id: String,
    },
    #[serde(rename_all = "camelCase")]
    CompleteStep {
        step_id: String,
    },
    #[serde(rename_all = "camelCase")]
    FailStep {
        step_id: String,
        error_detail: String,
    },
    CompleteBoard,
    #[serde(rename_all = "camelCase")]
    AbandonBoard {
        reason: Option<String>,
    },
}
