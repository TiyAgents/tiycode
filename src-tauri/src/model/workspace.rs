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

/// Classification of a workspace row, enabling worktree semantics alongside
/// standalone folders and Git repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    /// A regular folder workspace with no Git worktree relationship.
    Standalone,
    /// A Git repository root that may have child worktrees.
    Repo,
    /// A Git worktree pointing to a parent repo workspace.
    Worktree,
}

impl WorkspaceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Repo => "repo",
            Self::Worktree => "worktree",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "repo" => Self::Repo,
            "worktree" => Self::Worktree,
            _ => Self::Standalone,
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
    pub kind: WorkspaceKind,
    pub parent_workspace_id: Option<String>,
    pub git_common_dir: Option<String>,
    pub branch: Option<String>,
    /// Short logical worktree identifier for this TiyCode workspace.
    /// Format: `<6-hex>-<branch-slug>`. The leading 6 hex characters are a
    /// random uniqueness prefix and are what the sidebar renders as the hash
    /// tag (`worktree_name[..6]`). Note: this is NOT the `git worktree`
    /// registration name (git derives that from the worktree directory
    /// basename); we store this label separately so the UI tag stays stable
    /// even when the user picks a custom path.
    pub worktree_name: Option<String>,
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
    pub kind: WorkspaceKind,
    pub parent_workspace_id: Option<String>,
    pub git_common_dir: Option<String>,
    pub branch: Option<String>,
    pub worktree_name: Option<String>,
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
            kind: r.kind,
            parent_workspace_id: r.parent_workspace_id,
            git_common_dir: r.git_common_dir,
            branch: r.branch,
            worktree_name: r.worktree_name,
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

/// Input from the frontend when creating a worktree for a repo workspace.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreateInput {
    /// The branch name to check out inside the new worktree.
    /// When `create_branch` is true this is the name of a new branch.
    pub branch: String,
    /// Optional starting point (commit / branch / remote ref) used when creating
    /// a brand-new branch. Ignored when `create_branch` is false.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// When true, a new local branch is created and checked out in the worktree.
    #[serde(default)]
    pub create_branch: bool,
    /// Optional custom path for the new worktree directory. When None the
    /// manager derives `<repo parent>/<repo name>-<branch-slug>`.
    #[serde(default)]
    pub path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn workspace_status_maps_known_values_and_defaults_unknown_to_invalid() {
        assert_eq!(WorkspaceStatus::Ready.as_str(), "ready");
        assert_eq!(WorkspaceStatus::Missing.as_str(), "missing");
        assert_eq!(WorkspaceStatus::Inaccessible.as_str(), "inaccessible");
        assert_eq!(WorkspaceStatus::Invalid.as_str(), "invalid");

        assert_eq!(WorkspaceStatus::from_str("ready"), WorkspaceStatus::Ready);
        assert_eq!(
            WorkspaceStatus::from_str("missing"),
            WorkspaceStatus::Missing
        );
        assert_eq!(
            WorkspaceStatus::from_str("inaccessible"),
            WorkspaceStatus::Inaccessible
        );
        assert_eq!(
            WorkspaceStatus::from_str("archived"),
            WorkspaceStatus::Invalid
        );
    }

    #[test]
    fn workspace_kind_maps_known_values_and_defaults_unknown_to_standalone() {
        assert_eq!(WorkspaceKind::Standalone.as_str(), "standalone");
        assert_eq!(WorkspaceKind::Repo.as_str(), "repo");
        assert_eq!(WorkspaceKind::Worktree.as_str(), "worktree");

        assert_eq!(WorkspaceKind::from_str("repo"), WorkspaceKind::Repo);
        assert_eq!(WorkspaceKind::from_str("worktree"), WorkspaceKind::Worktree);
        assert_eq!(WorkspaceKind::from_str("folder"), WorkspaceKind::Standalone);
    }

    #[test]
    fn workspace_dto_from_record_preserves_worktree_fields_and_formats_dates() {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 24, 1, 2, 3).unwrap();
        let updated_at = Utc.with_ymd_and_hms(2026, 4, 24, 4, 5, 6).unwrap();
        let last_validated_at = Utc.with_ymd_and_hms(2026, 4, 24, 7, 8, 9).unwrap();

        let dto = WorkspaceDto::from(WorkspaceRecord {
            id: "ws-worktree".to_string(),
            name: "Feature Worktree".to_string(),
            path: "/repo-feature".to_string(),
            canonical_path: "/canonical/repo-feature".to_string(),
            display_path: "~/repo-feature".to_string(),
            is_default: true,
            is_git: true,
            auto_work_tree: true,
            status: WorkspaceStatus::Ready,
            last_validated_at: Some(last_validated_at),
            created_at,
            updated_at,
            kind: WorkspaceKind::Worktree,
            parent_workspace_id: Some("ws-parent".to_string()),
            git_common_dir: Some("/repo/.git/worktrees/feature".to_string()),
            branch: Some("feature/test".to_string()),
            worktree_name: Some("abc123-feature-test".to_string()),
        });

        assert_eq!(dto.id, "ws-worktree");
        assert_eq!(dto.status, WorkspaceStatus::Ready);
        assert_eq!(dto.kind, WorkspaceKind::Worktree);
        assert_eq!(dto.parent_workspace_id.as_deref(), Some("ws-parent"));
        assert_eq!(
            dto.git_common_dir.as_deref(),
            Some("/repo/.git/worktrees/feature")
        );
        assert_eq!(dto.branch.as_deref(), Some("feature/test"));
        assert_eq!(dto.worktree_name.as_deref(), Some("abc123-feature-test"));
        assert_eq!(dto.created_at, created_at.to_rfc3339());
        assert_eq!(dto.updated_at, updated_at.to_rfc3339());
        assert_eq!(
            dto.last_validated_at.as_deref(),
            Some(last_validated_at.to_rfc3339().as_str())
        );
    }

    #[test]
    fn worktree_create_input_uses_serde_defaults() {
        let input: WorktreeCreateInput = serde_json::from_value(json!({
            "branch": "feature/test"
        }))
        .unwrap();

        assert_eq!(input.branch, "feature/test");
        assert_eq!(input.base_ref, None);
        assert!(!input.create_branch);
        assert_eq!(input.path, None);
    }
}
