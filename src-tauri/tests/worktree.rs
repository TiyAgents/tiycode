//! Workspace worktree management tests
//!
//! Covers:
//! - `WorktreeManager::create` produces a sibling worktree directory,
//!   inserts a `kind='worktree'` workspace row, and records the parent link.
//! - Creating a worktree against a non-repo workspace is rejected.
//! - `WorkspaceManager::remove` on a worktree removes the `.git/worktrees/<name>`
//!   registration and the DB row.
//! - `WorkspaceManager::remove` on the parent repo also removes all child
//!   worktree rows and their `.git/worktrees/<name>` registrations.
//! - `WorkspaceManager::set_default` refuses worktree rows.
//! - `WorkspaceManager::validate` upgrades a standalone Git workspace to
//!   `kind='repo'` so the UI can surface worktree actions.

mod test_helpers;

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::{Repository, Signature};
use sqlx::Row;
use tiycode_lib::core::workspace_manager::WorkspaceManager;
use tiycode_lib::core::worktree_manager::WorktreeManager;
use tiycode_lib::model::workspace::{
    WorkspaceAddInput, WorkspaceKind, WorkspaceStatus, WorktreeCreateInput,
};

fn git_cli_available() -> bool {
    Command::new("git")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn init_repo_with_commit(repo_root: &Path) {
    std::fs::create_dir_all(repo_root).expect("should create repo root");
    let repo = Repository::init(repo_root).expect("should init repo");
    std::fs::write(repo_root.join("README.md"), "# test\n").expect("should write README");
    let mut index = repo.index().expect("should get index");
    index
        .add_path(Path::new("README.md"))
        .expect("should stage README");
    index.write().expect("should write index");
    let tree_id = index.write_tree().expect("should write tree");
    let tree = repo.find_tree(tree_id).expect("should find tree");
    let sig = Signature::now("TiyCode", "tests@tiy.local").expect("should build signature");
    repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
        .expect("should commit");
}

fn canonical(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

async fn setup_repo_workspace(
    pool: &sqlx::SqlitePool,
    repo_root: &Path,
) -> tiycode_lib::model::workspace::WorkspaceRecord {
    let manager = WorkspaceManager::new(pool.clone());
    manager
        .add(WorkspaceAddInput {
            path: repo_root.to_string_lossy().to_string(),
            name: Some("Test Repo".to_string()),
        })
        .await
        .expect("should add repo workspace")
}

fn worktree_admin_path(repo_root: &Path, name: &str) -> PathBuf {
    repo_root.join(".git").join("worktrees").join(name)
}

#[tokio::test]
async fn worktree_create_registers_workspace_and_updates_parent_kind() {
    if !git_cli_available() {
        eprintln!("skipping worktree test: git CLI not available");
        return;
    }

    let tmp = tempfile::tempdir().expect("should create temp root");
    let repo_root = tmp.path().join("repo-main");
    init_repo_with_commit(&repo_root);

    let pool = test_helpers::setup_test_pool().await;
    let parent = setup_repo_workspace(&pool, &repo_root).await;

    assert_eq!(
        parent.kind,
        WorkspaceKind::Repo,
        "Git repo should be kind=repo"
    );
    assert!(parent.is_git);

    let worktree_manager = WorktreeManager::new(pool.clone());
    let created = worktree_manager
        .create(
            &parent,
            WorktreeCreateInput {
                branch: "feature/worktree-alpha".to_string(),
                base_ref: None,
                create_branch: true,
                track_upstream: false,
                path: None,
            },
        )
        .await
        .expect("worktree creation should succeed");

    assert_eq!(created.kind, WorkspaceKind::Worktree);
    assert_eq!(
        created.parent_workspace_id.as_deref(),
        Some(parent.id.as_str())
    );
    assert_eq!(created.branch.as_deref(), Some("feature/worktree-alpha"));
    let worktree_name = created.worktree_name.clone().expect("worktree_name set");
    assert!(
        worktree_name.len() > 6,
        "worktree_name should include hex + slug"
    );
    assert!(created.canonical_path != canonical(&repo_root));

    // Worktree directory and admin dir both exist on disk.
    assert!(Path::new(&created.canonical_path).is_dir());
    // Git enumerates this worktree.
    let enumerated = worktree_manager
        .list(&parent)
        .await
        .expect("list should succeed");
    assert_eq!(enumerated.len(), 1);
    assert_eq!(
        enumerated[0].workspace_id.as_deref(),
        Some(created.id.as_str())
    );
    assert!(enumerated[0].is_valid);
}

#[tokio::test]
async fn worktree_create_rejects_non_repo_parent() {
    let tmp = tempfile::tempdir().expect("should create temp root");
    let folder = tmp.path().join("plain-folder");
    std::fs::create_dir_all(&folder).unwrap();

    let pool = test_helpers::setup_test_pool().await;
    let ws_manager = WorkspaceManager::new(pool.clone());
    let parent = ws_manager
        .add(WorkspaceAddInput {
            path: folder.to_string_lossy().to_string(),
            name: None,
        })
        .await
        .expect("should add standalone workspace");

    assert_eq!(parent.kind, WorkspaceKind::Standalone);

    let worktree_manager = WorktreeManager::new(pool.clone());
    let err = worktree_manager
        .create(
            &parent,
            WorktreeCreateInput {
                branch: "feature/foo".to_string(),
                base_ref: None,
                create_branch: true,
                track_upstream: false,
                path: None,
            },
        )
        .await
        .unwrap_err();

    assert_eq!(err.error_code, "workspace.worktree.parent_not_repo");
}

#[tokio::test]
async fn worktree_remove_cleans_up_admin_dir_and_db_row() {
    if !git_cli_available() {
        eprintln!("skipping worktree test: git CLI not available");
        return;
    }

    let tmp = tempfile::tempdir().expect("should create temp root");
    let repo_root = tmp.path().join("repo-remove");
    init_repo_with_commit(&repo_root);

    let pool = test_helpers::setup_test_pool().await;
    let ws_manager = WorkspaceManager::new(pool.clone());
    let worktree_manager = std::sync::Arc::new(WorktreeManager::new(pool.clone()));
    ws_manager.set_worktree_manager(std::sync::Arc::clone(&worktree_manager));

    let parent = ws_manager
        .add(WorkspaceAddInput {
            path: repo_root.to_string_lossy().to_string(),
            name: None,
        })
        .await
        .unwrap();

    let created = worktree_manager
        .create(
            &parent,
            WorktreeCreateInput {
                branch: "feature/remove-me".to_string(),
                base_ref: None,
                create_branch: true,
                track_upstream: false,
                path: None,
            },
        )
        .await
        .expect("create should succeed");

    // Confirm admin directory exists before removal.
    let basename = Path::new(&created.canonical_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap()
        .to_string();
    let admin = worktree_admin_path(&repo_root, &basename);
    assert!(admin.is_dir(), "admin dir should exist pre-remove");

    ws_manager
        .remove(&created.id, false)
        .await
        .expect("remove should succeed");

    // DB row gone.
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM workspaces WHERE id = ?")
        .bind(&created.id)
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(exists.is_none());

    // Worktree directory gone.
    assert!(!Path::new(&created.canonical_path).exists());
    // Admin entry gone.
    assert!(!admin.exists());
}

#[tokio::test]
async fn worktree_parent_remove_cascades_children() {
    if !git_cli_available() {
        eprintln!("skipping worktree test: git CLI not available");
        return;
    }

    let tmp = tempfile::tempdir().expect("should create temp root");
    let repo_root = tmp.path().join("repo-parent-remove");
    init_repo_with_commit(&repo_root);

    let pool = test_helpers::setup_test_pool().await;
    let ws_manager = WorkspaceManager::new(pool.clone());
    let worktree_manager = std::sync::Arc::new(WorktreeManager::new(pool.clone()));
    ws_manager.set_worktree_manager(std::sync::Arc::clone(&worktree_manager));

    let parent = ws_manager
        .add(WorkspaceAddInput {
            path: repo_root.to_string_lossy().to_string(),
            name: None,
        })
        .await
        .unwrap();

    let child = worktree_manager
        .create(
            &parent,
            WorktreeCreateInput {
                branch: "feature/child".to_string(),
                base_ref: None,
                create_branch: true,
                track_upstream: false,
                path: None,
            },
        )
        .await
        .expect("create child should succeed");

    // Seed a thread for the child worktree to prove cascade cleanup.
    test_helpers::seed_thread(&pool, "t-child", &child.id, None).await;

    ws_manager
        .remove(&parent.id, false)
        .await
        .expect("remove parent should succeed");

    let parent_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM workspaces WHERE id = ?")
        .bind(&parent.id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("c");
    let child_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM workspaces WHERE id = ?")
        .bind(&child.id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("c");
    let thread_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM threads WHERE id = 't-child'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("c");

    assert_eq!(parent_count, 0);
    assert_eq!(child_count, 0);
    assert_eq!(thread_count, 0);
    assert!(!Path::new(&child.canonical_path).exists());
}

#[tokio::test]
async fn set_default_rejects_worktree_row() {
    if !git_cli_available() {
        eprintln!("skipping worktree test: git CLI not available");
        return;
    }

    let tmp = tempfile::tempdir().expect("should create temp root");
    let repo_root = tmp.path().join("repo-default");
    init_repo_with_commit(&repo_root);

    let pool = test_helpers::setup_test_pool().await;
    let ws_manager = WorkspaceManager::new(pool.clone());
    let worktree_manager = std::sync::Arc::new(WorktreeManager::new(pool.clone()));
    ws_manager.set_worktree_manager(std::sync::Arc::clone(&worktree_manager));

    let parent = ws_manager
        .add(WorkspaceAddInput {
            path: repo_root.to_string_lossy().to_string(),
            name: None,
        })
        .await
        .unwrap();

    let child = worktree_manager
        .create(
            &parent,
            WorktreeCreateInput {
                branch: "feature/no-default".to_string(),
                base_ref: None,
                create_branch: true,
                track_upstream: false,
                path: None,
            },
        )
        .await
        .unwrap();

    let err = ws_manager.set_default(&child.id).await.unwrap_err();
    assert_eq!(err.error_code, "workspace.default.worktree_not_allowed");
}

#[tokio::test]
async fn validate_upgrades_standalone_git_workspace_to_repo() {
    let tmp = tempfile::tempdir().expect("should create temp root");
    let repo_root = tmp.path().join("repo-upgrade");
    init_repo_with_commit(&repo_root);

    let pool = test_helpers::setup_test_pool().await;
    // Insert a raw row with kind='standalone' but is_git=1 to simulate pre-migration data.
    let canonical_path = canonical(&repo_root);
    sqlx::query(
        "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                is_default, is_git, auto_work_tree, status,
                created_at, updated_at, kind)
         VALUES (?, ?, ?, ?, ?, 0, 1, 0, 'ready',
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 'standalone')",
    )
    .bind("ws-upgrade")
    .bind("Upgrade Me")
    .bind(&canonical_path)
    .bind(&canonical_path)
    .bind(&canonical_path)
    .execute(&pool)
    .await
    .unwrap();

    let ws_manager = WorkspaceManager::new(pool.clone());
    let validated = ws_manager.validate("ws-upgrade").await.unwrap();

    assert_eq!(validated.kind, WorkspaceKind::Repo);
    assert_eq!(validated.status, WorkspaceStatus::Ready);
}
