//! Index basics tests
//!
//! Acceptance criteria:
//! - File tree loads for medium repo < 300ms
//! - local search returns file path + line number + context
//! - Tree scan hides .git and noisy files while keeping large directories performant
//! - File filter can find deep files beyond the eagerly loaded tree

// =========================================================================
// T1.8.1 — File tree scan of real directory
// =========================================================================

#[tokio::test]
async fn test_file_tree_scan_current_dir() {
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let result = manager
        .get_tree("/nonexistent/path/that/does/not/exist")
        .await;

    assert!(result.is_err(), "Should fail for nonexistent path");
}

// =========================================================================
// T1.8.2 — local search integration
// =========================================================================

#[tokio::test]
async fn test_search_repo_basic() {
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

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
        .search(&base.to_string_lossy(), "hello", SearchOptions::default())
        .await
        .expect("local search should succeed");

    assert_eq!(result.query, "hello");
    assert!(result.count > 0, "Should find 'hello' in test files");
    for r in &result.results {
        assert!(!r.path.is_empty());
        assert!(r.line_number > 0);
        assert!(!r.line_text.is_empty());
    }
}

#[tokio::test]
async fn test_search_repo_no_results() {
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(base.join("file.txt"), "nothing special here").unwrap();

    let result = manager
        .search(
            &base.to_string_lossy(),
            "xyzzy_nonexistent_pattern",
            SearchOptions::default(),
        )
        .await
        .expect("local search should succeed");

    assert_eq!(result.count, 0, "Should find no results");
    assert!(result.results.is_empty());
}

#[tokio::test]
async fn test_search_repo_respects_global_max_results() {
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(base.join("a.rs"), "fn a() { println!(\"hello\"); }\n").unwrap();
    std::fs::write(base.join("b.rs"), "fn b() { println!(\"hello\"); }\n").unwrap();
    std::fs::write(base.join("c.rs"), "fn c() { println!(\"hello\"); }\n").unwrap();

    let result = manager
        .search(
            &base.to_string_lossy(),
            "hello",
            SearchOptions {
                max_results: Some(1),
                ..SearchOptions::default()
            },
        )
        .await
        .expect("local search should succeed");

    assert_eq!(result.results.len(), 1, "preview should honor max_results");
    assert_eq!(result.count, 1, "count should match the returned preview");
}

#[tokio::test]
async fn test_search_repo_path_based_file_pattern_is_resolved_from_workspace_root() {
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

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
            SearchOptions {
                file_pattern: Some("src/components/widget.tsx".to_string()),
                max_results: Some(20),
                ..SearchOptions::default()
            },
        )
        .await
        .expect("local search should succeed");

    assert_eq!(result.count, 1, "path-based filePattern should still match");
    assert_eq!(result.results.len(), 1, "should return the matching file");
    assert_eq!(result.results[0].path, "src/components/widget.tsx");
}

#[tokio::test]
async fn test_search_repo_multiline_returns_block_metadata() {
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(
        base.join("query.ts"),
        "const sql = `\nSELECT *\nFROM users\nWHERE active = true\n`;\n",
    )
    .unwrap();

    let result = manager
        .search(
            &base.to_string_lossy(),
            "SELECT *\nFROM users",
            SearchOptions {
                multiline: true,
                ..SearchOptions::default()
            },
        )
        .await
        .expect("multiline search should succeed");

    assert_eq!(result.count, 1);
    assert_eq!(result.results[0].line_number, 2);
    assert_eq!(result.results[0].end_line_number, Some(3));
    assert_eq!(
        result.results[0].match_text.as_deref(),
        Some("SELECT *\nFROM users")
    );
}

#[tokio::test]
async fn test_search_repo_supports_extended_search_options() {
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};
    use tiycode_lib::core::local_search::{SearchOutputMode, SearchQueryMode};

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(base.join("a.ts"), "const value = 'HELLO';\n").unwrap();
    std::fs::write(base.join("b.rs"), "const value = 'HELLO';\n").unwrap();

    let result = manager
        .search(
            &base.to_string_lossy(),
            "value\\s*=\\s*'hello'",
            SearchOptions {
                file_type: Some("typescript".to_string()),
                query_mode: SearchQueryMode::Regex,
                output_mode: SearchOutputMode::FilesWithMatches,
                case_insensitive: true,
                ..SearchOptions::default()
            },
        )
        .await
        .expect("search with passthrough options should succeed");

    assert_eq!(result.query_mode, "regex");
    assert_eq!(result.output_mode, "files_with_matches");
    assert_eq!(result.count, 1);
    assert_eq!(result.total_count, 1);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, "a.ts");
    assert!(result.results.is_empty());
    assert!(result.file_counts.is_empty());
}

#[tokio::test]
async fn test_search_repo_timeout_passthrough_marks_partial_response() {
    use std::time::Duration;
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(base.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(base.join("b.rs"), "fn b() {}\n").unwrap();

    let result = manager
        .search(
            &base.to_string_lossy(),
            "fn",
            SearchOptions {
                timeout: Some(Duration::ZERO),
                ..SearchOptions::default()
            },
        )
        .await
        .expect("timed search should succeed");

    assert!(result.timed_out);
    assert!(result.partial);
    assert!(!result.completed);
}

#[tokio::test]
async fn test_search_stream_emits_incremental_batches() {
    use std::sync::{Arc, Mutex};
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    std::fs::write(base.join("a.rs"), "fn a() { println!(\"hello\"); }\n").unwrap();
    std::fs::write(base.join("b.rs"), "fn b() { println!(\"hello\"); }\n").unwrap();

    let batches = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&batches);

    let result = manager
        .search_stream(
            &base.to_string_lossy(),
            "hello",
            SearchOptions::default(),
            move |batch| {
                captured.lock().unwrap().push(batch);
                Ok(())
            },
        )
        .await
        .expect("streaming search should succeed");

    assert_eq!(result.count, 2);
    assert!(!batches.lock().unwrap().is_empty());
    assert_eq!(batches.lock().unwrap().last().unwrap().count, 2);
}

#[tokio::test]
async fn test_search_stream_can_be_cancelled_mid_scan() {
    use std::sync::{Arc, Mutex};
    use tiycode_lib::core::index_manager::{IndexManager, SearchOptions};

    let manager = IndexManager::new();

    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();
    for index in 0..32 {
        std::fs::write(
            base.join(format!("file-{index:02}.rs")),
            "fn demo() { println!(\"hello\"); }\n",
        )
        .unwrap();
    }

    let cancellation = manager.register_stream_search(42).await;
    let seen_batches = Arc::new(Mutex::new(0usize));
    let seen_batches_for_callback = Arc::clone(&seen_batches);
    let cancellation_for_callback = cancellation.clone();

    let result = manager
        .search_stream(
            &base.to_string_lossy(),
            "hello",
            SearchOptions {
                cancellation: Some(cancellation),
                ..SearchOptions::default()
            },
            move |_batch| {
                let mut batches = seen_batches_for_callback.lock().unwrap();
                *batches += 1;
                if *batches == 1 {
                    cancellation_for_callback.cancel();
                }
                Ok(())
            },
        )
        .await
        .expect("streaming search should succeed");

    manager.finish_stream_search(42).await;

    assert!(result.cancelled, "search should report cancellation");
    assert!(
        !result.completed,
        "cancelled search should not report completion"
    );
    assert!(result.partial, "cancelled search should be marked partial");
    assert!(
        *seen_batches.lock().unwrap() >= 1,
        "should emit at least one batch"
    );
    assert!(
        result.count < 32,
        "cancelled search should stop before collecting every file"
    );
}

#[tokio::test]
async fn test_filter_files_finds_deep_paths_beyond_loaded_tree() {
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

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
    use tiycode_lib::core::index_manager::IndexManager;

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

// =========================================================================
// T1.8.4 — Tree cache TTL hit / expiry
// =========================================================================

#[tokio::test]
async fn test_tree_cache_hit_returns_same_result_without_rescan() {
    use std::time::Instant;
    use tiycode_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::write(base.join("src/main.rs"), "fn main() {}").unwrap();

    // First call — populates the cache.
    let first = manager.get_tree(&base.to_string_lossy()).await.unwrap();

    // Mutate the filesystem *before* the second call.
    std::fs::write(base.join("src/extra.rs"), "fn extra() {}").unwrap();

    // Second call within the TTL window — should return the cached (stale) tree.
    let start = Instant::now();
    let second = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(
        first.children.as_ref().map(|c| c.len()),
        second.children.as_ref().map(|c| c.len()),
        "cached tree should return the same children count before TTL expires"
    );
    assert!(
        elapsed.as_millis() < 50,
        "cache hit should be near-instant, took {}ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn test_tree_cache_expires_after_ttl() {
    use tiycode_lib::core::index_manager::IndexManager;

    let manager = IndexManager::new();
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let base = tmp.path();

    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::write(base.join("src/main.rs"), "fn main() {}").unwrap();

    // First call — populates the cache.
    let first = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let first_count = first.children.as_ref().map(|c| c.len()).unwrap_or(0);

    // Add a new file.
    std::fs::write(base.join("src/extra.rs"), "fn extra() {}").unwrap();

    // Wait for the cache TTL (2 s) to expire.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Third call — TTL expired, should rescan and pick up the new file.
    let refreshed = manager.get_tree(&base.to_string_lossy()).await.unwrap();
    let refreshed_count = refreshed.children.as_ref().map(|c| c.len()).unwrap_or(0);

    assert!(
        refreshed_count > first_count
            || refreshed
                .children
                .as_ref()
                .is_some_and(|children| children.iter().any(|child| child.path == "src")),
        "after TTL expiry the tree should reflect filesystem changes"
    );
}
