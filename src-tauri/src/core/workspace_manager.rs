use chrono::Utc;
use sqlx::SqlitePool;
use std::path::Path;

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
        let canonical = dunce::canonicalize(raw_path).map_err(|e| {
            AppError::recoverable(
                ErrorSource::Workspace,
                "workspace.path.invalid",
                format!("Cannot resolve path '{}': {e}", input.path),
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
        if !canonical.is_dir() {
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
        let display_path = derive_display_path(&canonical);

        // Detect git repository
        let is_git = canonical.join(".git").exists();

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

        let new_status = if !canonical.exists() {
            WorkspaceStatus::Missing
        } else if !canonical.is_dir() {
            WorkspaceStatus::Invalid
        } else if std::fs::read_dir(canonical).is_err() {
            WorkspaceStatus::Inaccessible
        } else {
            WorkspaceStatus::Ready
        };

        // Update git detection as well
        let is_git = canonical.join(".git").exists();
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
}

/// Derive a human-readable name from the last path component.
fn derive_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled")
        .to_string()
}

/// Derive a display-friendly path using `~` for the home directory.
fn derive_display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }
    }
    path.display().to_string()
}
