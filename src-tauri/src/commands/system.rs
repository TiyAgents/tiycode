use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMetadata {
    app_name: String,
    version: String,
    platform: String,
    arch: String,
    runtime: String,
}

#[tauri::command]
pub fn get_system_metadata() -> SystemMetadata {
    SystemMetadata {
        app_name: "Tiy Agent".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        runtime: "Tauri 2".into(),
    }
}
