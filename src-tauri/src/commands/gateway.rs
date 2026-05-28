//! Tauri IPC commands for gateway process management.
//!
//! Exposes start/stop/restart/status to the frontend so the Settings UI
//! can trigger gateway lifecycle changes without restarting the app.

use tauri::State;

use crate::core::gateway_supervisor::{GatewayStatus, GatewaySupervisorHandle};

/// Start the gateway process (no-op if already running).
/// Called by the frontend after the user saves gateway configuration.
#[tauri::command]
pub async fn gateway_start(supervisor: State<'_, GatewaySupervisorHandle>) -> Result<bool, String> {
    supervisor.ensure_started().await.map_err(|e| e.to_string())
}

/// Stop the gateway process.
#[tauri::command]
pub async fn gateway_stop(supervisor: State<'_, GatewaySupervisorHandle>) -> Result<(), String> {
    supervisor.stop().await.map_err(|e| e.to_string())
}

/// Restart the gateway process (stop + start).
/// Useful after config changes or manual intervention.
#[tauri::command]
pub async fn gateway_restart(supervisor: State<'_, GatewaySupervisorHandle>) -> Result<(), String> {
    supervisor.restart().await.map_err(|e| e.to_string())
}

/// Get the current gateway process status.
#[tauri::command]
pub async fn gateway_status(
    supervisor: State<'_, GatewaySupervisorHandle>,
) -> Result<GatewayStatus, String> {
    Ok(supervisor.status().await)
}
