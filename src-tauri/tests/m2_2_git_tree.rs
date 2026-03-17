//! M2.2a — Git-backed TreeView tests

use std::path::Path;

use git2::{Repository, Signature};
use tiy_agent_lib::core::git_manager::GitManager;
use tiy_agent_lib::core::index_manager::{GitFileState, IndexManager};

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
async fn test_git_overlay_marks_tracked_untracked_and_ignored_nodes() {
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
        Some(GitFileState::Tracked),
        "tracked directory should be marked as tracked"
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
    let signature =
        Signature::now("Tiy Agent", "tests@tiy.local").expect("should create signature");

    repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
        .expect("should create commit");
}

fn find_node<'a>(
    node: &'a tiy_agent_lib::core::index_manager::FileTreeNode,
    target_path: &str,
) -> Option<&'a tiy_agent_lib::core::index_manager::FileTreeNode> {
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
    node: &tiy_agent_lib::core::index_manager::FileTreeNode,
    target_path: &str,
) -> Option<GitFileState> {
    find_node(node, target_path).and_then(|child| child.git_state)
}
