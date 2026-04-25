//! Git tree and repository operation tests

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use git2::{Repository, Signature};
use tiycode_lib::core::git_manager::GitManager;
use tiycode_lib::core::index_manager::IndexManager;
use tiycode_lib::ipc::frontend_channels::GitStreamEvent;
use tiycode_lib::model::git::{GitChangeKind, GitFileState};

#[tokio::test]
async fn test_git_overlay_reports_non_repo_workspace() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    std::fs::write(tmp.path().join("plain.txt"), "hello").expect("should write plain file");

    let overlay = GitManager::new()
        .get_workspace_overlay(&tmp.path().to_string_lossy())
        .await
        .expect("overlay lookup should succeed");

    assert!(
        !overlay.repo_available,
        "non-Git workspace should not report repo availability"
    );
    assert!(
        overlay.states.is_empty(),
        "non-Git workspace should not return Git states"
    );
}

#[tokio::test]
async fn test_git_overlay_marks_only_non_clean_tree_states() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("should create src directory");
    std::fs::create_dir_all(root.join("dist")).expect("should create dist directory");
    std::fs::create_dir_all(root.join("node_modules/pkg")).expect("should create node_modules");

    std::fs::write(
        root.join(".gitignore"),
        "dist/\nignored.log\nnode_modules/\n",
    )
    .expect("should write .gitignore");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");
    std::fs::write(root.join("notes.md"), "# draft\n").expect("should write untracked file");
    std::fs::write(root.join("ignored.log"), "ignored\n").expect("should write ignored file");
    std::fs::write(root.join("dist/app.js"), "console.log('dist');\n")
        .expect("should write ignored dir file");
    std::fs::write(
        root.join("node_modules/pkg/index.js"),
        "module.exports = {};\n",
    )
    .expect("should write collapsed ignored dir file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &[".gitignore", "src/main.rs"], "initial commit");

    let mut tree = IndexManager::new()
        .get_tree(&root.to_string_lossy())
        .await
        .expect("tree scan should succeed");
    let overlay = GitManager::new()
        .get_workspace_overlay(&root.to_string_lossy())
        .await
        .expect("overlay lookup should succeed");

    assert!(overlay.repo_available, "repository should be detected");
    tree.apply_git_overlay(&overlay.states);

    assert_eq!(
        find_git_state(&tree, "src"),
        None,
        "clean tracked directories should not receive overlay state"
    );
    assert_eq!(
        find_git_state(&tree, "notes.md"),
        Some(GitFileState::Untracked),
        "untracked file should be marked as untracked"
    );
    assert_eq!(
        find_git_state(&tree, "ignored.log"),
        Some(GitFileState::Ignored),
        "ignored file should be marked as ignored"
    );
    assert_eq!(
        find_git_state(&tree, "dist"),
        Some(GitFileState::Ignored),
        "ignored directory should be marked as ignored"
    );
    assert_eq!(
        find_git_state(&tree, "node_modules"),
        Some(GitFileState::Ignored),
        "collapsed ignored directories should still receive Git state"
    );
    assert!(
        find_node(&tree, ".git").is_none(),
        ".git directory should stay hidden from the tree"
    );
}

#[tokio::test]
async fn test_git_overlay_bubbles_untracked_state_above_tracked_ancestors() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src/components")).expect("should create nested directory");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");
    std::fs::write(root.join("src/components/new.rs"), "pub fn new_file() {}\n")
        .expect("should write untracked file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &["src/main.rs"], "initial commit");

    let mut tree = IndexManager::new()
        .get_tree(&root.to_string_lossy())
        .await
        .expect("tree scan should succeed");
    let overlay = GitManager::new()
        .get_workspace_overlay(&root.to_string_lossy())
        .await
        .expect("overlay lookup should succeed");

    tree.apply_git_overlay(&overlay.states);

    assert_eq!(
        find_git_state(&tree, "src"),
        Some(GitFileState::Untracked),
        "ancestor directories should surface untracked descendants"
    );
}

#[tokio::test]
async fn test_git_overlay_does_not_bubble_ignored_state_to_parent_directories() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src/target")).expect("should create nested directories");
    std::fs::write(root.join(".gitignore"), "src/target/\n").expect("should write .gitignore");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");
    std::fs::write(
        root.join("src/target/generated.rs"),
        "pub fn generated() {}\n",
    )
    .expect("should write ignored generated file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &[".gitignore", "src/main.rs"], "initial commit");

    let mut tree = IndexManager::new()
        .get_tree(&root.to_string_lossy())
        .await
        .expect("tree scan should succeed");
    let overlay = GitManager::new()
        .get_workspace_overlay(&root.to_string_lossy())
        .await
        .expect("overlay lookup should succeed");

    tree.apply_git_overlay(&overlay.states);

    assert_eq!(
        find_git_state(&tree, "src"),
        None,
        "ignored descendants should not gray out the whole parent directory"
    );

    let children = IndexManager::new()
        .get_children(&root.to_string_lossy(), "src", None, None)
        .await
        .expect("should load src children");
    let mut overlay_root = tiycode_lib::core::index_manager::FileTreeNode {
        name: "src".to_string(),
        path: "src".to_string(),
        is_dir: true,
        is_expandable: true,
        children_has_more: children.has_more,
        children_next_offset: children.next_offset,
        git_state: None,
        children: Some(children.children),
    };

    overlay_root.apply_git_overlay(&overlay.states);

    assert_eq!(
        find_git_state(&overlay_root, "src/target"),
        Some(GitFileState::Ignored),
        "the ignored child directory itself should still be marked as ignored"
    );
}

#[tokio::test]
async fn test_git_overlay_marks_modified_files_and_ancestors() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("should create src directory");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &["src/main.rs"], "initial commit");

    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"changed\");\n}\n",
    )
    .expect("should update tracked file");

    let mut tree = IndexManager::new()
        .get_tree(&root.to_string_lossy())
        .await
        .expect("tree scan should succeed");
    let overlay = GitManager::new()
        .get_workspace_overlay(&root.to_string_lossy())
        .await
        .expect("overlay lookup should succeed");

    tree.apply_git_overlay(&overlay.states);

    assert_eq!(
        find_git_state(&tree, "src"),
        Some(GitFileState::Modified),
        "ancestor directories should surface modified descendants"
    );

    let children = IndexManager::new()
        .get_children(&root.to_string_lossy(), "src", None, None)
        .await
        .expect("should load modified directory children");
    let mut overlay_root = tiycode_lib::core::index_manager::FileTreeNode {
        name: "src".to_string(),
        path: "src".to_string(),
        is_dir: true,
        is_expandable: true,
        children_has_more: children.has_more,
        children_next_offset: children.next_offset,
        git_state: None,
        children: Some(children.children),
    };

    overlay_root.apply_git_overlay(&overlay.states);

    assert_eq!(
        find_git_state(&overlay_root, "src/main.rs"),
        Some(GitFileState::Modified),
        "modified tracked file should be marked once its parent directory is loaded"
    );
}

#[tokio::test]
async fn test_git_refresh_invalidates_cached_overlay() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("should create src directory");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &["src/main.rs"], "initial commit");

    std::fs::write(root.join("notes.md"), "# draft\n").expect("should write untracked file");

    let manager = GitManager::new();
    let overlay = manager
        .get_workspace_overlay(&root.to_string_lossy())
        .await
        .expect("overlay lookup should succeed");
    assert_eq!(
        overlay.states.get("notes.md"),
        Some(&GitFileState::Untracked),
        "initial overlay should surface the untracked file"
    );

    commit_selected(&repo, &["notes.md"], "track notes");
    manager
        .refresh("workspace-1", &root.to_string_lossy())
        .await
        .expect("refresh should invalidate cached overlay state");

    let refreshed_overlay = manager
        .get_workspace_overlay(&root.to_string_lossy())
        .await
        .expect("overlay lookup should succeed after refresh");
    assert!(
        !refreshed_overlay.states.contains_key("notes.md"),
        "clean tracked files should disappear once refresh invalidates the overlay cache"
    );
}

#[tokio::test]
async fn test_git_snapshot_groups_staged_unstaged_untracked_and_history() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("should create src directory");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");
    std::fs::write(root.join("README.md"), "# Demo\n").expect("should write readme");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &["src/main.rs", "README.md"], "initial commit");

    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"changed\");\n}\n",
    )
    .expect("should update tracked file");
    std::fs::write(root.join("src/staged.rs"), "pub fn staged() {}\n")
        .expect("should write staged file");
    std::fs::write(root.join("notes.md"), "draft\n").expect("should write untracked file");
    stage_selected(&repo, &["src/staged.rs"]);

    let snapshot = GitManager::new()
        .get_snapshot("workspace-1", &root.to_string_lossy())
        .await
        .expect("snapshot lookup should succeed");

    assert!(
        snapshot.capabilities.repo_available,
        "repository capability should be detected"
    );
    assert!(
        snapshot.head_ref.is_some(),
        "snapshot should include the current HEAD ref"
    );
    assert_eq!(
        snapshot
            .recent_commits
            .first()
            .map(|commit| commit.summary.as_str()),
        Some("initial commit"),
        "recent history should include the latest commit"
    );
    assert!(
        snapshot
            .staged_files
            .iter()
            .any(|file| file.path == "src/staged.rs" && file.status == GitChangeKind::Added),
        "staged additions should be grouped in staged_files"
    );
    assert!(
        snapshot
            .unstaged_files
            .iter()
            .any(|file| file.path == "src/main.rs" && file.status == GitChangeKind::Modified),
        "tracked edits should be grouped in unstaged_files"
    );
    assert!(
        snapshot
            .untracked_files
            .iter()
            .any(|file| file.path == "notes.md" && file.status == GitChangeKind::Added),
        "untracked files should be grouped separately"
    );
}

#[tokio::test]
async fn test_git_diff_and_file_status_cover_staged_and_untracked_files() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("should create src directory");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &["src/main.rs"], "initial commit");

    std::fs::write(root.join("src/staged.rs"), "pub fn staged() {}\n")
        .expect("should write staged file");
    std::fs::write(root.join("notes.md"), "draft line\n").expect("should write untracked file");
    stage_selected(&repo, &["src/staged.rs"]);

    let manager = GitManager::new();

    let staged_status = manager
        .get_file_status(&root.to_string_lossy(), "src/staged.rs")
        .await
        .expect("staged file status should succeed");
    assert_eq!(staged_status.staged_status, Some(GitChangeKind::Added));
    assert_eq!(staged_status.unstaged_status, None);
    assert!(!staged_status.is_untracked);

    let untracked_status = manager
        .get_file_status(&root.to_string_lossy(), "notes.md")
        .await
        .expect("untracked file status should succeed");
    assert_eq!(untracked_status.staged_status, None);
    assert_eq!(untracked_status.unstaged_status, Some(GitChangeKind::Added));
    assert!(untracked_status.is_untracked);

    let staged_diff = manager
        .get_diff(&root.to_string_lossy(), "src/staged.rs", true)
        .await
        .expect("staged diff should succeed");
    assert_eq!(staged_diff.status, GitChangeKind::Added);
    assert!(
        staged_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.text.contains("pub fn staged")),
        "staged diff should include the new file content"
    );

    let untracked_diff = manager
        .get_diff(&root.to_string_lossy(), "notes.md", false)
        .await
        .expect("untracked diff should succeed");
    assert_eq!(untracked_diff.status, GitChangeKind::Added);
    assert!(
        untracked_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.text.contains("draft line")),
        "untracked diff should include the working tree file content"
    );
}

#[tokio::test]
async fn test_git_history_and_refresh_events_follow_snapshot_refresh_lifecycle() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("should create src directory");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &["src/main.rs"], "initial commit");

    std::fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .expect("should update tracked file");

    let manager = GitManager::new();
    let history = manager
        .get_history(&root.to_string_lossy(), Some(10))
        .await
        .expect("history lookup should succeed");
    assert_eq!(
        history.first().map(|commit| commit.summary.as_str()),
        Some("initial commit"),
        "history should return the most recent commit first"
    );

    let mut receiver = manager.subscribe("workspace-1").await;
    let refreshed_snapshot = manager
        .refresh("workspace-1", &root.to_string_lossy())
        .await
        .expect("refresh should succeed");
    assert!(
        refreshed_snapshot
            .unstaged_files
            .iter()
            .any(|file| file.path == "src/main.rs"),
        "refresh should recalculate the latest snapshot"
    );

    let started = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("should receive refresh_started")
        .expect("channel should stay open");
    let updated = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("should receive snapshot_updated")
        .expect("channel should stay open");
    let completed = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("should receive refresh_completed")
        .expect("channel should stay open");

    assert!(matches!(
        started,
        GitStreamEvent::RefreshStarted { ref workspace_id } if workspace_id == "workspace-1"
    ));
    assert!(matches!(
        updated,
        GitStreamEvent::SnapshotUpdated { ref workspace_id, ref snapshot }
            if workspace_id == "workspace-1" && snapshot.workspace_id == "workspace-1"
    ));
    assert!(matches!(
        completed,
        GitStreamEvent::RefreshCompleted { ref workspace_id } if workspace_id == "workspace-1"
    ));
}

#[tokio::test]
async fn test_git_stage_and_unstage_move_files_between_snapshot_groups() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("should create src directory");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("should write tracked file");

    let repo = Repository::init(root).expect("should init repository");
    commit_selected(&repo, &["src/main.rs"], "initial commit");

    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"tracked\");\n}\n",
    )
    .expect("should modify tracked file");
    std::fs::write(root.join("notes.md"), "draft\n").expect("should write untracked file");

    let manager = GitManager::new();

    let staged_snapshot = manager
        .stage(
            "workspace-1",
            &root.to_string_lossy(),
            &["src/main.rs".to_string(), "notes.md".to_string()],
        )
        .await
        .expect("stage should succeed");

    assert!(
        staged_snapshot
            .staged_files
            .iter()
            .any(|file| file.path == "src/main.rs" && file.status == GitChangeKind::Modified),
        "tracked file should move into staged group"
    );
    assert!(
        staged_snapshot
            .staged_files
            .iter()
            .any(|file| file.path == "notes.md" && file.status == GitChangeKind::Added),
        "untracked file should move into staged group as an addition"
    );
    assert!(
        !staged_snapshot
            .unstaged_files
            .iter()
            .any(|file| file.path == "src/main.rs"),
        "staged tracked file should leave the tracked group"
    );
    assert!(
        !staged_snapshot
            .untracked_files
            .iter()
            .any(|file| file.path == "notes.md"),
        "staged untracked file should leave the untracked group"
    );

    let unstaged_snapshot = manager
        .unstage(
            "workspace-1",
            &root.to_string_lossy(),
            &["src/main.rs".to_string(), "notes.md".to_string()],
        )
        .await
        .expect("unstage should succeed");

    assert!(
        unstaged_snapshot
            .unstaged_files
            .iter()
            .any(|file| file.path == "src/main.rs" && file.status == GitChangeKind::Modified),
        "tracked file should return to the tracked group after unstage"
    );
    assert!(
        unstaged_snapshot
            .untracked_files
            .iter()
            .any(|file| file.path == "notes.md" && file.status == GitChangeKind::Added),
        "new file should return to the untracked group after unstage"
    );
}

#[tokio::test]
async fn test_git_commit_refreshes_snapshot_and_history() {
    if !git_cli_available() {
        return;
    }

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let root = tmp.path();

    std::fs::write(root.join("README.md"), "# Demo\n").expect("should write readme");

    let repo = Repository::init(root).expect("should init repository");
    configure_git_user(root);
    commit_selected(&repo, &["README.md"], "initial commit");

    std::fs::write(root.join("README.md"), "# Demo\n\nupdated\n").expect("should update readme");
    stage_selected(&repo, &["README.md"]);

    let manager = GitManager::new();
    let (result, snapshot) = manager
        .commit(
            "workspace-1",
            &root.to_string_lossy(),
            "docs: update readme",
        )
        .await
        .expect("git commit should succeed");

    assert_eq!(result.action.as_str(), "commit");
    assert_eq!(result.summary, "Committed staged changes");
    assert!(
        snapshot.staged_files.is_empty(),
        "staged changes should be cleared after commit"
    );
    assert_eq!(
        snapshot
            .recent_commits
            .first()
            .map(|commit| commit.summary.as_str()),
        Some("docs: update readme"),
        "latest history entry should reflect the new commit"
    );
}

#[tokio::test]
async fn test_git_fetch_pull_and_push_round_trip_against_local_remote() {
    if !git_cli_available() {
        return;
    }

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");
    let peer = tmp.path().join("peer");

    Command::new("git")
        .args(["init", "--bare", remote.to_string_lossy().as_ref()])
        .status()
        .expect("should init bare remote");

    run_git(
        tmp.path(),
        &[
            "clone",
            remote.to_string_lossy().as_ref(),
            local.to_string_lossy().as_ref(),
        ],
    );
    configure_git_user(&local);
    std::fs::write(local.join("README.md"), "# Demo\n").expect("should write initial file");
    run_git(&local, &["add", "README.md"]);
    run_git(&local, &["commit", "-m", "initial commit"]);
    run_git(&local, &["push", "-u", "origin", "HEAD"]);

    run_git(
        tmp.path(),
        &[
            "clone",
            remote.to_string_lossy().as_ref(),
            peer.to_string_lossy().as_ref(),
        ],
    );
    configure_git_user(&peer);
    std::fs::write(peer.join("peer.txt"), "peer change\n").expect("should write peer file");
    run_git(&peer, &["add", "peer.txt"]);
    run_git(&peer, &["commit", "-m", "peer change"]);
    run_git(&peer, &["push"]);

    let manager = GitManager::new();

    let (fetch_result, fetched_snapshot) = manager
        .fetch("workspace-1", &local.to_string_lossy())
        .await
        .expect("git fetch should succeed");
    assert_eq!(fetch_result.action.as_str(), "fetch");
    assert!(
        fetched_snapshot.behind_count >= 1,
        "fetch should update ahead/behind metadata after the remote moves"
    );

    let (pull_result, pulled_snapshot) = manager
        .pull("workspace-1", &local.to_string_lossy())
        .await
        .expect("git pull should succeed");
    assert_eq!(pull_result.action.as_str(), "pull");
    assert_eq!(pulled_snapshot.behind_count, 0);
    assert_eq!(
        pulled_snapshot
            .recent_commits
            .first()
            .map(|commit| commit.summary.as_str()),
        Some("peer change"),
        "pull should bring the peer commit into local history"
    );

    std::fs::write(local.join("local.txt"), "local change\n").expect("should write local file");
    run_git(&local, &["add", "local.txt"]);

    manager
        .commit("workspace-1", &local.to_string_lossy(), "local push change")
        .await
        .expect("local commit should succeed");

    let (push_result, pushed_snapshot) = manager
        .push("workspace-1", &local.to_string_lossy())
        .await
        .expect("git push should succeed");
    assert_eq!(push_result.action.as_str(), "push");
    assert_eq!(pushed_snapshot.ahead_count, 0);

    let remote_subject = run_git(
        tmp.path(),
        &[
            "--git-dir",
            remote.to_string_lossy().as_ref(),
            "log",
            "-1",
            "--pretty=%s",
        ],
    );
    assert_eq!(remote_subject.trim(), "local push change");
}

fn commit_selected(repo: &Repository, paths: &[&str], message: &str) {
    let mut index = repo.index().expect("should get repository index");

    for path in paths {
        index
            .add_path(Path::new(path))
            .expect("should stage selected path");
    }
    index.write().expect("should write index");

    let tree_id = index.write_tree().expect("should write tree");
    let tree = repo.find_tree(tree_id).expect("should find tree");
    let signature = Signature::now("TiyCode", "tests@tiy.local").expect("should create signature");
    let parent_commit = repo.head().ok().and_then(|head| head.peel_to_commit().ok());

    match parent_commit.as_ref() {
        Some(parent) => {
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[parent],
            )
            .expect("should create commit");
        }
        None => {
            repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .expect("should create root commit");
        }
    }
}

fn stage_selected(repo: &Repository, paths: &[&str]) {
    let mut index = repo.index().expect("should get repository index");

    for path in paths {
        index
            .add_path(Path::new(path))
            .expect("should stage selected path");
    }
    index.write().expect("should write index");
}

fn git_cli_available() -> bool {
    Command::new("git")
        .arg("--version")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn configure_git_user(repo_root: &Path) {
    run_git(repo_root, &["config", "user.name", "TiyCode"]);
    run_git(repo_root, &["config", "user.email", "tests@tiy.local"]);
}

fn run_git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git command should launch");

    assert!(
        output.status.success(),
        "git {:?} should succeed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).to_string()
}

fn find_node<'a>(
    node: &'a tiycode_lib::core::index_manager::FileTreeNode,
    target_path: &str,
) -> Option<&'a tiycode_lib::core::index_manager::FileTreeNode> {
    if node.path == target_path {
        return Some(node);
    }

    node.children.as_ref().and_then(|children| {
        children
            .iter()
            .find_map(|child| find_node(child, target_path))
    })
}

fn find_git_state(
    node: &tiycode_lib::core::index_manager::FileTreeNode,
    target_path: &str,
) -> Option<GitFileState> {
    find_node(node, target_path).and_then(|child| child.git_state)
}
