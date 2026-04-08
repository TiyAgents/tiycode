mod commands;
pub mod core;
pub mod extensions;
pub mod ipc;
pub mod model;
mod persistence;

use std::fs;
use std::path::PathBuf;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent, WindowEvent};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_autostart::ManagerExt as AutoStartManagerExt;
use tauri_plugin_window_state::StateFlags;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::core::app_state::AppState;
use crate::core::desktop_runtime::{
    DesktopRuntimeState, LAUNCH_AT_LOGIN_SETTING_KEY, MINIMIZE_TO_TRAY_SETTING_KEY,
};
use crate::core::sleep_manager::PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY;
#[cfg(target_os = "macos")]
use crate::core::startup_manager;

const MAIN_WINDOW_LABEL: &str = "main";
const MAIN_TRAY_ID: &str = "main-tray";
const TRAY_SHOW_ID: &str = "tray-show";
const TRAY_QUIT_ID: &str = "tray-quit";

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

fn parse_bool_setting(record: Option<crate::model::settings::SettingRecord>, key: &str) -> bool {
    match record {
        Some(setting) => match serde_json::from_str::<bool>(&setting.value_json) {
            Ok(enabled) => enabled,
            Err(error) => {
                tracing::warn!(setting = key, error = %error, "failed to parse boolean setting");
                false
            }
        },
        None => false,
    }
}

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn sync_tray_visibility<R: tauri::Runtime>(app: &tauri::AppHandle<R>, visible: bool) {
    if let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) {
        if let Err(error) = tray.set_visible(visible) {
            tracing::warn!(error = %error, "failed to update tray visibility");
        }
    }
}

fn build_tray<R: tauri::Runtime>(
    app: &tauri::App<R>,
    visible: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItemBuilder::with_id(TRAY_SHOW_ID, "Show").build(app)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT_ID, "Quit").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;

    let mut tray_builder = TrayIconBuilder::with_id(MAIN_TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("TiyCode");

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let tray = tray_builder.build(app)?;
    tray.set_visible(visible)?;

    Ok(())
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
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(persisted_window_state_flags())
                .build(),
        );

    let builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());

    let builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());

    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        None::<Vec<&'static str>>,
    ));

    #[cfg(target_os = "macos")]
    let builder = builder;

    builder
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
            commands::extensions::marketplace_get_remove_source_plan,
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
            commands::terminal::terminal_list_available_shells,
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
            let desktop_runtime = DesktopRuntimeState::default();

            let (prevent_sleep_while_running, launch_at_login, minimize_to_tray) =
                tauri::async_runtime::block_on(async {
                    let prevent_sleep = state
                        .settings_manager
                        .get_setting(PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY)
                        .await?;
                    let launch_at_login = state
                        .settings_manager
                        .get_setting(LAUNCH_AT_LOGIN_SETTING_KEY)
                        .await?;
                    let minimize_to_tray = state
                        .settings_manager
                        .get_setting(MINIMIZE_TO_TRAY_SETTING_KEY)
                        .await?;

                    Ok::<_, crate::model::errors::AppError>((
                        parse_bool_setting(prevent_sleep, PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY),
                        parse_bool_setting(launch_at_login, LAUNCH_AT_LOGIN_SETTING_KEY),
                        parse_bool_setting(minimize_to_tray, MINIMIZE_TO_TRAY_SETTING_KEY),
                    ))
                })?;

            tauri::async_runtime::block_on(async {
                state
                    .sleep_manager
                    .set_user_preference(prevent_sleep_while_running)
                    .await;
            });

            desktop_runtime.set_minimize_to_tray(minimize_to_tray);

            #[cfg(target_os = "macos")]
            if let Some(system_enabled) = startup_manager::launch_at_login_enabled() {
                if system_enabled != launch_at_login {
                    let value_json =
                        serde_json::to_string(&system_enabled).unwrap_or_else(|_| "false".to_string());
                    let sync_result = state.settings_manager.set_setting(
                        LAUNCH_AT_LOGIN_SETTING_KEY,
                        &value_json,
                    );

                    if let Err(error) = tauri::async_runtime::block_on(sync_result) {
                        tracing::warn!(error = %error, "failed to sync launch at login setting from system state");
                    }
                }
            } else if let Err(error) = startup_manager::set_launch_at_login(launch_at_login) {
                tracing::warn!(error = %error, "failed to sync launch at login state");
            }

            #[cfg(not(target_os = "macos"))]
            {
                let autolaunch = app.handle().autolaunch();
                match autolaunch.is_enabled() {
                    Ok(system_enabled) if system_enabled != launch_at_login => {
                        let value_json = serde_json::to_string(&system_enabled)
                            .unwrap_or_else(|_| "false".to_string());
                        if let Err(error) = tauri::async_runtime::block_on(
                            state.settings_manager.set_setting(
                                LAUNCH_AT_LOGIN_SETTING_KEY,
                                &value_json,
                            ),
                        ) {
                            tracing::warn!(error = %error, "failed to sync launch at login setting from system state");
                        }
                    }
                    Ok(_) => {} // already in sync
                    Err(error) => {
                        tracing::warn!(error = %error, "failed to query autolaunch state, applying DB value");
                        let result = if launch_at_login {
                            autolaunch.enable()
                        } else {
                            autolaunch.disable()
                        };
                        if let Err(error) = result {
                            tracing::warn!(error = %error, "failed to sync launch at login state");
                        }
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

            // Apply bundled catalog snapshot if it is newer than the local cache.
            // This ensures a usable catalog is available even without network access
            // (e.g. fresh install or app update in an offline environment).
            crate::core::settings_manager::apply_bundled_catalog_if_newer(app.handle());

            tauri::async_runtime::spawn(async {
                crate::core::settings_manager::SettingsManager::refresh_catalog_snapshot_silently()
                    .await;
            });

            app.manage(state);
            app.manage(desktop_runtime);
            build_tray(app, minimize_to_tray)?;

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
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                let runtime = window.state::<DesktopRuntimeState>();
                if runtime.minimize_to_tray_enabled() && !runtime.is_quitting() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_SHOW_ID => show_main_window(app),
            TRAY_QUIT_ID => {
                app.state::<DesktopRuntimeState>().mark_quitting();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|app, event| {
            if event.id().as_ref() != MAIN_TRAY_ID {
                return;
            }

            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(app);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::ExitRequested { .. } => {
                app.state::<DesktopRuntimeState>().mark_quitting();
                sync_tray_visibility(app, false);
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => {
                show_main_window(app);
            }
            _ => {}
        });
}
