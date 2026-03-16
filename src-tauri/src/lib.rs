mod commands;
pub mod core;
pub mod ipc;
pub mod model;
mod persistence;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::core::app_state::AppState;

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

/// Resolve the platform-native log directory for Tiy Agent.
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

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));

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
            // System
            commands::system::get_system_metadata,
            commands::system::get_workspace_open_apps,
            commands::system::open_workspace_in_app,
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
            commands::settings::provider_list,
            commands::settings::provider_create,
            commands::settings::provider_update,
            commands::settings::provider_delete,
            commands::settings::provider_model_list,
            commands::settings::provider_model_add,
            commands::settings::provider_model_remove,
            // Agent Profiles
            commands::settings::profile_list,
            commands::settings::profile_create,
            commands::settings::profile_update,
            commands::settings::profile_delete,
            // Threads
            commands::thread::thread_list,
            commands::thread::thread_create,
            commands::thread::thread_load,
            commands::thread::thread_update_title,
            commands::thread::thread_delete,
            commands::thread::thread_add_message,
            // Agent Run
            commands::agent::thread_start_run,
            commands::agent::thread_cancel_run,
            commands::agent::tool_approval_respond,
            commands::agent::sidecar_status,
            // Index
            commands::index::index_get_tree,
            commands::index::index_search,
        ])
        .setup(move |app| {
            // 4. Initialize database (async, on the tokio runtime that Tauri provides)
            let db_path = tiy_home.join("db/tiy-agent.db");

            let pool = tauri::async_runtime::block_on(async {
                persistence::init_database(&db_path).await
            })?;

            tracing::info!(db = %db_path.display(), "database ready");

            // 5. Resolve sidecar path
            // In dev, use a placeholder. Real sidecar will be resolved from
            // app resources or config once the sidecar project is built.
            let sidecar_path = std::env::var("TIY_SIDECAR_PATH")
                .unwrap_or_else(|_| "tiy-agent-sidecar".to_string());

            // 6. Construct and manage AppState
            let state = AppState::new(pool, sidecar_path);

            // 7. Startup recovery: validate workspaces + interrupt dangling runs
            tauri::async_runtime::block_on(async {
                state.workspace_manager.validate_all().await?;
                state.thread_manager.recover_interrupted_runs().await?;
                Ok::<(), crate::model::errors::AppError>(())
            })?;

            // 8. Start sidecar process and event processing loop
            //    Sidecar start is best-effort — if the binary is not found,
            //    the app still launches but agent runs will fail gracefully.
            let sidecar_started = tauri::async_runtime::block_on(async {
                match state.sidecar_manager.start().await {
                    Ok(()) => true,
                    Err(e) => {
                        tracing::warn!(error = %e, "sidecar failed to start (agent runs will be unavailable)");
                        false
                    }
                }
            });

            if sidecar_started {
                if let Some(event_rx) = state.sidecar_manager.take_event_receiver() {
                    let manager = Arc::clone(&state.agent_run_manager);
                    tauri::async_runtime::spawn(async move {
                        manager.spawn_event_loop(event_rx);
                    });
                }
            }

            app.manage(state);

            // 9. Platform-specific window setup
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
