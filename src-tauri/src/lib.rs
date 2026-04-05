mod commands;
pub mod core;
pub mod extensions;
pub mod ipc;
pub mod model;
mod persistence;

use std::fs;
use std::path::PathBuf;

use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::core::app_state::AppState;
use crate::core::sleep_manager::PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY;

const MAIN_WINDOW_LABEL: &str = "main";

fn persisted_window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN
}

/// Resolve the `$HOME/.tiy/` base directory.
fn tiy_home() -> PathBuf {
    dirs::home_dir()
        .expect("cannot resolve HOME directory")
        .join(".tiy")
}

/// Resolve the platform-native log directory for TiyCode.
fn log_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .expect("cannot resolve HOME directory")
            .join("Library/Logs/TiyAgent")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir()
            .expect("cannot resolve LOCALAPPDATA")
            .join("TiyAgent/logs")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::state_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
            .expect("cannot resolve state directory")
            .join("tiy-agent/logs")
    }
}

/// Create all required directories under `$HOME/.tiy/` and the log directory.
fn init_directories(base: &PathBuf) -> std::io::Result<()> {
    let dirs_to_create = [
        base.join("db"),
        base.join("db/backups"),
        base.join("skills"),
        base.join("prompts"),
        base.join("plugins"),
        base.join("automations"),
        base.join("cache"),
        base.join("cache/index"),
        base.join("catalog"),
    ];

    for dir in &dirs_to_create {
        fs::create_dir_all(dir)?;
    }

    // Log directory follows OS conventions
    fs::create_dir_all(log_dir())?;

    Ok(())
}

/// Initialize the tracing/logging subsystem.
fn init_logging() {
    let log_path = log_dir();

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .max_log_files(5)
        .filename_prefix("app")
        .filename_suffix("log")
        .build(&log_path)
        .expect("failed to create log appender");

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .json()
                .with_writer(file_appender)
                .with_ansi(false),
        )
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(true)
                .compact(),
        )
        .init();
}

fn bundled_rg_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "rg.exe"
    }

    #[cfg(not(target_os = "windows"))]
    {
        "rg"
    }
}

fn configure_ripgrep_path<R: tauri::Runtime>(app: &tauri::App<R>) {
    let bundled_name = bundled_rg_name();
    let candidates = [
        app.path()
            .executable_dir()
            .ok()
            .map(|path| path.join(bundled_name)),
        app.path()
            .resource_dir()
            .ok()
            .map(|path| path.join(bundled_name)),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.is_file() {
            std::env::set_var("TIY_RG_PATH", &candidate);
            tracing::info!(path = %candidate.display(), "configured bundled ripgrep");
            return;
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let tiy_home = tiy_home();

    // 1. Initialize directories
    init_directories(&tiy_home).expect("failed to initialize $HOME/.tiy/ directories");

    // 2. Initialize logging
    init_logging();
    tracing::info!(path = %tiy_home.display(), "tiy agent starting");

    // 3. Build Tauri app
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(persisted_window_state_flags())
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::attachment::attachment_read_files,
            // System
            commands::system::get_system_metadata,
            commands::system::get_workspace_open_apps,
            commands::system::open_workspace_in_app,
            commands::system::open_tree_path_in_app,
            // Workspace
            commands::workspace::workspace_list,
            commands::workspace::workspace_add,
            commands::workspace::workspace_remove,
            commands::workspace::workspace_set_default,
            commands::workspace::workspace_validate,
            // Settings & Policies
            commands::settings::settings_get,
            commands::settings::settings_get_all,
            commands::settings::settings_set,
            commands::settings::policy_get,
            commands::settings::policy_get_all,
            commands::settings::policy_set,
            // Providers
            commands::settings::provider_catalog_list,
            commands::settings::provider_settings_get_all,
            commands::settings::provider_settings_fetch_models,
            commands::settings::provider_settings_upsert_builtin,
            commands::settings::provider_settings_create_custom,
            commands::settings::provider_settings_update_custom,
            commands::settings::provider_settings_delete_custom,
            commands::settings::provider_model_test_connection,
            // Agent Profiles
            commands::settings::profile_list,
            commands::settings::profile_create,
            commands::settings::profile_update,
            commands::settings::profile_delete,
            // Extensions
            commands::extensions::extensions_list,
            commands::extensions::extension_get_detail,
            commands::extensions::extension_enable,
            commands::extensions::extension_disable,
            commands::extensions::extension_uninstall,
            commands::extensions::extensions_list_commands,
            commands::extensions::extensions_list_activity,
            commands::extensions::marketplace_list_sources,
            commands::extensions::marketplace_add_source,
            commands::extensions::marketplace_remove_source,
            commands::extensions::marketplace_refresh_source,
            commands::extensions::marketplace_list_items,
            commands::extensions::marketplace_install_item,
            commands::extensions::plugin_validate_dir,
            commands::extensions::plugin_install_from_dir,
            commands::extensions::plugin_update_config,
            commands::extensions::mcp_list_servers,
            commands::extensions::mcp_add_server,
            commands::extensions::mcp_update_server,
            commands::extensions::mcp_remove_server,
            commands::extensions::mcp_restart_server,
            commands::extensions::mcp_get_server_state,
            commands::extensions::skill_list,
            commands::extensions::skill_rescan,
            commands::extensions::skill_enable,
            commands::extensions::skill_disable,
            commands::extensions::skill_pin,
            commands::extensions::skill_preview,
            // Threads
            commands::thread::thread_list,
            commands::thread::thread_create,
            commands::thread::thread_load,
            commands::thread::thread_update_title,
            commands::thread::thread_delete,
            commands::thread::thread_add_message,
            // Agent Run
            commands::agent::thread_start_run,
            commands::agent::thread_subscribe_run,
            commands::agent::thread_execute_approved_plan,
            commands::agent::thread_clear_context,
            commands::agent::thread_compact_context,
            commands::agent::thread_cancel_run,
            commands::agent::tool_approval_respond,
            commands::agent::tool_clarify_respond,
            // Index
            commands::index::index_get_tree,
            commands::index::index_get_children,
            commands::index::index_filter_files,
            commands::index::index_reveal_path,
            commands::index::index_search,
            // Git
            commands::git::git_get_snapshot,
            commands::git::git_get_history,
            commands::git::git_get_diff,
            commands::git::git_get_file_status,
            commands::git::git_subscribe,
            commands::git::git_refresh,
            commands::git::git_stage,
            commands::git::git_unstage,
            commands::git::git_generate_commit_message,
            commands::git::git_commit,
            commands::git::git_fetch,
            commands::git::git_pull,
            commands::git::git_push,
            // Terminal
            commands::terminal::terminal_create_or_attach,
            commands::terminal::terminal_write_input,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_restart,
            commands::terminal::terminal_close,
            commands::terminal::terminal_list,
        ])
        .setup(move |app| {
            configure_ripgrep_path(app);

            // 4. Initialize database (async, on the tokio runtime that Tauri provides)
            let db_path = tiy_home.join("db/tiy-agent.db");

            let pool = tauri::async_runtime::block_on(async {
                persistence::init_database(&db_path).await
            })?;

            tracing::info!(db = %db_path.display(), "database ready");

            // 5. Construct and manage AppState
            let state = AppState::new(pool, app.handle().clone());

            if let Some(setting) = tauri::async_runtime::block_on(async {
                state
                    .settings_manager
                    .get_setting(PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY)
                    .await
            })? {
                match serde_json::from_str::<bool>(&setting.value_json) {
                    Ok(enabled) => {
                        tauri::async_runtime::block_on(async {
                            state.sleep_manager.set_user_preference(enabled).await;
                        });
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "failed to parse prevent sleep setting");
                    }
                }
            }

            // 6. Startup recovery: validate workspaces + interrupt dangling runs
            tauri::async_runtime::block_on(async {
                state.workspace_manager.validate_all().await?;
                state.thread_manager.recover_interrupted_runs().await?;
                state.terminal_manager.recover_orphaned_sessions().await?;
                Ok::<(), crate::model::errors::AppError>(())
            })?;

            tauri::async_runtime::spawn(async {
                crate::core::settings_manager::SettingsManager::refresh_catalog_snapshot_silently()
                    .await;
            });

            app.manage(state);

            // 7. Platform-specific window setup
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = window.set_decorations(false);
            }

            Ok(())
        })
        .on_page_load(|webview, payload| {
            if webview.label() != MAIN_WINDOW_LABEL || payload.event() != PageLoadEvent::Finished {
                return;
            }

            let window = webview.window();
            let _ = window.show();
            let _ = window.set_focus();
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
