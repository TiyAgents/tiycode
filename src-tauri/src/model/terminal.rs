use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSessionStatus {
    Starting,
    Running,
    Exited,
}

impl TerminalSessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Exited => "exited",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "starting" => Self::Starting,
            "running" => Self::Running,
            "exited" => Self::Exited,
            _ => Self::Exited,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalSessionRecord {
    pub id: String,
    pub thread_id: String,
    pub workspace_id: String,
    pub shell_path: Option<String>,
    pub cwd: Option<String>,
    pub status: TerminalSessionStatus,
    pub pid: Option<i64>,
    pub exit_code: Option<i32>,
    pub created_at: String,
    pub exited_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionDto {
    pub session_id: String,
    pub thread_id: String,
    pub workspace_id: String,
    pub shell: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub status: TerminalSessionStatus,
    pub has_unread_output: bool,
    pub last_output_at: Option<String>,
    pub exit_code: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAttachDto {
    pub session: TerminalSessionDto,
    pub replay: String,
}

