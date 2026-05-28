//! Tauri IPC commands for gateway process management.
//!
//! Exposes start/stop/restart/status to the frontend so the Settings UI
//! can trigger gateway lifecycle changes without restarting the app.

use tauri::State;

use crate::core::gateway_supervisor::{GatewayStatus, GatewaySupervisorHandle};
use crate::gateway::config::{self as gateway_config, GatewayConfigDto, GatewayConfigUpdateInput};
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

/// Get the current gateway config (secrets masked).
#[tauri::command]
pub async fn gateway_get_config() -> Result<Option<GatewayConfigDto>, String> {
    Ok(gateway_config::load_config().map(|c| GatewayConfigDto::from_config(&c)))
}

/// Save gateway config updates from the frontend.
///
/// Merges the input into the existing config (or creates a new one).
/// Empty `wecom_secret` means "keep the existing secret".
#[tauri::command]
pub async fn gateway_save_config(input: GatewayConfigUpdateInput) -> Result<(), String> {
    use crate::gateway::config::{GatewayConfig, WecomConfig, WeixinConfig};
    use crate::gateway::traits::Platform;

    let mut config = gateway_config::load_config().unwrap_or_else(|| GatewayConfig {
        platform: Platform::Weixin,
        approval_timeout_seconds: 60,
        send_chunk_delay_seconds: 1.5,
        weixin: Some(WeixinConfig {
            enabled: false,
            account_id: String::new(),
            token: None,
            base_url: "ilinkai.weixin.qq.com".to_string(),
            cdn_base_url: "novac2c.cdn.weixin.qq.com/c2c".to_string(),
        }),
        wecom: Some(WecomConfig {
            enabled: false,
            bot_id: String::new(),
            secret: String::new(),
            ws_url: "openws.work.weixin.qq.com".to_string(),
        }),
    });

    // Update weixin enabled.
    if let Some(ref mut wx) = config.weixin {
        wx.enabled = input.weixin_enabled;
    } else if input.weixin_enabled {
        config.weixin = Some(WeixinConfig {
            enabled: true,
            account_id: String::new(),
            token: None,
            base_url: "ilinkai.weixin.qq.com".to_string(),
            cdn_base_url: "novac2c.cdn.weixin.qq.com/c2c".to_string(),
        });
    }

    // Update wecom fields.
    if let Some(ref mut wc) = config.wecom {
        wc.enabled = input.wecom_enabled;
        wc.bot_id = input.wecom_bot_id;
        if !input.wecom_secret.is_empty() {
            wc.secret = input.wecom_secret;
        }
        wc.ws_url = input.wecom_ws_url;
    } else {
        config.wecom = Some(WecomConfig {
            enabled: input.wecom_enabled,
            bot_id: input.wecom_bot_id,
            secret: input.wecom_secret,
            ws_url: input.wecom_ws_url,
        });
    }

    // Set active platform to the first enabled one.
    if input.weixin_enabled {
        config.platform = Platform::Weixin;
    } else if input.wecom_enabled {
        config.platform = Platform::Wecom;
    }

    gateway_config::save_config(&config).map_err(|e| e.to_string())
}
