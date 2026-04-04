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
        node_modules.is_expandable,
        "heavy directories should remain expandable in the tree"
    );
    assert!(
        node_modules.children.is_none(),
        "heavy directories should stay lazily collapsed for performance"
    );
}

#[tokio::test]
async fn test_file_tree_loads_root_level_only_and_defers_nested_branches() {
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

    assert!(
        src.is_expandable,
        "root-level directories should still be expandable placeholders"
    );
    assert!(
        src.children.is_none(),
        "initial tree should defer child directory materialization"
    );

    let src_children = manager
        .get_children(&base.to_string_lossy(), "src", None, None)
        .await
        .expect("should load root-level directory children on demand");
    let components = src_children
        .children
        .iter()
        .find(|child| child.path == "src/components")
        .expect("second level dir should materialize after expansion");

    assert!(
        components.is_expandable,
        "deep directories should remain expandable placeholders"
    );
    assert!(
        components.children.is_none(),
        "second-level directories should still defer deeper descendants"
    );

    let loaded_children = manager
        .get_children(&base.to_string_lossy(), "src/components", None, None)
        .await
        .expect("should load children on demand");
    let button = loaded_children
        .children
        .iter()
        .find(|child| child.path == "src/components/button")
        .expect("deferred child directory should load");

    assert!(
        button.is_expandable,
        "nested directory should still be expandable"
    );
}

#[tokio::test]
async fn test_large_directories_page_children_without_filtering_them_from_tree() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    for index in 0..205 {
        let package_dir = base.join("node_modules").join(format!("pkg_{index:03}"));
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(package_dir.join("index.js"), "module.exports = {};\n").unwrap();
    }

    let tree = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let node_modules = tree
        .children
        .as_ref()
        .and_then(|children| children.iter().find(|child| child.path == "node_modules"))
        .expect("node_modules should be visible in the tree");

    assert!(
        node_modules.is_expandable,
        "node_modules should be expandable"
    );
    assert!(
        node_modules.children.is_none(),
        "node_modules should defer loading until expanded"
    );

    let first_page = manager
        .get_children(&base.to_string_lossy(), "node_modules", Some(0), Some(200))
        .await
        .expect("should load first page");
    assert_eq!(
        first_page.children.len(),
        200,
        "first page should be capped"
    );
    assert!(
        first_page.has_more,
        "first page should advertise more content"
    );
    assert_eq!(first_page.next_offset, Some(200));

    let second_page = manager
        .get_children(
            &base.to_string_lossy(),
            "node_modules",
            first_page.next_offset,
            Some(200),
        )
        .await
        .expect("should load second page");
    assert_eq!(
        second_page.children.len(),
        5,
        "second page should contain the remainder"
    );
    assert!(
        !second_page.has_more,
        "second page should exhaust the available children"
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
async fn test_search_repo_respects_global_max_results() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(base.join("a.rs"), "fn a() { println!(\"hello\"); }\n").unwrap();
    std::fs::write(base.join("b.rs"), "fn b() { println!(\"hello\"); }\n").unwrap();
    std::fs::write(base.join("c.rs"), "fn c() { println!(\"hello\"); }\n").unwrap();

    let result = manager
        .search(&base.to_string_lossy(), "hello", None, Some(1))
        .await;

    match result {
        Ok(response) => {
            assert_eq!(
                response.results.len(),
                1,
                "preview should honor max_results"
            );
            assert_eq!(response.count, 1, "count should match the returned preview");
        }
        Err(e) => {
            let err_msg = format!("{e}");
            assert!(
                err_msg.contains("not found") || err_msg.contains("No such file"),
                "If search fails, it should be because ripgrep is not installed: {e}"
            );
        }
    }
}

#[tokio::test]
async fn test_search_repo_path_based_file_pattern_is_resolved_from_workspace_root() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::create_dir_all(base.join("src/components")).unwrap();
    std::fs::write(
        base.join("src/components/widget.tsx"),
        "export const label = 'needle';\n",
    )
    .unwrap();

    let result = manager
        .search(
            &base.to_string_lossy(),
            "needle",
            Some("src/components/widget.tsx"),
            Some(20),
        )
        .await;

    match result {
        Ok(response) => {
            assert_eq!(
                response.count, 1,
                "path-based filePattern should still match"
            );
            assert_eq!(response.results.len(), 1, "should return the matching file");
            assert_eq!(response.results[0].path, "src/components/widget.tsx");
        }
        Err(e) => {
            let err_msg = format!("{e}");
            assert!(
                err_msg.contains("not found") || err_msg.contains("No such file"),
                "If search fails, it should be because ripgrep is not installed: {e}"
            );
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
    let src = tree
        .children
        .as_ref()
        .and_then(|children| children.iter().find(|child| child.path == "src"))
        .expect("src should be part of the root tree");
    assert!(
        src.children.is_none(),
        "nested directories should stay unloaded until expansion"
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

#[tokio::test]
async fn test_reveal_path_materializes_new_file_in_loaded_directory() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::write(
        base.join("src/existing.ts"),
        "export const existing = true;\n",
    )
    .unwrap();

    let tree = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let src = tree
        .children
        .as_ref()
        .and_then(|children| children.iter().find(|child| child.path == "src"))
        .expect("src should be present in the initial tree");
    assert!(
        src.children.is_none(),
        "initial tree should leave nested directory contents unloaded"
    );

    std::fs::write(base.join("src/new.ts"), "export const fresh = true;\n").unwrap();

    let reveal = manager
        .reveal_path(&base.to_string_lossy(), "src/new.ts")
        .await
        .expect("reveal should materialize the new file");

    let src_segment = reveal
        .segments
        .iter()
        .find(|segment| segment.directory_path == "src")
        .expect("src segment should be returned");
    assert!(
        src_segment
            .children
            .iter()
            .any(|child| child.path == "src/new.ts"),
        "reveal should return the newly added child for a previously loaded directory"
    );
}

#[tokio::test]
async fn test_reveal_path_materializes_root_level_file() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    std::fs::write(base.join("existing.md"), "# existing\n").unwrap();
    let tree = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    assert!(
        tree.children
            .as_ref()
            .is_some_and(|children| children.iter().all(|child| child.path != "root-new.md")),
        "new root file should not appear in the stale tree snapshot"
    );

    std::fs::write(base.join("root-new.md"), "# new\n").unwrap();

    let reveal = manager
        .reveal_path(&base.to_string_lossy(), "root-new.md")
        .await
        .expect("reveal should materialize a root-level file");

    assert_eq!(reveal.target_path, "root-new.md");
    let root_segment = reveal
        .segments
        .iter()
        .find(|segment| segment.directory_path.is_empty())
        .expect("root segment should be returned");
    assert!(
        root_segment
            .children
            .iter()
            .any(|child| child.path == "root-new.md"),
        "reveal should return root-level files through the root segment"
    );
}

#[tokio::test]
async fn test_reveal_path_pages_until_large_directory_target_is_found() {
    use tiy_agent_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    let large_dir = base.join("large");
    std::fs::create_dir_all(&large_dir).unwrap();
    for index in 0..205 {
        std::fs::write(
            large_dir.join(format!("file_{index:03}.ts")),
            "export const value = 1;\n",
        )
        .unwrap();
    }

    let tree = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let large = tree
        .children
        .as_ref()
        .and_then(|children| children.iter().find(|child| child.path == "large"))
        .expect("large directory should be present");
    assert!(
        large.is_expandable,
        "large directories should still advertise expandability in the initial tree"
    );
    assert!(
        large.children.is_none(),
        "initial tree should not preload paged directory entries"
    );

    let reveal = manager
        .reveal_path(&base.to_string_lossy(), "large/file_204.ts")
        .await
        .expect("reveal should page until the target child is included");

    let large_segment = reveal
        .segments
        .iter()
        .find(|segment| segment.directory_path == "large")
        .expect("large segment should be returned");
    assert_eq!(
        large_segment.children.len(),
        205,
        "reveal should merge additional pages until the target child is present"
    );
    assert!(
        large_segment
            .children
            .iter()
            .any(|child| child.path == "large/file_204.ts"),
        "reveal should include the requested target child from later pages"
    );
    assert!(
        !large_segment.has_more,
        "the reveal segment should report that no additional pages remain once exhausted"
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
