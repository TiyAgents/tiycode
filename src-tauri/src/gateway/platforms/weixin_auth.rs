//! WeChat iLink Bot QR-code login flow.
//!
//! Implements the full lifecycle: request QR code → poll login status → persist
//! session token. Used by the GUI process IPC commands before starting the
//! gateway subprocess.

use std::path::PathBuf;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// iLink API base URL (with scheme).
const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

/// iLink endpoint: get bot QR code.
const EP_GET_BOT_QR: &str = "ilink/bot/get_bot_qrcode";

/// iLink endpoint: poll QR code status.
const EP_GET_QR_STATUS: &str = "ilink/bot/get_qrcode_status";

/// Default bot_type query parameter for QR code request.
const DEFAULT_BOT_TYPE: &str = "3";

/// iLink App-Id header value.
const ILINK_APP_ID: &str = "bot";

/// iLink client version: (2 << 16) | (2 << 8) | 0 = 131584
const ILINK_APP_CLIENT_VERSION: &str = "131584";

/// QR API timeout (35s).
const QR_TIMEOUT: Duration = Duration::from_secs(35);

/// Poll interval for status check (seconds).
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
    /// iLink API base URL (may differ from default after redirect).
    #[serde(default)]
    pub base_url: String,
    /// User ID from iLink.
    #[serde(default)]
    pub user_id: String,
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
    /// Scanned but requires redirect to a different host.
    ScannedRedirect { redirect_host: String },
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
    /// QR code image as base64-encoded SVG (generated locally from the scannable URL).
    pub qr_image_base64: Option<String>,
    /// QR code scannable URL (for rendering as link / fallback).
    pub qr_url: Option<String>,
    /// The `qrcode` hex token used for subsequent status polls.
    pub login_uuid: String,
    /// Image media type hint: always "svg+xml" for locally generated QR codes.
    pub media_type: Option<String>,
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

/// Build minimal headers for iLink GET requests (QR endpoints).
///
/// Only App-Id and ClientVersion are required for unauthenticated GET endpoints.
fn qr_get_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("iLink-App-Id", ILINK_APP_ID.parse().unwrap());
    headers.insert(
        "iLink-App-ClientVersion",
        ILINK_APP_CLIENT_VERSION.parse().unwrap(),
    );
    headers
}

/// Build the full iLink API URL.
fn api_url(base_url: &str, endpoint: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), endpoint)
}

/// Generate a QR code SVG string from the given data using the `qrcode` crate.
fn generate_qr_svg(data: &str) -> anyhow::Result<String> {
    use qrcode::QrCode;
    let code = QrCode::new(data.as_bytes())?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .build();
    Ok(svg)
}

/// Request a new QR code for login from the iLink API.
///
/// GET `ilink/bot/get_bot_qrcode?bot_type=3`.
/// Response contains `qrcode` (hex token for polling) and `qrcode_img_content`
/// (full scannable liteapp URL). We generate a QR code SVG locally from the
/// scannable URL and return it as base64 for the frontend to display.
pub async fn request_qr_code(base_url: Option<&str>) -> anyhow::Result<QrLoginResult> {
    let base = base_url.unwrap_or(DEFAULT_BASE_URL);
    let client = Client::builder().timeout(QR_TIMEOUT).build()?;

    let url = format!(
        "{}?bot_type={}",
        api_url(base, EP_GET_BOT_QR),
        DEFAULT_BOT_TYPE
    );

    let resp = client.get(&url).headers(qr_get_headers()).send().await?;

    let status = resp.status();
    let raw = resp.text().await?;

    if !status.is_success() {
        anyhow::bail!(
            "get_bot_qrcode HTTP {}: {}",
            status,
            &raw[..raw.len().min(200)]
        );
    }

    let data: Value = serde_json::from_str(&raw).map_err(|e| {
        anyhow::anyhow!(
            "get_bot_qrcode response is not JSON (status={status}): {e}\nbody preview: {}",
            &raw[..raw.len().min(200)]
        )
    })?;

    // Extract the hex token for polling.
    let qrcode_value = data
        .get("qrcode")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Extract the full scannable URL (liteapp URL).
    let qrcode_img_content = data
        .get("qrcode_img_content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if qrcode_value.is_empty() {
        anyhow::bail!("get_bot_qrcode response missing 'qrcode' field: {data}");
    }

    // Determine what to encode into the QR image.
    // Prefer the full URL; fall back to the hex token.
    let qr_scan_data = if qrcode_img_content.is_empty() {
        &qrcode_value
    } else {
        &qrcode_img_content
    };

    // Generate QR code SVG locally.
    let qr_image_base64 = match generate_qr_svg(qr_scan_data) {
        Ok(svg) => {
            use base64::Engine;
            Some(base64::engine::general_purpose::STANDARD.encode(svg.as_bytes()))
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to generate QR code SVG");
            None
        }
    };

    tracing::info!(
        qrcode_len = qrcode_value.len(),
        has_img_content = !qrcode_img_content.is_empty(),
        "get_bot_qrcode succeeded"
    );

    Ok(QrLoginResult {
        qr_image_base64,
        qr_url: if qrcode_img_content.is_empty() {
            None
        } else {
            Some(qrcode_img_content)
        },
        login_uuid: qrcode_value,
        media_type: Some("svg+xml".to_string()),
    })
}

/// Poll the login status once. Returns the current status.
///
/// GET `ilink/bot/get_qrcode_status?qrcode={token}`.
/// Status values: `wait`, `scaned`, `scaned_but_redirect`, `expired`, `confirmed`.
pub async fn poll_login_status(
    login_uuid: &str,
    base_url: Option<&str>,
) -> anyhow::Result<LoginPollStatus> {
    let base = base_url.unwrap_or(DEFAULT_BASE_URL);
    let client = Client::builder().timeout(QR_TIMEOUT).build()?;

    let url = format!("{}?qrcode={}", api_url(base, EP_GET_QR_STATUS), login_uuid);

    let resp = client.get(&url).headers(qr_get_headers()).send().await?;

    let status_code = resp.status();
    let raw = resp.text().await?;

    if !status_code.is_success() {
        return Ok(LoginPollStatus::Failed {
            error: format!(
                "get_qrcode_status HTTP {}: {}",
                status_code,
                &raw[..raw.len().min(200)]
            ),
        });
    }

    let data: Value = serde_json::from_str(&raw).map_err(|e| {
        anyhow::anyhow!(
            "get_qrcode_status response is not JSON: {e}\nbody: {}",
            &raw[..raw.len().min(200)]
        )
    })?;

    let qr_status = data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("wait");

    match qr_status {
        "wait" => Ok(LoginPollStatus::WaitingScan),

        "scaned" => Ok(LoginPollStatus::WaitingConfirm),

        "scaned_but_redirect" => {
            let redirect_host = data
                .get("redirect_host")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Ok(LoginPollStatus::ScannedRedirect { redirect_host })
        }

        "expired" => Ok(LoginPollStatus::Expired),

        "confirmed" => {
            let account_id = data
                .get("ilink_bot_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let token = data
                .get("bot_token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let confirmed_base_url = data
                .get("baseurl")
                .and_then(|v| v.as_str())
                .unwrap_or(DEFAULT_BASE_URL)
                .to_string();

            let user_id = data
                .get("ilink_user_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if account_id.is_empty() || token.is_empty() {
                return Ok(LoginPollStatus::Failed {
                    error: "QR confirmed but credential payload was incomplete".to_string(),
                });
            }

            let session = WeixinSession {
                token,
                account_id,
                base_url: confirmed_base_url,
                user_id,
                created_at: chrono::Utc::now().timestamp(),
            };

            // Persist session immediately.
            if let Err(e) = save_session(&session) {
                tracing::warn!(error = %e, "failed to persist weixin session");
            }

            Ok(LoginPollStatus::Success { session })
        }

        other => Ok(LoginPollStatus::Failed {
            error: format!("unknown QR status: {other}"),
        }),
    }
}

/// Run the full QR login flow synchronously (blocking poll loop).
///
/// This is a convenience function for non-interactive (CLI) usage.
/// For GUI usage, prefer the step-by-step `request_qr_code` + `poll_login_status`.
pub async fn run_qr_login(base_url: Option<&str>) -> anyhow::Result<WeixinSession> {
    let qr = request_qr_code(base_url).await?;
    tracing::info!(uuid = %qr.login_uuid, "QR code generated, waiting for scan...");

    let mut current_base_url = base_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    for _attempt in 0..MAX_POLL_ATTEMPTS {
        tokio::time::sleep(LOGIN_POLL_INTERVAL).await;

        match poll_login_status(&qr.login_uuid, Some(&current_base_url)).await? {
            LoginPollStatus::Success { session } => {
                tracing::info!(account_id = %session.account_id, "login successful");
                return Ok(session);
            }
            LoginPollStatus::WaitingScan => continue,
            LoginPollStatus::WaitingConfirm => {
                tracing::info!("QR scanned, waiting for confirmation...");
                continue;
            }
            LoginPollStatus::ScannedRedirect { redirect_host } => {
                if !redirect_host.is_empty() {
                    current_base_url = format!("https://{redirect_host}");
                    tracing::info!(new_base_url = %current_base_url, "redirecting");
                }
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
