//! Tests for the extensions module — ConfigScope, MCP server CRUD,
//! extension listing, diagnostics, and marketplace basics.
//!
//! Uses in-memory SQLite pool via setup_test_pool().

mod test_helpers;
use test_helpers::setup_test_pool;

use tiycode::extensions::{ConfigScope, ExtensionsManager};
use tiycode::model::extensions::McpServerConfigInput;

fn make_mcp_input(name: &str) -> McpServerConfigInput {
    McpServerConfigInput {
        id: format!("test-{name}"),
        label: name.to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        auto_start: false,
        command: Some("node".to_string()),
        args: Some(vec![format!("mcp-{name}")]),
        env: None,
        cwd: None,
        url: None,
        headers: None,
        timeout_ms: Some(15000),
    }
}

// ─── ConfigScope pure functions ──────────────────────────────────────────

#[test]
fn config_scope_from_option_defaults_to_global() {
    assert_eq!(ConfigScope::from_option(None), ConfigScope::Global);
}

#[test]
fn config_scope_from_option_workspace() {
    assert_eq!(
        ConfigScope::from_option(Some("workspace")),
        ConfigScope::Workspace
    );
}

#[test]
fn config_scope_from_option_unknown_falls_back_to_global() {
    assert_eq!(
        ConfigScope::from_option(Some("invalid")),
        ConfigScope::Global
    );
}

#[test]
fn config_scope_from_str_global() {
    assert_eq!(ConfigScope::from_str("global"), ConfigScope::Global);
}

#[test]
fn config_scope_from_str_workspace() {
    assert_eq!(ConfigScope::from_str("workspace"), ConfigScope::Workspace);
}

#[test]
fn config_scope_from_str_unknown_falls_back() {
    assert_eq!(ConfigScope::from_str("anything-else"), ConfigScope::Global);
}

// ─── ExtensionsManager basic lifecycle ─────────────────────────────────

#[tokio::test]
async fn extensions_manager_new_and_diagnostics() {
    let pool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool);

    let diag = mgr.list_config_diagnostics();
    assert!(
        diag.is_empty(),
        "fresh manager: no diagnostics, got {:?}",
        diag
    );
}

#[tokio::test]
async fn list_extensions_returns_vec() {
    let pool: sqlx::SqlitePool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool.clone());

    // Just verify it doesn't panic and returns a Vec
    let exts = mgr
        .list_extensions(None, ConfigScope::Global)
        .await
        .expect("list");
    let _ = exts.len(); // may or may not be empty depending on env
}

#[tokio::test]
async fn get_extension_detail_nonexistent_errors() {
    let pool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool);

    let result = mgr
        .get_extension_detail("nonexistent", None, ConfigScope::Global)
        .await;
    assert!(result.is_err());
}

// ─── MCP Server CRUD (each test uses unique IDs) ──────────────────────

#[tokio::test]
async fn mcp_add_list_remove_roundtrip() {
    let pool: sqlx::SqlitePool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool.clone());

    let input = make_mcp_input("roundtrip1");

    let added = mgr
        .add_mcp_server(input, None, ConfigScope::Global)
        .await
        .expect("add");
    assert_eq!(added.label, "roundtrip1");
    let server_id = added.id;

    // List — at least our one
    let servers = mgr
        .list_mcp_servers(None, ConfigScope::Global)
        .await
        .expect("list after add");
    assert!(servers.iter().any(|s| s.id == server_id));

    // Update
    let mut update_input = make_mcp_input("updated1");
    update_input.id = server_id.clone();
    update_input.label = "renamed".to_string();

    let updated = mgr
        .update_mcp_server(&server_id, update_input, None, ConfigScope::Global)
        .await
        .expect("update");
    assert_eq!(updated.label, "renamed");

    // Remove
    let removed = mgr
        .remove_mcp_server(&server_id, None, ConfigScope::Global)
        .await
        .expect("remove");
    assert!(removed);
}

#[tokio::test]
async fn mcp_update_nonexistent_errors() {
    let pool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool);

    let mut input = make_mcp_input("nope");
    input.id = "ghost-id".to_string();
    let id = input.id.clone();
    let result = mgr
        .update_mcp_server(&id, input, None, ConfigScope::Global)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn mcp_remove_nonexistent_does_not_panic() {
    let pool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool);

    // May return Ok(false) or Err depending on implementation
    // Just verify no panic
    let _ = mgr
        .remove_mcp_server("totally-ghost-id", None, ConfigScope::Global)
        .await;
}

#[tokio::test]
async fn mcp_add_multiple_servers_all_listed() {
    let pool: sqlx::SqlitePool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool.clone());

    let a = mgr
        .add_mcp_server(make_mcp_input("multi-a"), None, ConfigScope::Global)
        .await
        .expect("add a")
        .id;
    let b = mgr
        .add_mcp_server(make_mcp_input("multi-b"), None, ConfigScope::Global)
        .await
        .expect("add b")
        .id;

    let servers = mgr
        .list_mcp_servers(None, ConfigScope::Global)
        .await
        .expect("list");
    assert!(servers.iter().any(|s| s.id == a));
    assert!(servers.iter().any(|s| s.id == b));
}

// ─── Skills ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_skills_returns_vec() {
    let pool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool);

    let skills = mgr
        .list_skills(None, ConfigScope::Global)
        .await
        .expect("list skills");
    let _ = skills.len(); // just verify it returns successfully
}

// ─── Marketplace ─────────────────────────────────────────────────────────

#[tokio::test]
async fn marketplace_sources_includes_builtin() {
    let pool = setup_test_pool().await;
    let mgr = ExtensionsManager::new(pool);

    let sources = mgr.marketplace_list_sources().await.expect("list sources");
    assert!(
        !sources.is_empty(),
        "should have built-in marketplace sources"
    );
    assert!(
        sources.iter().any(|s| s.name.contains("Anthropic")),
        "should include Anthropic source"
    );
}
