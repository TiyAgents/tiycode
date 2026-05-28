//! Tauri IPC commands for gateway process management.
//!
//! Exposes start/stop/restart/status to the frontend so the Settings UI
//! can trigger gateway lifecycle changes without restarting the app.

use tauri::State;

use crate::core::gateway_supervisor::{GatewayStatus, GatewaySupervisorHandle};
use crate::gateway::platforms::weixin_auth::{self, LoginPollStatus, QrLoginResult, WeixinSession};

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

/// Initiate WeChat QR login — requests a QR code from iLink API.
///
/// Frontend should display the returned QR image/URL, then poll with
/// `gateway_weixin_login_poll` until login completes.
#[tauri::command]
pub async fn gateway_weixin_qr_login(base_url: Option<String>) -> Result<QrLoginResult, String> {
    weixin_auth::request_qr_code(base_url.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Poll WeChat QR login status.
///
/// Call this repeatedly (e.g. every 3s) after `gateway_weixin_qr_login` until
/// the status is `success` or a terminal state.
#[tauri::command]
pub async fn gateway_weixin_login_poll(
    login_uuid: String,
    base_url: Option<String>,
) -> Result<LoginPollStatus, String> {
    weixin_auth::poll_login_status(&login_uuid, base_url.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Get the current WeChat session info (if logged in).
#[tauri::command]
pub async fn gateway_weixin_session() -> Result<Option<WeixinSession>, String> {
    Ok(weixin_auth::load_session())
}

/// Clear (logout) the WeChat session.
#[tauri::command]
pub async fn gateway_weixin_logout() -> Result<(), String> {
    weixin_auth::clear_session();
    Ok(())
}
