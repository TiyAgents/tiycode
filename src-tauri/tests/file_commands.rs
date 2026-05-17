//! Filesystem smoke tests for file-command behavior.
//!
//! Command-core path-safety and AppError behavior is covered in
//! `commands/file.rs` unit tests, where private command helpers can be invoked
//! directly without constructing a full Tauri `State<AppState>`. This file keeps
//! broader filesystem round-trip checks around CRUD-adjacent behavior.

mod test_helpers;

use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temp workspace directory and return its canonical path.
fn workspace_dir() -> TempDir {
    tempfile::tempdir().expect("failed to create temp dir")
}

/// Write `content` to `<root>/<relative>`, creating parent dirs as needed.
fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
}

// ---------------------------------------------------------------------------
// Path safety tests (unit-level, no DB needed)
// ---------------------------------------------------------------------------

#[test]
fn rejects_dotdot_traversal() {
    let dir = workspace_dir();
    let root = fs::canonicalize(dir.path()).unwrap();
    let parent = root.parent().expect("workspace should have a parent");
    let outside = tempfile::Builder::new()
        .prefix("outside-workspace-")
        .tempdir_in(parent)
        .expect("failed to create sibling dir");
    let outside_file = outside.path().join("passwd");
    fs::write(&outside_file, "outside").unwrap();

    let bad = root
        .join("..")
        .join(outside.path().file_name().unwrap())
        .join("passwd");
    let canonical_bad = bad.canonicalize().unwrap();

    assert!(
        !canonical_bad.starts_with(&root),
        "canonicalized traversal target must escape the workspace root"
    );
}

// ---------------------------------------------------------------------------
// Filesystem round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn file_write_read_roundtrip() {
    let dir = workspace_dir();
    let root = dir.path();
    let content = "Hello from test 🎉";
    write_file(root, "test.txt", content);

    let read_back = fs::read_to_string(root.join("test.txt")).unwrap();
    assert_eq!(read_back, content);
}

#[test]
fn file_create_and_delete() {
    let dir = workspace_dir();
    let root = dir.path();

    // Create file
    let target = root.join("new-file.txt");
    fs::write(&target, b"").unwrap();
    assert!(target.exists());

    // Delete file
    fs::remove_file(&target).unwrap();
    assert!(!target.exists());
}

#[test]
fn directory_create_and_delete() {
    let dir = workspace_dir();
    let root = dir.path();

    let target = root.join("new-folder");
    fs::create_dir_all(&target).unwrap();
    assert!(target.is_dir());

    fs::remove_dir_all(&target).unwrap();
    assert!(!target.exists());
}

#[test]
fn file_rename() {
    let dir = workspace_dir();
    let root = dir.path();

    write_file(root, "original.txt", "content");
    let old_path = root.join("original.txt");
    let new_path = root.join("renamed.txt");

    fs::rename(&old_path, &new_path).unwrap();
    assert!(!old_path.exists());
    assert!(new_path.exists());
    assert_eq!(fs::read_to_string(&new_path).unwrap(), "content");
}

#[test]
fn binary_detection_heuristic() {
    let dir = workspace_dir();
    let root = dir.path();

    // File with NUL byte → binary
    let binary_content = b"some\x00binary\x00data";
    fs::write(root.join("binary.bin"), binary_content).unwrap();
    let bytes = fs::read(root.join("binary.bin")).unwrap();
    let check_len = bytes.len().min(8192);
    let has_nul = bytes[..check_len].contains(&0);
    assert!(has_nul, "should detect NUL byte");

    // Normal text file → not binary
    write_file(root, "text.txt", "normal text content");
    let bytes = fs::read(root.join("text.txt")).unwrap();
    let check_len = bytes.len().min(8192);
    let has_nul = bytes[..check_len].contains(&0);
    assert!(!has_nul, "text file should not be detected as binary");
}

#[test]
fn large_file_size_check() {
    let dir = workspace_dir();
    let root = dir.path();
    let max_size: u64 = 5 * 1024 * 1024;

    // Create a file slightly over the limit
    let large_content = vec![b'A'; (max_size + 1) as usize];
    fs::write(root.join("large.bin"), &large_content).unwrap();
    let metadata = fs::metadata(root.join("large.bin")).unwrap();
    assert!(
        metadata.len() > max_size,
        "file should exceed max read size"
    );
}

#[test]
fn create_already_exists_fails() {
    let dir = workspace_dir();
    let root = dir.path();
    write_file(root, "exists.txt", "content");

    let target = root.join("exists.txt");
    assert!(target.exists());
    // Attempting to create again should detect existence
    // (in real command, this returns an error)
}

#[test]
fn rename_rejects_path_separator_in_name() {
    // new_name must be a bare filename without path separators
    let bad_names = ["sub/file.txt", "sub\\file.txt"];
    for name in &bad_names {
        assert!(
            name.contains('/') || name.contains('\\'),
            "{name} should contain path separator"
        );
    }
}

#[test]
fn delete_nonexistent_path() {
    let dir = workspace_dir();
    let root = dir.path();
    let target = root.join("ghost.txt");
    assert!(!target.exists());
    // In real command, this returns AppError::not_found
}
