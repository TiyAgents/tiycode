use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitFileState {
    Tracked,
    Modified,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Typechange,
    Unmerged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepoCapabilitiesDto {
    pub repo_available: bool,
    pub git_cli_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChangeDto {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub status: GitChangeKind,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitSummaryDto {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author_name: String,
    pub committed_at: String,
    pub refs: Vec<String>,
    pub is_head: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshotDto {
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub capabilities: GitRepoCapabilitiesDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_oid: Option<String>,
    pub is_detached: bool,
    pub ahead_count: u32,
    pub behind_count: u32,
    pub staged_files: Vec<GitFileChangeDto>,
    pub unstaged_files: Vec<GitFileChangeDto>,
    pub untracked_files: Vec<GitFileChangeDto>,
    pub recent_commits: Vec<GitCommitSummaryDto>,
    pub last_refreshed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitDiffLineKind {
    Context,
    Add,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffLineDto {
    pub kind: GitDiffLineKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_number: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffHunkDto {
    pub header: String,
    pub lines: Vec<GitDiffLineDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffDto {
    pub path: String,
    pub staged: bool,
    pub status: GitChangeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_path: Option<String>,
    pub additions: u32,
    pub deletions: u32,
    pub is_binary: bool,
    pub truncated: bool,
    pub hunks: Vec<GitDiffHunkDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatusDto {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged_status: Option<GitChangeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unstaged_status: Option<GitChangeKind>,
    pub is_untracked: bool,
    pub is_ignored: bool,
}
