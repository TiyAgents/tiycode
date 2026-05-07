//! Lightweight global events broadcast via `AppHandle::emit` to all frontend
//! windows. These are intentionally separate from the per-run `ThreadStreamEvent`
//! channel so that sidebar and workspace-level UI can react to background thread
//! lifecycle changes without needing a dedicated stream subscription.

use serde::Serialize;

/// Event name constants used for `AppHandle::emit`.
pub const THREAD_RUN_STARTED: &str = "thread-run-started";
pub const THREAD_RUN_FINISHED: &str = "thread-run-finished";
pub const THREAD_RUN_STATUS_CHANGED: &str = "thread-run-status-changed";
pub const THREAD_TITLE_UPDATED: &str = "thread-title-updated";
pub const INDEX_GIT_OVERLAY_READY: &str = "index-git-overlay-ready";

/// Payload emitted when a thread run transitions to the `running` state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRunStartedPayload {
    pub thread_id: String,
    pub run_id: String,
}

/// Payload emitted when a thread run reaches a terminal state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRunFinishedPayload {
    pub thread_id: String,
    pub run_id: String,
    pub status: String,
}

/// Payload emitted when a thread's run status changes in a way that is
/// relevant to the sidebar indicator. Covers all intermediate states
/// (waiting_approval, needs_reply, running) as well as terminal states,
/// so the frontend can update background threads without a per-thread stream.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRunStatusChangedPayload {
    pub thread_id: String,
    pub run_id: String,
    pub status: String,
}

/// Payload emitted after a thread title is generated and persisted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTitleUpdatedPayload {
    pub thread_id: String,
    pub title: String,
}

/// Payload emitted when a workspace git overlay has been computed asynchronously
/// after the initial tree response was returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexGitOverlayReadyPayload {
    pub workspace_id: String,
    pub repo_available: bool,
    pub states: std::collections::HashMap<String, crate::model::git::GitFileState>,
}
