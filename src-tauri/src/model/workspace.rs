use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Workspace status reflecting the current accessibility of its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceStatus {
    Ready,
    Missing,
    Inaccessible,
    Invalid,
}

impl WorkspaceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::Inaccessible => "inaccessible",
            Self::Invalid => "invalid",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "ready" => Self::Ready,
            "missing" => Self::Missing,
            "inaccessible" => Self::Inaccessible,
            _ => Self::Invalid,
        }
    }
}

/// Database row representation for the `workspaces` table.
#[derive(Debug, Clone)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub canonical_path: String,
    pub display_path: String,
    pub is_default: bool,
    pub is_git: bool,
    pub auto_work_tree: bool,
    pub status: WorkspaceStatus,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// DTO sent to the frontend via Tauri invoke.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub canonical_path: String,
    pub display_path: String,
    pub is_default: bool,
    pub is_git: bool,
    pub auto_work_tree: bool,
    pub status: WorkspaceStatus,
    pub last_validated_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<WorkspaceRecord> for WorkspaceDto {
    fn from(r: WorkspaceRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            path: r.path,
            canonical_path: r.canonical_path,
            display_path: r.display_path,
            is_default: r.is_default,
            is_git: r.is_git,
            auto_work_tree: r.auto_work_tree,
            status: r.status,
            last_validated_at: r.last_validated_at.map(|t| t.to_rfc3339()),
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

/// Input from frontend for adding a workspace.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAddInput {
    pub path: String,
    pub name: Option<String>,
}
