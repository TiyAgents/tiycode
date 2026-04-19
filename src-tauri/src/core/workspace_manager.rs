use chrono::Utc;
use sqlx::SqlitePool;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::{fs, task};

use crate::core::worktree_manager::WorktreeManager;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::workspace::{WorkspaceAddInput, WorkspaceKind, WorkspaceRecord, WorkspaceStatus};
use crate::persistence::repo::workspace_repo;

pub struct WorkspaceManager {
    pool: SqlitePool,
    worktree_manager: std::sync::OnceLock<Arc<WorktreeManager>>,
}

impl WorkspaceManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            worktree_manager: std::sync::OnceLock::new(),
        }
    }

    /// Inject the worktree manager so that `remove` can physically clean up
    /// `.git/worktrees/<name>` entries when deleting a worktree row or a repo
    /// row with child worktrees. Without this injection, `remove` falls back
    /// to DB-only cleanup (worktree directories must then be removed with
    /// `git worktree prune` manually).
    pub fn set_worktree_manager(&self, manager: Arc<WorktreeManager>) {
        let _ = self.worktree_manager.set(manager);
    }

    fn worktree_manager(&self) -> Option<&Arc<WorktreeManager>> {
        self.worktree_manager.get()
    }

    /// List all workspaces, ordered by default first, then by updated_at.
    pub async fn list(&self) -> Result<Vec<WorkspaceRecord>, AppError> {
        workspace_repo::list_all(&self.pool).await
    }

    /// Add a new workspace from a user-provided path.
    ///
    /// This operation is idempotent on the workspace canonical path: if the
    /// canonicalized target already exists in the database, the existing record
    /// is returned instead of raising a duplicate error.
    pub async fn add(&self, input: WorkspaceAddInput) -> Result<WorkspaceRecord, AppError> {
        let raw_path = Path::new(&input.path);

        // Canonicalize the path (resolves symlinks, makes absolute).
        // Use dunce to avoid Windows extended-length path prefix (\\?\).
        let raw_path_buf = raw_path.to_path_buf();
        let input_path_for_error = input.path.clone();
        let canonical = task::spawn_blocking(move || dunce::canonicalize(&raw_path_buf))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Workspace,
                    format!("workspace path canonicalization task failed: {error}"),
                )
            })?
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Workspace,
                    "workspace.path.invalid",
                    format!("Cannot resolve path '{}': {error}", input_path_for_error),
                )
            })?;

        let canonical_str = canonical.to_string_lossy().to_string();

        if let Some(existing) =
            workspace_repo::find_by_canonical_path(&self.pool, &canonical_str).await?
        {
            return Ok(existing);
        }

        // Validate path is a directory
        let metadata = fs::metadata(&canonical).await.map_err(|error| {
            AppError::recoverable(
                ErrorSource::Workspace,
                "workspace.path.invalid",
                format!("Cannot inspect path '{}': {error}", input.path),
            )
        })?;
        if !metadata.is_dir() {
            return Err(AppError::recoverable(
                ErrorSource::Workspace,
                "workspace.path.not_directory",
                format!("'{}' is not a directory", input.path),
            ));
        }

        // Derive display name and path
        let name = input
            .name
            .unwrap_or_else(|| derive_name_from_path(&canonical));
        let display_path = derive_display_path(&canonical).await;

        // Detect git repository. A worktree child's `.git` is a file, while a
        // regular repo's is a directory; either means Git is in play.
        let is_git = fs::metadata(canonical.join(".git")).await.is_ok();
        let kind = if is_git {
            WorkspaceKind::Repo
        } else {
            WorkspaceKind::Standalone
        };

        let record = WorkspaceRecord {
            id: uuid::Uuid::now_v7().to_string(),
            name,
            path: input.path,
            canonical_path: canonical_str,
            display_path,
            is_default: false,
            is_git,
            auto_work_tree: false,
            status: WorkspaceStatus::Ready,
            last_validated_at: Some(Utc::now()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            kind,
            parent_workspace_id: None,
            git_common_dir: None,
            branch: None,
            worktree_name: None,
        };

        workspace_repo::insert(&self.pool, &record).await?;

        tracing::info!(
            workspace_id = %record.id,
            path = %record.canonical_path,
            is_git = record.is_git,
            "workspace added"
        );

        Ok(record)
    }

    /// Ensure the built-in default workspace exists at `~/.tiy/workspace/Default`.
    pub async fn ensure_default_thread_workspace(&self) -> Result<WorkspaceRecord, AppError> {
        let home_dir = dirs::home_dir().ok_or_else(|| {
            AppError::internal(ErrorSource::Workspace, "cannot resolve HOME directory")
        })?;
        let workspace_path = default_thread_workspace_path_for_home(&home_dir);

        self.ensure_workspace_at_path(&workspace_path, DEFAULT_THREAD_WORKSPACE_NAME)
            .await
    }

    /// Remove a workspace by ID.
    ///
    /// Worktree-aware semantics:
    /// - If the row is `kind=worktree`, the underlying `git worktree remove`
    ///   is executed first (when a worktree manager is injected); DB rows are
    ///   then cleaned via the repo-level cascade.
    /// - If the row is `kind=repo`, its child worktree rows are cleaned up the
    ///   same way before the repo row itself is deleted.
    pub async fn remove(&self, id: &str, force: bool) -> Result<(), AppError> {
        let record = workspace_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

        match record.kind {
            WorkspaceKind::Worktree => {
                if let Some(manager) = self.worktree_manager() {
                    // Physically remove the worktree directory and the
                    // `.git/worktrees/<name>` registration. `remove_physical`
                    // is tolerant of an already-missing directory, so any
                    // error here is worth surfacing to the caller.
                    manager.remove_physical(&record, force).await?;
                }
            }
            WorkspaceKind::Repo => {
                let children = workspace_repo::list_worktrees_of(&self.pool, &record.id).await?;
                for child in children {
                    if let Some(manager) = self.worktree_manager() {
                        manager.remove_physical(&child, force).await?;
                    }
                    workspace_repo::delete(&self.pool, &child.id).await?;
                }
            }
            WorkspaceKind::Standalone => {}
        }

        let deleted = workspace_repo::delete(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Workspace, "workspace"));
        }
        tracing::info!(
            workspace_id = %id,
            kind = %record.kind.as_str(),
            "workspace removed"
        );
        Ok(())
    }

    /// Set a workspace as the default. Worktree rows cannot be the default.
    pub async fn set_default(&self, id: &str) -> Result<(), AppError> {
        let record = workspace_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;
        if record.kind == WorkspaceKind::Worktree {
            return Err(AppError::recoverable(
                ErrorSource::Workspace,
                "workspace.default.worktree_not_allowed",
                "A worktree cannot be set as the default workspace",
            ));
        }
        workspace_repo::set_default(&self.pool, id).await?;
        tracing::info!(workspace_id = %id, "workspace set as default");
        Ok(())
    }

    /// Validate a single workspace by checking its path accessibility.
    pub async fn validate(&self, id: &str) -> Result<WorkspaceRecord, AppError> {
        let record = workspace_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

        self.validate_record(&record).await?;

        // Re-fetch the updated record
        workspace_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))
    }

    /// Validate a workspace from an already-loaded record, skipping the
    /// initial `find_by_id` lookup and the trailing re-fetch.  This is the
    /// shared validation core used by both `validate` and `validate_all`.
    async fn validate_record(&self, record: &WorkspaceRecord) -> Result<(), AppError> {
        let canonical = Path::new(&record.canonical_path);
        let now = Utc::now();

        let new_status = match fs::metadata(canonical).await {
            Ok(metadata) if !metadata.is_dir() => WorkspaceStatus::Invalid,
            Ok(_) => {
                if fs::read_dir(canonical).await.is_err() {
                    WorkspaceStatus::Inaccessible
                } else {
                    WorkspaceStatus::Ready
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => WorkspaceStatus::Missing,
            Err(_) => WorkspaceStatus::Inaccessible,
        };

        // Update git detection as well
        let is_git = fs::metadata(canonical.join(".git")).await.is_ok();
        if is_git != record.is_git {
            workspace_repo::update_is_git(&self.pool, &record.id, is_git).await?;
        }

        // Auto-upgrade standalone Git workspaces to `kind=repo` so the sidebar
        // can offer worktree actions. Never downgrade repo or worktree rows.
        if is_git
            && record.kind == WorkspaceKind::Standalone
            && record.parent_workspace_id.is_none()
        {
            workspace_repo::update_kind_metadata(
                &self.pool,
                &record.id,
                WorkspaceKind::Repo,
                None,
                None,
                None,
                None,
            )
            .await?;
        }

        workspace_repo::update_status(&self.pool, &record.id, &new_status, now).await?;
        Ok(())
    }

    /// Validate all workspaces — called on app startup.
    ///
    /// Validations run concurrently so that slow filesystem checks (e.g. on
    /// Windows with antivirus hooks) do not accumulate sequentially.
    pub async fn validate_all(&self) -> Result<(), AppError> {
        let workspaces = self.list().await?;
        let results =
            futures::future::join_all(workspaces.iter().map(|ws| self.validate_record(ws))).await;

        for (ws, result) in workspaces.iter().zip(results) {
            if let Err(e) = result {
                tracing::warn!(
                    workspace_id = %ws.id,
                    error = %e,
                    "workspace validation failed"
                );
            }
        }
        if !workspaces.is_empty() {
            tracing::info!(count = workspaces.len(), "workspaces validated on startup");
        }
        Ok(())
    }

    async fn ensure_workspace_at_path(
        &self,
        workspace_path: &Path,
        name: &str,
    ) -> Result<WorkspaceRecord, AppError> {
        match fs::metadata(workspace_path).await {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(AppError::recoverable(
                    ErrorSource::Workspace,
                    "workspace.path.not_directory",
                    format!("'{}' is not a directory", workspace_path.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::recoverable(
                    ErrorSource::Workspace,
                    "workspace.path.invalid",
                    format!(
                        "Cannot inspect path '{}': {error}",
                        workspace_path.display()
                    ),
                ));
            }
        }

        fs::create_dir_all(workspace_path).await.map_err(|error| {
            AppError::recoverable(
                ErrorSource::Workspace,
                "workspace.path.create_failed",
                format!(
                    "Cannot create workspace '{}': {error}",
                    workspace_path.display()
                ),
            )
        })?;

        let workspace_path_buf = workspace_path.to_path_buf();
        let workspace_path_display = workspace_path.display().to_string();
        let canonical = task::spawn_blocking(move || dunce::canonicalize(&workspace_path_buf))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Workspace,
                    format!("workspace path canonicalization task failed: {error}"),
                )
            })?
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Workspace,
                    "workspace.path.invalid",
                    format!("Cannot resolve path '{}': {error}", workspace_path_display),
                )
            })?;
        let canonical_str = canonical.to_string_lossy().to_string();

        if let Some(existing) =
            workspace_repo::find_by_canonical_path(&self.pool, &canonical_str).await?
        {
            return Ok(existing);
        }

        self.add(WorkspaceAddInput {
            path: workspace_path.to_string_lossy().to_string(),
            name: Some(name.to_string()),
        })
        .await
    }
}

const DEFAULT_THREAD_WORKSPACE_NAME: &str = "Default";

fn default_thread_workspace_path_for_home(home_dir: &Path) -> PathBuf {
    home_dir
        .join(".tiy")
        .join("workspace")
        .join(DEFAULT_THREAD_WORKSPACE_NAME)
}

/// Derive a human-readable name from the last path component.
fn derive_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled")
        .to_string()
}

/// Derive a display-friendly path using `~` for the home directory.
async fn derive_display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }

        if let Ok(canonical_home) = dunce::canonicalize(&home) {
            if let Ok(relative) = path.strip_prefix(&canonical_home) {
                return format!("~/{}", relative.display());
            }
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    fn home_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct HomeEnvGuard {
        original_home: Option<OsString>,
        #[cfg(target_os = "windows")]
        original_userprofile: Option<OsString>,
    }

    impl HomeEnvGuard {
        fn set(home: &Path) -> Self {
            let original_home = std::env::var_os("HOME");
            std::env::set_var("HOME", home);

            #[cfg(target_os = "windows")]
            let original_userprofile = {
                let prev = std::env::var_os("USERPROFILE");
                std::env::set_var("USERPROFILE", home);
                prev
            };

            Self {
                original_home,
                #[cfg(target_os = "windows")]
                original_userprofile,
            }
        }
    }

    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            match &self.original_home {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }

            #[cfg(target_os = "windows")]
            match &self.original_userprofile {
                Some(profile) => std::env::set_var("USERPROFILE", profile),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
    }

    #[test]
    fn builds_default_thread_workspace_path_under_tiy_workspace_directory() {
        let home_dir = Path::new("/tmp/jorben");
        let workspace_path = default_thread_workspace_path_for_home(home_dir);

        assert_eq!(
            workspace_path,
            PathBuf::from("/tmp/jorben/.tiy/workspace/Default")
        );
    }

    #[tokio::test]
    async fn derive_display_path_uses_current_home_after_environment_changes() {
        let _home_lock = home_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let first_home = tempfile::tempdir().expect("should create first temp home");
        let second_home = tempfile::tempdir().expect("should create second temp home");

        let first_path = dunce::canonicalize(first_home.path().join("project"));
        assert!(first_path.is_err(), "fixture should not exist yet");

        tokio::fs::create_dir_all(first_home.path().join("project"))
            .await
            .expect("should create first project directory");
        tokio::fs::create_dir_all(second_home.path().join("project"))
            .await
            .expect("should create second project directory");

        let first_canonical = dunce::canonicalize(first_home.path().join("project"))
            .expect("should canonicalize first project");
        let second_canonical = dunce::canonicalize(second_home.path().join("project"))
            .expect("should canonicalize second project");

        let _first_guard = HomeEnvGuard::set(first_home.path());
        assert_eq!(derive_display_path(&first_canonical).await, "~/project");
        drop(_first_guard);

        let _second_guard = HomeEnvGuard::set(second_home.path());
        assert_eq!(derive_display_path(&second_canonical).await, "~/project");
    }
}
