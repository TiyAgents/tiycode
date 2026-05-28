//! User session state machine for the IM gateway.
//!
//! Tracks which workspace and thread a user is currently interacting with,
//! and persists this state to SQLite for recovery after restart.

use sqlx::SqlitePool;

use crate::model::thread::ThreadSummaryDto;
use crate::model::workspace::WorkspaceRecord;

use super::traits::Platform;

/// The current state of a gateway user session.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    /// Waiting for user input (no active run).
    Idle,
    /// User was shown a workspace list and we expect a number selection.
    AwaitingWorkspaceSelection,
    /// User was shown a thread list and we expect a number selection.
    AwaitingThreadSelection,
    /// An agent run is in progress.
    AgentRunning { run_id: String },
    /// Waiting for user to confirm/deny a tool approval.
    AwaitingApproval {
        tool_call_id: String,
        tool_name: String,
    },
}

impl SessionState {
    /// Serialize state to a simple string for DB storage.
    /// Only the persistent "position" is stored — transient states reset to idle.
    pub fn to_db_string(&self) -> &'static str {
        // Only persist the stable state; transient states (running, approval) reset on restart.
        "idle"
    }
}

/// Tracks a single user's gateway session state including current workspace,
/// thread, and interaction mode.
#[derive(Debug, Clone)]
pub struct UserSession {
    /// Platform this session belongs to.
    pub platform: Platform,
    /// User identifier on the platform.
    pub user_id: String,
    /// Currently active workspace ID.
    pub current_workspace_id: Option<String>,
    /// Currently active thread ID within the workspace.
    pub current_thread_id: Option<String>,
    /// Current interaction state.
    pub state: SessionState,
    /// Cached workspace list for number-based selection.
    pub cached_workspaces: Vec<WorkspaceRecord>,
    /// Cached thread list for number-based selection.
    pub cached_threads: Vec<ThreadSummaryDto>,
    /// Cached profile list for number-based selection.
    pub cached_profiles: Vec<crate::model::provider::AgentProfileRecord>,
}

impl UserSession {
    /// Create a new idle session.
    pub fn new(platform: Platform, user_id: String) -> Self {
        Self {
            platform,
            user_id,
            current_workspace_id: None,
            current_thread_id: None,
            state: SessionState::Idle,
            cached_workspaces: Vec::new(),
            cached_threads: Vec::new(),
            cached_profiles: Vec::new(),
        }
    }

    /// Return the current workspace ID or an error message for the user.
    pub fn require_workspace(&self) -> anyhow::Result<&str> {
        self.current_workspace_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("请先选择 workspace，发送 /ws 查看列表"))
    }

    /// Return the current thread ID or an error message for the user.
    pub fn require_thread(&self) -> anyhow::Result<&str> {
        self.current_thread_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("请先进入会话，发送 /threads 查看列表或 /new 创建新会话")
        })
    }

    /// Whether the session has an active agent run.
    pub fn is_running(&self) -> bool {
        matches!(self.state, SessionState::AgentRunning { .. })
    }

    /// Whether the session is awaiting user approval for a tool call.
    pub fn is_awaiting_approval(&self) -> bool {
        matches!(self.state, SessionState::AwaitingApproval { .. })
    }

    /// Load session from DB, or create a new one if not found.
    pub async fn load_or_create(
        pool: &SqlitePool,
        platform: Platform,
        user_id: &str,
    ) -> anyhow::Result<Self> {
        let platform_str = platform.to_string();
        let row = sqlx::query_as::<_, GatewaySessionRow>(
            "SELECT user_id, platform, current_workspace_id, current_thread_id, state
             FROM gateway_sessions
             WHERE user_id = ? AND platform = ?",
        )
        .bind(user_id)
        .bind(&platform_str)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(Self {
                platform,
                user_id: row.user_id,
                current_workspace_id: row.current_workspace_id,
                current_thread_id: row.current_thread_id,
                state: SessionState::Idle, // Always reset transient state on load
                cached_workspaces: Vec::new(),
                cached_threads: Vec::new(),
                cached_profiles: Vec::new(),
            })
        } else {
            let session = Self::new(platform, user_id.to_string());
            session.save(pool).await?;
            Ok(session)
        }
    }

    /// Persist the current workspace/thread binding to DB.
    pub async fn save(&self, pool: &SqlitePool) -> anyhow::Result<()> {
        let platform_str = self.platform.to_string();
        sqlx::query(
            "INSERT INTO gateway_sessions (user_id, platform, current_workspace_id, current_thread_id, state, updated_at)
             VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%f', 'now'))
             ON CONFLICT (user_id, platform) DO UPDATE SET
                current_workspace_id = excluded.current_workspace_id,
                current_thread_id = excluded.current_thread_id,
                state = excluded.state,
                updated_at = excluded.updated_at",
        )
        .bind(&self.user_id)
        .bind(&platform_str)
        .bind(&self.current_workspace_id)
        .bind(&self.current_thread_id)
        .bind(self.state.to_db_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update the current workspace and persist.
    pub async fn switch_workspace(
        &mut self,
        pool: &SqlitePool,
        workspace_id: &str,
    ) -> anyhow::Result<()> {
        self.current_workspace_id = Some(workspace_id.to_string());
        self.current_thread_id = None; // Reset thread when switching workspace
        self.state = SessionState::Idle;
        self.save(pool).await
    }

    /// Update the current thread and persist.
    pub async fn switch_thread(
        &mut self,
        pool: &SqlitePool,
        thread_id: &str,
    ) -> anyhow::Result<()> {
        self.current_thread_id = Some(thread_id.to_string());
        self.state = SessionState::Idle;
        self.save(pool).await
    }
}

/// Row type for reading from `gateway_sessions` table.
#[derive(Debug, sqlx::FromRow)]
struct GatewaySessionRow {
    user_id: String,
    #[allow(dead_code)]
    platform: String,
    current_workspace_id: Option<String>,
    current_thread_id: Option<String>,
    #[allow(dead_code)]
    state: String,
}
