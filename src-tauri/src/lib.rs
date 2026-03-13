mod commands;

use tauri::{webview::PageLoadEvent, Manager};
use tauri_plugin_window_state::{StateFlags, WindowExt};

const MAIN_WINDOW_LABEL: &str = "main";

fn restorable_window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN
}

fn restore_main_window_state<W: WindowExt>(window: &W) {
    if let Err(error) = window.restore_state(restorable_window_state_flags()) {
        eprintln!("failed to restore main window state: {error}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_metadata,
            commands::system::get_workspace_open_apps,
            commands::system::open_workspace_in_app
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                #[cfg(target_os = "windows")]
                {
                    let _ = window.set_decorations(false);
                }

                restore_main_window_state(&window);
            }

            Ok(())
        })
        .on_page_load(|webview, payload| {
            if webview.label() != MAIN_WINDOW_LABEL || payload.event() != PageLoadEvent::Finished {
                return;
            }

            restore_main_window_state(&webview.window());
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
