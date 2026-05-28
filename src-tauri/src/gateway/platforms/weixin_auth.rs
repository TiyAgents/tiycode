//! WeChat iLink Bot QR-code login flow.
//!
//! Implements the full lifecycle: request QR code → poll login status → persist
//! session token. Used by the GUI process IPC commands before starting the
//! gateway subprocess.
//!
//! Reference: hermes-agent/gateway/platforms/weixin.py (login flow)

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// iLink API base URL.
const DEFAULT_BASE_URL: &str = "ilinkai.weixin.qq.com";

/// Poll interval for checklogin (seconds).
const LOGIN_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Maximum poll attempts before timeout.
const MAX_POLL_ATTEMPTS: u32 = 100; // ~5 minutes

/// Persisted session data written to `~/.tiy/gateway/weixin/session.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinSession {
    /// Bearer token for iLink API.
    pub token: String,
    /// Bot account ID.
    pub account_id: String,
    /// Timestamp when the session was created (Unix seconds).
    pub created_at: i64,
}

/// Status of the login polling loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LoginPollStatus {
    /// QR code generated, waiting for scan.
    WaitingScan,
    /// User scanned but not yet confirmed.
    WaitingConfirm,
    /// Login successful — token acquired.
    Success { session: WeixinSession },
    /// Login failed or timed out.
    Failed { error: String },
    /// QR code expired.
    Expired,
}

/// Result of the initial QR code request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrLoginResult {
    /// QR code image as base64-encoded PNG (for frontend display).
    pub qr_image_base64: Option<String>,
    /// QR code URL (if available, for rendering as link).
    pub qr_url: Option<String>,
    /// UUID for this login session (used in subsequent checklogin polls).
    pub login_uuid: String,
}

/// Persistence directory for WeChat session: `~/.tiy/gateway/weixin/`.
pub fn weixin_state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tiy/gateway/weixin")
}

/// Session file path.
pub fn session_file_path() -> PathBuf {
    weixin_state_dir().join("session.json")
}

/// Load a previously persisted session (if any).
pub fn load_session() -> Option<WeixinSession> {
    let path = session_file_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// Persist session to disk atomically.
pub fn save_session(session: &WeixinSession) -> anyhow::Result<()> {
    let dir = weixin_state_dir();
    std::fs::create_dir_all(&dir)?;
    let path = session_file_path();
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(session)?;
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Delete persisted session (for logout).
pub fn clear_session() {
    let _ = std::fs::remove_file(session_file_path());
}

/// Build standard API headers for iLink requests (pre-auth, no token needed for QR).
fn qr_api_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("iLink-App-Id", "bot".parse().unwrap());
    headers.insert("iLink-App-ClientVersion", "131584".parse().unwrap());
    headers.insert("AuthorizationType", "ilink_bot_token".parse().unwrap());
    // Aligned with hermes-agent: random u32 → decimal string → base64.
    let uin_int = u32::from_be_bytes(
        uuid::Uuid::now_v7().as_bytes()[0..4]
            .try_into()
            .unwrap_or([0x12, 0x34, 0x56, 0x78]),
    );
    let wechat_uin =
        base64::engine::general_purpose::STANDARD.encode(uin_int.to_string().as_bytes());
    headers.insert("X-WECHAT-UIN", wechat_uin.parse().unwrap());
    headers
}

/// Build the full iLink API URL.
fn api_url(base_url: &str, endpoint: &str) -> String {
    format!("https://{}/ilink/bot/{}", base_url, endpoint)
}

/// Request a new QR code for login from the iLink API.
///
/// Returns the QR code data needed for the frontend to display.
pub async fn request_qr_code(base_url: Option<&str>) -> anyhow::Result<QrLoginResult> {
    let base = base_url.unwrap_or(DEFAULT_BASE_URL);
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let resp = client
        .post(api_url(base, "getqrcode"))
        .headers(qr_api_headers())
        .json(&json!({}))
        .send()
        .await?;

    let data: Value = resp.json().await?;
    let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
    if errcode != 0 {
        let errmsg = data
            .get("errmsg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("getqrcode failed: {errcode} {errmsg}");
    }

    // Extract QR data. The iLink API may return:
    // - `qr_code`: base64 PNG image
    // - `qr_url`: scannable URL
    // - `uuid`: login session identifier
    let qr_image_base64 = data
        .get("qr_code")
        .or_else(|| data.get("qrcode"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let qr_url = data
        .get("qr_url")
        .or_else(|| data.get("url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let login_uuid = data
        .get("uuid")
        .or_else(|| data.get("login_uuid"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if login_uuid.is_empty() && qr_image_base64.is_none() && qr_url.is_none() {
        anyhow::bail!("getqrcode response missing QR data: {data}");
    }

    Ok(QrLoginResult {
        qr_image_base64,
        qr_url,
        login_uuid,
    })
}

/// Poll the login status once. Returns the current status.
///
/// The frontend should call this repeatedly until it gets `Success` or `Failed`.
pub async fn poll_login_status(
    login_uuid: &str,
    base_url: Option<&str>,
) -> anyhow::Result<LoginPollStatus> {
    let base = base_url.unwrap_or(DEFAULT_BASE_URL);
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let resp = client
        .post(api_url(base, "checklogin"))
        .headers(qr_api_headers())
        .json(&json!({ "uuid": login_uuid }))
        .send()
        .await?;

    let data: Value = resp.json().await?;
    let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);

    match errcode {
        0 => {
            // Login successful — extract token and account info.
            let token = data
                .get("token")
                .or_else(|| data.get("auth_token"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let account_id = data
                .get("account_id")
                .or_else(|| data.get("bot_id"))
                .or_else(|| data.get("user_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if token.is_empty() {
                return Ok(LoginPollStatus::Failed {
                    error: "login response missing token".to_string(),
                });
            }

            let session = WeixinSession {
                token,
                account_id,
                created_at: chrono::Utc::now().timestamp(),
            };

            // Persist session immediately.
            if let Err(e) = save_session(&session) {
                tracing::warn!(error = %e, "failed to persist weixin session");
            }

            Ok(LoginPollStatus::Success { session })
        }
        // Waiting for scan.
        408 => Ok(LoginPollStatus::WaitingScan),
        // Scanned, waiting for confirm.
        201 => Ok(LoginPollStatus::WaitingConfirm),
        // QR expired.
        400 | 402 => Ok(LoginPollStatus::Expired),
        // Other errors.
        _ => {
            let errmsg = data
                .get("errmsg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Ok(LoginPollStatus::Failed {
                error: format!("checklogin error {errcode}: {errmsg}"),
            })
        }
    }
}

/// Run the full QR login flow synchronously (blocking poll loop).
///
/// This is a convenience function for non-interactive (CLI) usage.
/// For GUI usage, prefer the step-by-step `request_qr_code` + `poll_login_status`.
pub async fn run_qr_login(base_url: Option<&str>) -> anyhow::Result<WeixinSession> {
    let qr = request_qr_code(base_url).await?;
    tracing::info!(uuid = %qr.login_uuid, "QR code generated, waiting for scan...");

    for _attempt in 0..MAX_POLL_ATTEMPTS {
        tokio::time::sleep(LOGIN_POLL_INTERVAL).await;

        match poll_login_status(&qr.login_uuid, base_url).await? {
            LoginPollStatus::Success { session } => {
                tracing::info!(account_id = %session.account_id, "login successful");
                return Ok(session);
            }
            LoginPollStatus::WaitingScan => continue,
            LoginPollStatus::WaitingConfirm => {
                tracing::info!("QR scanned, waiting for confirmation...");
                continue;
            }
            LoginPollStatus::Expired => {
                anyhow::bail!("QR code expired, please retry");
            }
            LoginPollStatus::Failed { error } => {
                anyhow::bail!("login failed: {error}");
            }
        }
    }

    anyhow::bail!("login timed out after {} attempts", MAX_POLL_ATTEMPTS)
}
