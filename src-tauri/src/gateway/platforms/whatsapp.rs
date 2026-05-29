//! WhatsApp Cloud API platform adapter.
//!
//! Connects via Webhook HTTP server to receive event pushes from the Meta
//! webhook, and sends messages via the WhatsApp Cloud API
//! (`graph.facebook.com/v21.0/{phone_number_id}/messages`). Supports:
//! - HMAC-SHA256 signature verification for webhook payloads
//! - Meta webhook subscription verification (GET handler)
//! - Message dedup with TTL (mirrors wecom.rs pattern)
//! - DM policy: open / allowlist / disabled
//! - Text, image, document, audio, video, sticker, location, contacts types

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_stream::stream;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::Stream;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::gateway::config::{DmPolicy, WhatsAppConfig};
use crate::gateway::traits::{InboundMessage, Platform, PlatformAdapter, SendResult};

// ── Constants ────────────────────────────────────────────────────────

/// Maximum message length for a single WhatsApp message.
const MAX_WHATSAPP_MESSAGE_LENGTH: usize = 4096;
/// Dedup TTL in seconds (5 minutes — Meta may redeliver quickly).
const DEDUP_TTL_SECONDS: u64 = 300;
/// Maximum entries in the dedup map before eviction.
const DEDUP_MAX_SIZE: usize = 1000;
/// Type alias for HMAC-SHA256.
type HmacSha256 = Hmac<Sha256>;

// ── WhatsApp adapter ─────────────────────────────────────────────────

pub struct WhatsAppAdapter {
    config: WhatsAppConfig,
    /// axum server shutdown signal.
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    server_handle: Option<JoinHandle<()>>,
    /// Inbound message channel.
    inbound_tx: mpsc::Sender<anyhow::Result<InboundMessage>>,
    /// Inbound message channel (reader consumed by poll_messages).
    inbound_rx: Arc<Mutex<Option<mpsc::Receiver<anyhow::Result<InboundMessage>>>>>,
    /// Message ID dedup map with TTL: msg_id → insert time.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    /// HTTP client for WhatsApp Cloud API calls.
    http: reqwest::Client,
}

impl WhatsAppAdapter {
    pub fn new(config: WhatsAppConfig) -> Self {
        let (inbound_tx, inbound_rx) = mpsc::channel(256);
        Self {
            config,
            shutdown_tx: None,
            server_handle: None,
            inbound_tx,
            inbound_rx: Arc::new(Mutex::new(Some(inbound_rx))),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            http: reqwest::Client::new(),
        }
    }

    // ── Signature verification ────────────────────────────────────

    /// Verify the Meta webhook HMAC-SHA256 signature.
    /// `x_hub_signature_256` format: `sha256=<hex>`
    /// Uses constant-time comparison via `subtle`.
    fn verify_signature(app_secret: &str, body: &[u8], x_hub_signature_256: &str) -> bool {
        let Some(hex_sig) = x_hub_signature_256.strip_prefix("sha256=") else {
            return false;
        };
        let mut mac: HmacSha256 = match HmacSha256::new_from_slice(app_secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => return false,
        };
        mac.update(body);
        let result = mac.finalize().into_bytes();
        // Constant-time comparison using subtle.
        use subtle::ConstantTimeEq;
        match hex::decode(hex_sig) {
            Ok(sig_bytes) => {
                if sig_bytes.len() != result.len() {
                    return false;
                }
                result.as_slice().ct_eq(&sig_bytes).into()
            }
            Err(_) => false,
        }
    }

    // ── Message parsing ───────────────────────────────────────────

    /// Parse a WhatsApp inbound message from the webhook `messages` array entry.
    fn parse_message(msg: &Value) -> Option<InboundMessage> {
        let message_id = msg
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let from = msg
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let msg_type = msg
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let text = match msg_type {
            "text" => msg
                .get("text")
                .and_then(|t| t.get("body"))
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string(),
            "image" | "document" | "audio" | "video" | "sticker" => {
                let caption = msg
                    .get(msg_type)
                    .and_then(|m| m.get("caption"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let media_id = msg
                    .get(msg_type)
                    .and_then(|m| m.get("id"))
                    .and_then(|i| i.as_str())
                    .unwrap_or("");
                if caption.is_empty() {
                    format!("[{msg_type}: {media_id}]")
                } else {
                    format!("[{msg_type}: {media_id}]\n{caption}")
                }
            }
            "location" => {
                let lat = msg
                    .get("location")
                    .and_then(|l| l.get("latitude"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let lng = msg
                    .get("location")
                    .and_then(|l| l.get("longitude"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let name = msg
                    .get("location")
                    .and_then(|l| l.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let address = msg
                    .get("location")
                    .and_then(|l| l.get("address"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mut parts = vec![format!("📍 {lat}, {lng}")];
                if !name.is_empty() {
                    parts.push(name.to_string());
                }
                if !address.is_empty() {
                    parts.push(address.to_string());
                }
                parts.join(" — ")
            }
            "contacts" => {
                let contacts = msg.get("contacts").and_then(|c| c.as_array());
                match contacts {
                    Some(arr) => {
                        let names: Vec<String> = arr
                            .iter()
                            .filter_map(|c| {
                                c.get("name")
                                    .and_then(|n| n.get("formatted_name"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect();
                        format!("👤 Contacts: {}", names.join(", "))
                    }
                    None => "[contacts]".to_string(),
                }
            }
            "reaction" => {
                let emoji = msg
                    .get("reaction")
                    .and_then(|r| r.get("emoji"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("👍");
                format!("[reaction: {emoji}]")
            }
            _ => {
                tracing::debug!(msg_type, "unsupported WhatsApp message type");
                return None;
            }
        };

        if text.is_empty() || message_id.is_empty() || from.is_empty() {
            return None;
        }

        Some(InboundMessage {
            message_id,
            sender_id: from.clone(),
            chat_id: from, // WhatsApp DM: chat_id = sender phone number
            text,
            is_group: false, // WhatsApp Cloud API messages are DM by default
            media_urls: vec![],
            media_attachments: vec![],
            req_id: None,
        })
    }

    // ── DM policy check ───────────────────────────────────────────

    /// Returns `true` if a DM should be processed.
    #[allow(dead_code)]
    fn should_process_dm(&self, sender: &str) -> bool {
        match self.config.dm_policy {
            DmPolicy::Disabled => false,
            DmPolicy::Allowlist => self.config.allow_from.iter().any(|a| a == sender),
            // DmPolicy::Open or unknown → allow
            DmPolicy::Open => true,
        }
    }

    // ── WhatsApp Cloud API calls ──────────────────────────────────

    /// Send a text message via the WhatsApp Cloud API, optionally replying to a specific message.
    async fn whatsapp_send_text(
        &self,
        to: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> anyhow::Result<SendResult> {
        let url = format!(
            "https://graph.facebook.com/{}/{}/messages",
            self.config.api_version, self.config.phone_number_id
        );
        let mut body = json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": { "preview_url": false, "body": text },
        });
        if let Some(context_msg_id) = reply_to {
            body.as_object_mut().map(|obj| {
                obj.insert(
                    "context".to_string(),
                    json!({ "message_id": context_msg_id }),
                );
                obj
            });
        }

        let resp: Value = self
            .http
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.access_token),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        // Check for error.
        if let Some(error) = resp.get("error") {
            let code = error.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            let msg = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Ok(SendResult::err(format!(
                "WhatsApp API error: code={code}, msg={msg}"
            )));
        }

        let message_id = resp
            .get("messages")
            .and_then(|m| m.get(0))
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(SendResult::ok(message_id))
    }
}

// ── Axum handler types ───────────────────────────────────────────────

/// Shared state accessible from axum handlers.
#[derive(Clone)]
struct AppState {
    inbound_tx: mpsc::Sender<anyhow::Result<InboundMessage>>,
    app_secret: String,
    webhook_verify_token: String,
    dm_policy: DmPolicy,
    allow_from: Vec<String>,
    /// Message ID dedup map with TTL: msg_id → insert time.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
}

/// Query params for GET webhook verification.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifyQuery {
    #[serde(rename = "hub.mode")]
    hub_mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    hub_verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    hub_challenge: Option<String>,
}

// ── Axum routes ──────────────────────────────────────────────────────

/// Meta webhook subscription verification (GET).
async fn whatsapp_webhook_get(
    State(state): State<AppState>,
    Query(query): Query<VerifyQuery>,
) -> (StatusCode, String) {
    if query.hub_mode.as_deref() != Some("subscribe") {
        return (
            StatusCode::BAD_REQUEST,
            "Missing hub.mode=subscribe".to_string(),
        );
    }
    if query.hub_verify_token.as_deref() != Some(&state.webhook_verify_token) {
        return (StatusCode::FORBIDDEN, "Verify token mismatch".to_string());
    }
    match query.hub_challenge {
        Some(challenge) => {
            tracing::info!("WhatsApp webhook verification succeeded");
            (StatusCode::OK, challenge)
        }
        None => (StatusCode::BAD_REQUEST, "Missing hub.challenge".to_string()),
    }
}

/// Meta webhook event receiver (POST).
async fn whatsapp_webhook_post(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    // 1. Verify signature using raw bytes.
    if let Some(sig) = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
    {
        if !WhatsAppAdapter::verify_signature(&state.app_secret, &body, sig) {
            tracing::warn!("WhatsApp webhook signature verification failed");
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "signature mismatch"})),
            );
        }
    } else if !state.app_secret.is_empty() {
        // app_secret configured but no signature header — reject as likely malicious.
        tracing::warn!(
            "WhatsApp webhook missing X-Hub-Signature-256 header (app_secret configured)"
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "missing signature header"})),
        );
    } else {
        tracing::debug!(
            "WhatsApp webhook: app_secret not configured, skipping signature verification"
        );
    }

    // 2. Parse the outer payload from raw bytes.
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse WhatsApp webhook body");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid json"})),
            );
        }
    };

    // 3. Drill into entry[].changes[].value.messages[].
    if let Some(entries) = payload.get("entry").and_then(|e| e.as_array()) {
        for entry in entries {
            if let Some(changes) = entry.get("changes").and_then(|c| c.as_array()) {
                for change in changes {
                    if let Some(value) = change.get("value") {
                        // Check for status updates — we ignore them.
                        if value.get("statuses").is_some() {
                            continue;
                        }
                        if let Some(messages) = value.get("messages").and_then(|m| m.as_array()) {
                            for msg in messages {
                                if let Some(inbound) = WhatsAppAdapter::parse_message(msg) {
                                    // ── Dedup check ────────────────────────────────
                                    {
                                        let mut seen = state.seen_messages.lock().await;
                                        let now = Instant::now();
                                        let cutoff = now - Duration::from_secs(DEDUP_TTL_SECONDS);
                                        if let Some(ts) = seen.get(&inbound.message_id) {
                                            if *ts >= cutoff {
                                                tracing::debug!(
                                                    msg_id = %inbound.message_id,
                                                    "WhatsApp dedup: skipping duplicate"
                                                );
                                                continue;
                                            }
                                        }
                                        seen.insert(inbound.message_id.clone(), now);
                                        // Evict oldest entries if over capacity.
                                        if seen.len() > DEDUP_MAX_SIZE {
                                            let mut entries: Vec<_> = seen.iter().collect();
                                            entries.sort_by_key(|(_, ts)| **ts);
                                            let keep = entries.len() - DEDUP_MAX_SIZE / 2;
                                            let keys_to_remove: Vec<_> = entries
                                                .iter()
                                                .take(keep)
                                                .map(|(k, _)| (*k).clone())
                                                .collect();
                                            for k in keys_to_remove {
                                                seen.remove(&k);
                                            }
                                        }
                                    }

                                    // DM policy check.
                                    if !inbound.is_group {
                                        if !state.should_process_dm_inner(&inbound.sender_id) {
                                            tracing::debug!(
                                                sender = %inbound.sender_id,
                                                "WhatsApp DM filtered by policy"
                                            );
                                            continue;
                                        }
                                    }

                                    if let Err(e) = state.inbound_tx.send(Ok(inbound)).await {
                                        tracing::warn!(
                                            error = %e,
                                            "failed to forward WhatsApp inbound message"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (StatusCode::OK, Json(json!({})))
}

impl AppState {
    /// Inner helper for DM policy check within handler context.
    fn should_process_dm_inner(&self, sender: &str) -> bool {
        match self.dm_policy {
            DmPolicy::Disabled => false,
            DmPolicy::Allowlist => self.allow_from.iter().any(|a| a == sender),
            DmPolicy::Open => true,
        }
    }
}

// ── PlatformAdapter impl ─────────────────────────────────────────────

#[async_trait::async_trait]
impl PlatformAdapter for WhatsAppAdapter {
    async fn connect(&mut self) -> anyhow::Result<()> {
        let app_state = AppState {
            inbound_tx: self.inbound_tx.clone(),
            app_secret: self.config.app_secret.clone(),
            webhook_verify_token: self.config.webhook_verify_token.clone(),
            dm_policy: self.config.dm_policy,
            allow_from: self.config.allow_from.clone(),
            seen_messages: Arc::clone(&self.seen_messages),
        };

        let app = Router::new()
            .route("/webhook/whatsapp", get(whatsapp_webhook_get))
            .route("/webhook/whatsapp", post(whatsapp_webhook_post))
            .with_state(app_state);

        let addr = format!("{}:{}", self.config.webhook_host, self.config.webhook_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!(addr = %addr, "WhatsApp webhook server starting");

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        self.shutdown_tx = Some(shutdown_tx);

        self.server_handle = Some(tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap_or_else(|e| {
                    tracing::error!(error = %e, "WhatsApp webhook server error");
                });
        }));

        Ok(())
    }

    async fn disconnect(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.server_handle.take() {
            h.abort();
        }
        tracing::info!("WhatsApp adapter disconnected");
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> anyhow::Result<SendResult> {
        // Split if exceeding max length — chain subsequent chunks as replies.
        if text.len() > MAX_WHATSAPP_MESSAGE_LENGTH {
            let chunks =
                crate::gateway::message_formatter::format_and_split(text, Platform::WhatsApp);
            let mut last_result = SendResult::ok(None);
            for (i, chunk) in chunks.into_iter().enumerate() {
                let reply_to = if i == 0 {
                    None
                } else {
                    last_result.message_id.as_deref()
                };
                last_result = self.whatsapp_send_text(chat_id, &chunk, reply_to).await?;
                if !last_result.success {
                    return Ok(last_result);
                }
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
            Ok(last_result)
        } else {
            self.whatsapp_send_text(chat_id, text, None).await
        }
    }

    async fn send_media(
        &self,
        _chat_id: &str,
        _file_path: &std::path::Path,
        _media_type: super::weixin_media::UploadMediaType,
    ) -> anyhow::Result<SendResult> {
        // WhatsApp Cloud API media sending via URL requires uploading to
        // a public URL first. Deferred for a follow-up.
        Ok(SendResult::err(
            "WhatsApp media sending not yet implemented",
        ))
    }

    async fn send_typing(&self, _chat_id: &str) -> anyhow::Result<()> {
        // WhatsApp Cloud API does not support typing indicators.
        Ok(())
    }

    async fn stop_typing(&self, _chat_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn poll_messages(
        &self,
    ) -> Pin<Box<dyn Stream<Item = anyhow::Result<InboundMessage>> + Send + '_>> {
        let inbound_rx = self.inbound_rx.clone();
        Box::pin(stream! {
            let mut rx = inbound_rx.lock().await;
            if let Some(mut receiver) = rx.take() {
                while let Some(item) = receiver.recv().await {
                    yield item;
                }
            } else {
                tracing::error!("WhatsApp poll_messages called more than once; no receiver available");
            }
        })
    }

    fn platform(&self) -> Platform {
        Platform::WhatsApp
    }
}
