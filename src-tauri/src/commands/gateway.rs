//! Tauri IPC commands for gateway process management.
//!
//! Exposes start/stop/restart/status to the frontend so the Settings UI
//! can trigger gateway lifecycle changes without restarting the app.

use std::str::FromStr;
use tauri::State;

use crate::core::gateway_supervisor::{GatewayStatus, GatewaySupervisorHandle};
use crate::gateway::config::{
    self as gateway_config, DmPolicy, GatewayConfigDto, GatewayConfigUpdateInput, GroupPolicy,
};
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
    use crate::gateway::config::{
        FeishuConfig, GatewayConfig, WecomConfig, WeixinConfig, WhatsAppConfig,
    };
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
        feishu: Some(FeishuConfig {
            enabled: false,
            app_id: String::new(),
            app_secret: String::new(),
            encrypt_key: None,
            verification_token: None,
            webhook_host: "127.0.0.1".to_string(),
            webhook_port: 8765,
            group_policy: GroupPolicy::Open,
            group_allowlist: vec![],
            group_blacklist: vec![],
            domain: "open.feishu.cn".to_string(),
        }),
        whatsapp: Some(WhatsAppConfig {
            enabled: false,
            phone_number_id: String::new(),
            access_token: String::new(),
            app_secret: String::new(),
            webhook_verify_token: String::new(),
            webhook_host: "127.0.0.1".to_string(),
            webhook_port: 8766,
            dm_policy: DmPolicy::Open,
            allow_from: vec![],
            api_version: "v21.0".to_string(),
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

    // Update feishu fields.
    if let Some(ref mut fs) = config.feishu {
        fs.enabled = input.feishu_enabled;
        fs.app_id = input.feishu_app_id;
        if !input.feishu_app_secret.is_empty() {
            fs.app_secret = input.feishu_app_secret;
        }
        fs.webhook_host = input.feishu_webhook_host;
        fs.webhook_port = input.feishu_webhook_port;
        fs.group_policy =
            GroupPolicy::from_str(&input.feishu_group_policy).map_err(|e| e.to_string())?;
        if let Some(ref ek) = input.feishu_encrypt_key {
            fs.encrypt_key = if ek.is_empty() {
                None
            } else {
                Some(ek.clone())
            };
        }
        if let Some(ref vt) = input.feishu_verification_token {
            fs.verification_token = if vt.is_empty() {
                None
            } else {
                Some(vt.clone())
            };
        }
    } else {
        config.feishu = Some(FeishuConfig {
            enabled: input.feishu_enabled,
            app_id: input.feishu_app_id,
            app_secret: input.feishu_app_secret,
            encrypt_key: input.feishu_encrypt_key.and_then(|v| {
                if v.is_empty() {
                    None
                } else {
                    Some(v)
                }
            }),
            verification_token: input.feishu_verification_token.and_then(|v| {
                if v.is_empty() {
                    None
                } else {
                    Some(v)
                }
            }),
            webhook_host: input.feishu_webhook_host,
            webhook_port: input.feishu_webhook_port,
            group_policy: GroupPolicy::from_str(&input.feishu_group_policy)
                .map_err(|e| e.to_string())?,
            group_allowlist: vec![],
            group_blacklist: vec![],
            domain: "open.feishu.cn".to_string(),
        });
    }

    // Update whatsapp fields.
    if let Some(ref mut wa) = config.whatsapp {
        wa.enabled = input.whatsapp_enabled;
        wa.phone_number_id = input.whatsapp_phone_number_id;
        if !input.whatsapp_access_token.is_empty() {
            wa.access_token = input.whatsapp_access_token;
        }
        wa.webhook_host = input.whatsapp_webhook_host;
        wa.webhook_port = input.whatsapp_webhook_port;
        wa.dm_policy = DmPolicy::from_str(&input.whatsapp_dm_policy).map_err(|e| e.to_string())?;
        if let Some(ref s) = input.whatsapp_app_secret {
            if !s.is_empty() {
                wa.app_secret = s.clone();
            }
        }
        if let Some(ref t) = input.whatsapp_webhook_verify_token {
            if !t.is_empty() {
                wa.webhook_verify_token = t.clone();
            }
        }
    } else {
        config.whatsapp = Some(WhatsAppConfig {
            enabled: input.whatsapp_enabled,
            phone_number_id: input.whatsapp_phone_number_id,
            access_token: input.whatsapp_access_token,
            app_secret: input.whatsapp_app_secret.unwrap_or_default(),
            webhook_verify_token: input.whatsapp_webhook_verify_token.unwrap_or_default(),
            webhook_host: input.whatsapp_webhook_host,
            webhook_port: input.whatsapp_webhook_port,
            dm_policy: DmPolicy::from_str(&input.whatsapp_dm_policy).map_err(|e| e.to_string())?,
            allow_from: vec![],
            api_version: "v21.0".to_string(),
        });
    }

    // Set active platform to the first enabled one.
    if input.weixin_enabled {
        config.platform = Platform::Weixin;
    } else if input.wecom_enabled {
        config.platform = Platform::Wecom;
    } else if input.feishu_enabled {
        config.platform = Platform::Feishu;
    } else if input.whatsapp_enabled {
        config.platform = Platform::WhatsApp;
    }

    gateway_config::save_config(&config).map_err(|e| e.to_string())
}
