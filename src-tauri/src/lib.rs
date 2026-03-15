mod commands;

use tauri::webview::PageLoadEvent;
#[cfg(target_os = "windows")]
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

const MAIN_WINDOW_LABEL: &str = "main";

fn persisted_window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(persisted_window_state_flags())
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_metadata,
            commands::system::get_workspace_open_apps,
            commands::system::open_workspace_in_app
        ])
        .setup(|_app| {
            #[cfg(target_os = "windows")]
            if let Some(window) = _app.get_webview_window(MAIN_WINDOW_LABEL) {
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
