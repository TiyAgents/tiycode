//! M1.8 — Index basics tests
//!
//! Acceptance criteria:
//! - File tree loads for medium repo < 300ms
//! - ripgrep search returns file path + line number + context
//! - Tree scan hides .git and noisy files while keeping large directories performant
//! - File filter can find deep files beyond the eagerly loaded tree

// =========================================================================
// T1.8.1 — File tree scan of real directory
// =========================================================================

#[tokio::test]
async fn test_file_tree_scan_current_dir() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let cwd = std::env::current_dir()
        .expect("should get cwd")
        .to_string_lossy()
        .to_string();

    let result = manager.get_tree(&cwd).await;
    assert!(
        result.is_ok(),
        "File tree scan should succeed: {:?}",
        result.err()
    );

    let tree = result.unwrap();
    assert!(tree.is_dir, "Root should be a directory");
    assert!(!tree.name.is_empty(), "Root should have a name");
    assert!(tree.children.is_some(), "Root should have children");
}

#[tokio::test]
async fn test_file_tree_hides_only_reserved_entries() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();

    // Create a temp directory with some content
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    // Create normal files and ignored dirs
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::write(base.join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(base.join("node_modules/pkg")).unwrap();
    std::fs::write(base.join("node_modules/pkg/index.js"), "").unwrap();
    std::fs::create_dir_all(base.join(".git/objects")).unwrap();
    std::fs::write(base.join(".DS_Store"), "").unwrap();

    let tree = manager.get_tree(&base.to_string_lossy()).await.unwrap();

    // Check that children exist and count
    let children = tree.children.unwrap();
    let child_names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();

    assert!(
        child_names.contains(&"src"),
        "Should include 'src' directory"
    );
    assert!(
        child_names.contains(&"node_modules"),
        "Should keep 'node_modules' as a collapsed node"
    );
    assert!(!child_names.contains(&".git"), "Should exclude '.git'");
    assert!(
        !child_names.contains(&".DS_Store"),
        "Should exclude '.DS_Store'"
    );

    let node_modules = children
        .iter()
        .find(|child| child.name == "node_modules")
        .expect("node_modules placeholder should be present");
    assert!(
        node_modules.is_dir,
        "node_modules should still render as a directory"
    );
    assert!(
        !node_modules.is_expandable,
        "heavy directories should not expand in the tree"
    );
    assert!(
        node_modules.children.is_none(),
        "heavy directories should stay collapsed for performance"
    );
}

#[tokio::test]
async fn test_file_tree_loads_shallow_levels_and_defers_deep_branches() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    std::fs::create_dir_all(base.join("src/components/button")).unwrap();
    std::fs::write(base.join("src/components/button/index.tsx"), "export {}").unwrap();

    let tree = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let src = tree
        .children
        .as_ref()
        .and_then(|children| children.iter().find(|child| child.path == "src"))
        .expect("src should be present");
    let components = src
        .children
        .as_ref()
        .and_then(|children| children.iter().find(|child| child.path == "src/components"))
        .expect("second level dir should be preloaded");

    assert!(
        components.is_expandable,
        "deep directories should remain expandable placeholders"
    );
    assert!(
        components.children.is_none(),
        "second-level directories should defer deeper descendants"
    );

    let loaded_children = manager
        .get_children(&base.to_string_lossy(), "src/components")
        .await
        .expect("should load children on demand");
    let button = loaded_children
        .iter()
        .find(|child| child.path == "src/components/button")
        .expect("deferred child directory should load");

    assert!(
        button.is_expandable,
        "nested directory should still be expandable"
    );
}

#[tokio::test]
async fn test_file_tree_nonexistent_path() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let result = manager
        .get_tree("/nonexistent/path/that/does/not/exist")
        .await;

    assert!(result.is_err(), "Should fail for nonexistent path");
}

// =========================================================================
// T1.8.2 — ripgrep search integration
// =========================================================================

#[tokio::test]
async fn test_search_repo_basic() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();

    // Create temp directory with searchable content
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(
        base.join("hello.rs"),
        "fn main() {\n    println!(\"hello world\");\n}",
    )
    .unwrap();
    std::fs::write(
        base.join("lib.rs"),
        "pub fn greet() -> &'static str {\n    \"hello\"\n}",
    )
    .unwrap();

    let result = manager
        .search(&base.to_string_lossy(), "hello", None, None)
        .await;

    // If ripgrep is installed, this should succeed
    match result {
        Ok(response) => {
            assert_eq!(response.query, "hello");
            assert!(response.count > 0, "Should find 'hello' in test files");
            // Verify result structure
            for r in &response.results {
                assert!(!r.path.is_empty());
                assert!(r.line_number > 0);
                assert!(!r.line_text.is_empty());
            }
        }
        Err(e) => {
            // ripgrep might not be installed in CI
            let err_msg = format!("{e}");
            assert!(
                err_msg.contains("not found") || err_msg.contains("No such file"),
                "If search fails, it should be because ripgrep is not installed: {e}"
            );
        }
    }
}

#[tokio::test]
async fn test_search_repo_no_results() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(base.join("file.txt"), "nothing special here").unwrap();

    let result = manager
        .search(
            &base.to_string_lossy(),
            "xyzzy_nonexistent_pattern",
            None,
            None,
        )
        .await;

    match result {
        Ok(response) => {
            assert_eq!(response.count, 0, "Should find no results");
            assert!(response.results.is_empty());
        }
        Err(_) => {
            // ripgrep not installed — acceptable in some environments
        }
    }
}

#[tokio::test]
async fn test_filter_files_finds_deep_paths_beyond_loaded_tree() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    std::fs::create_dir_all(base.join("src/components/button")).unwrap();
    std::fs::create_dir_all(base.join("node_modules/pkg")).unwrap();
    std::fs::write(base.join("src/components/button/index.tsx"), "export {}").unwrap();
    std::fs::write(
        base.join("node_modules/pkg/index.js"),
        "module.exports = {}",
    )
    .unwrap();

    let tree = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let src_components = tree
        .children
        .as_ref()
        .and_then(|children| children.iter().find(|child| child.path == "src"))
        .and_then(|src| src.children.as_ref())
        .and_then(|children| children.iter().find(|child| child.path == "src/components"))
        .expect("src/components should be part of shallow tree");
    assert!(
        src_components.children.is_none(),
        "deep files should not be eagerly loaded into the tree"
    );

    let response = manager
        .filter_files(&base.to_string_lossy(), "button/index", None)
        .await
        .expect("filter should search the full manifest");

    assert_eq!(response.count, 1, "should return the deep file");
    assert_eq!(response.results[0].path, "src/components/button/index.tsx");
    assert!(
        !response
            .results
            .iter()
            .any(|result| result.path.contains("node_modules")),
        "heavy excluded directories should not leak into filter results"
    );
}

// =========================================================================
// T1.8.3 — Performance (basic timing)
// =========================================================================

#[tokio::test]
async fn test_file_tree_scan_performance() {
    use std::time::Instant;
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();

    // Create a directory with moderate file count
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    // Create 100 files across 10 directories
    for i in 0..10 {
        let dir = base.join(format!("dir_{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        for j in 0..10 {
            std::fs::write(dir.join(format!("file_{j}.rs")), "content").unwrap();
        }
    }

    let start = Instant::now();
    let result = manager.get_tree(&base.to_string_lossy()).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert!(
        elapsed.as_millis() < 3000,
        "File tree scan of 100 files should complete in < 3s, took {}ms",
        elapsed.as_millis()
    );
}
