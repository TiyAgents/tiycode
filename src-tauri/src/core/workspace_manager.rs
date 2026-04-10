use chrono::Utc;
use sqlx::SqlitePool;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::{fs, task};

use crate::model::errors::{AppError, ErrorSource};
use crate::model::workspace::{WorkspaceAddInput, WorkspaceRecord, WorkspaceStatus};
use crate::persistence::repo::workspace_repo;

pub struct WorkspaceManager {
    pool: SqlitePool,
}

impl WorkspaceManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// List all workspaces, ordered by default first, then by updated_at.
    pub async fn list(&self) -> Result<Vec<WorkspaceRecord>, AppError> {
        workspace_repo::list_all(&self.pool).await
    }

    /// Add a new workspace from a user-provided path.
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

        // Check for duplicates
        if workspace_repo::find_by_canonical_path(&self.pool, &canonical_str)
            .await?
            .is_some()
        {
            return Err(AppError::recoverable(
                ErrorSource::Workspace,
                "workspace.duplicate",
                format!("Workspace '{}' already exists", canonical_str),
            ));
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

        // Detect git repository
        let is_git = fs::metadata(canonical.join(".git")).await.is_ok();

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
    pub async fn remove(&self, id: &str) -> Result<(), AppError> {
        let deleted = workspace_repo::delete(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Workspace, "workspace"));
        }
        tracing::info!(workspace_id = %id, "workspace removed");
        Ok(())
    }

    /// Set a workspace as the default.
    pub async fn set_default(&self, id: &str) -> Result<(), AppError> {
        workspace_repo::set_default(&self.pool, id).await?;
        tracing::info!(workspace_id = %id, "workspace set as default");
        Ok(())
    }

    /// Validate a single workspace by checking its path accessibility.
    pub async fn validate(&self, id: &str) -> Result<WorkspaceRecord, AppError> {
        let record = workspace_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

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
            workspace_repo::update_is_git(&self.pool, id, is_git).await?;
        }

        workspace_repo::update_status(&self.pool, id, &new_status, now).await?;

        // Re-fetch the updated record
        workspace_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))
    }

    /// Validate all workspaces — called on app startup.
    pub async fn validate_all(&self) -> Result<(), AppError> {
        let workspaces = self.list().await?;
        for ws in &workspaces {
            if let Err(e) = self.validate(&ws.id).await {
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

        match self
            .add(WorkspaceAddInput {
                path: workspace_path.to_string_lossy().to_string(),
                name: Some(name.to_string()),
            })
            .await
        {
            Ok(record) => Ok(record),
            Err(error) if error.error_code == "workspace.duplicate" => {
                workspace_repo::find_by_canonical_path(&self.pool, &canonical_str)
                    .await?
                    .ok_or_else(|| {
                        AppError::internal(
                            ErrorSource::Workspace,
                            "workspace already exists but could not be loaded",
                        )
                    })
            }
            Err(error) => Err(error),
        }
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

        let home_for_canonicalize = home.clone();
        if let Ok(Ok(canonical_home)) =
            task::spawn_blocking(move || dunce::canonicalize(&home_for_canonicalize)).await
        {
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

    #[test]
    fn builds_default_thread_workspace_path_under_tiy_workspace_directory() {
        let home_dir = Path::new("/tmp/jorben");
        let workspace_path = default_thread_workspace_path_for_home(home_dir);

        assert_eq!(
            workspace_path,
            PathBuf::from("/tmp/jorben/.tiy/workspace/Default")
        );
    }
}
