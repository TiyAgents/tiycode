//! Feishu/Lark platform adapter.
//!
//! Connects via Webhook HTTP server to receive event pushes from the Feishu
//! open platform, and sends messages via the Feishu REST API
//! (`im/v1/messages`). Supports:
//! - Tenant access token auto-refresh
//! - Event payload signature verification (encrypt_key + verification_token)
//! - Message dedup with TTL (mirrors wecom.rs pattern)
//! - Typing indicator via reaction API
//! - Group policy: open / allowlist / blacklist / disabled

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_stream::stream;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::gateway::config::{FeishuConfig, GroupPolicy};
use crate::gateway::traits::{InboundMessage, Platform, PlatformAdapter, SendResult};

// ── Constants ────────────────────────────────────────────────────────

/// Maximum message length for Feishu post-type messages.
const MAX_FEISHU_MESSAGE_LENGTH: usize = 6000;
/// Tenant access token refresh interval — refresh 5 minutes before expiry.
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(300);
/// Reaction emoji for typing indicator.
const REACTION_TYPING: &str = "Typing";
/// Reaction emoji for failure.
const _REACTION_FAILURE: &str = "CrossMark";
/// Dedup TTL in seconds (24 hours).
const DEDUP_TTL_SECONDS: u64 = 86_400;
/// Maximum entries in the dedup map before eviction.
const DEDUP_MAX_SIZE: usize = 2048;
/// Feishu reply error codes that indicate the reply target is withdrawn or missing.
const FEISHU_REPLY_FALLBACK_CODES: [i64; 2] = [230011, 231003];

// ── Bot identity ─────────────────────────────────────────────────────

/// Bot identity used for self-echo filtering (single lock instead of two).
struct BotIdentity {
    open_id: String,
    user_id: String,
}

// ── Feishu adapter ───────────────────────────────────────────────────

pub struct FeishuAdapter {
    config: FeishuConfig,
    /// axum server shutdown signal.
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    server_handle: Option<JoinHandle<()>>,
    /// Inbound message channel (writer held by axum handlers).
    inbound_tx: mpsc::Sender<anyhow::Result<InboundMessage>>,
    /// Inbound message channel (reader consumed by poll_messages).
    inbound_rx: Arc<Mutex<Option<mpsc::Receiver<anyhow::Result<InboundMessage>>>>>,
    /// Background token-refresh task handle.
    token_handle: Option<JoinHandle<()>>,
    /// Current tenant_access_token.
    current_token: Arc<Mutex<String>>,
    /// Token expiry instant.
    token_expires_at: Arc<Mutex<Instant>>,
    /// HTTP client for Feishu API calls.
    http: reqwest::Client,
    /// Message ID dedup map with TTL: msg_id → insert time.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    /// Bot identity (open_id + user_id) for self-echo filtering.
    bot_identity: Arc<Mutex<BotIdentity>>,
    /// Optional last sent message_id per chat (for reaction-based typing).
    last_sent_message_ids: Arc<Mutex<HashMap<String, String>>>,
}

impl FeishuAdapter {
    pub fn new(config: FeishuConfig) -> Self {
        let (inbound_tx, inbound_rx) = mpsc::channel(256);
        Self {
            config,
            shutdown_tx: None,
            server_handle: None,
            inbound_tx,
            inbound_rx: Arc::new(Mutex::new(Some(inbound_rx))),
            token_handle: None,
            current_token: Arc::new(Mutex::new(String::new())),
            token_expires_at: Arc::new(Mutex::new(Instant::now())),
            http: reqwest::Client::new(),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            bot_identity: Arc::new(Mutex::new(BotIdentity {
                open_id: String::new(),
                user_id: String::new(),
            })),
            last_sent_message_ids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // ── Token management ──────────────────────────────────────────

    /// Fetch a new tenant_access_token from the Feishu API and cache it.
    async fn fetch_tenant_token(
        http: &reqwest::Client,
        config: &FeishuConfig,
    ) -> anyhow::Result<(String, u64)> {
        let url = format!(
            "https://{}/open-apis/auth/v3/tenant_access_token/internal",
            config.domain
        );
        let body = json!({
            "app_id": config.app_id,
            "app_secret": config.app_secret,
        });
        let resp: Value = http
            .post(&url)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = resp
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu token error: code={code}, msg={msg}");
        }

        let token = resp
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let expire = resp.get("expire").and_then(|v| v.as_u64()).unwrap_or(7200);

        Ok((token, expire))
    }

    /// Ensure we have a valid token, refresh if near expiry.
    async fn ensure_token(&self) -> anyhow::Result<String> {
        let expires_at = *self.token_expires_at.lock().await;
        if Instant::now() + TOKEN_REFRESH_MARGIN >= expires_at {
            let (token, expire) = Self::fetch_tenant_token(&self.http, &self.config).await?;
            *self.current_token.lock().await = token.clone();
            *self.token_expires_at.lock().await = Instant::now() + Duration::from_secs(expire);
            Ok(token)
        } else {
            Ok(self.current_token.lock().await.clone())
        }
    }

    // ── Dedup ─────────────────────────────────────────────────────

    // ── Bot identity ─────────────────────────────────────────────

    /// Fetch bot's own open_id and user_id via /open-apis/bot/v3/info for self-echo filtering.
    async fn fetch_bot_identity(
        http: &reqwest::Client,
        config: &FeishuConfig,
        token: &str,
    ) -> anyhow::Result<(String, String)> {
        let url = format!("https://{}/open-apis/bot/v3/info", config.domain);
        let resp: Value = http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?
            .json()
            .await?;

        let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = resp
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu bot info error: code={code}, msg={msg}");
        }

        let bot = resp.get("bot").cloned().unwrap_or_default();
        let open_id = bot
            .get("open_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let user_id = bot
            .get("user_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok((open_id, user_id))
    }

    /// Check if a sender is the bot itself (self-echo defense).
    fn is_self_sender(sender_id: &str, open_id: &str, user_id: &str) -> bool {
        if !open_id.is_empty() && sender_id == open_id {
            return true;
        }
        if !user_id.is_empty() && sender_id == user_id {
            return true;
        }
        false
    }

    // ── Signature verification ───────────────────────────────────

    /// Verify the Feishu webhook signature (`X-Lark-Signature` header).
    /// Algorithm per official docs: `SHA-256(timestamp + nonce + encrypt_key + body)`
    /// NOT HMAC — all official SDK samples (Python/Go/Java) use plain SHA-256.
    /// Uses timing-safe comparison via `subtle::ConstantTimeEq`.
    fn verify_lark_signature(
        encrypt_key: &str,
        timestamp: &str,
        nonce: &str,
        body: &[u8],
        x_lark_signature: &str,
    ) -> bool {
        // Build sign base: timestamp || nonce || encrypt_key || body (raw bytes).
        let mut hasher = Sha256::new();
        hasher.update(timestamp.as_bytes());
        hasher.update(nonce.as_bytes());
        hasher.update(encrypt_key.as_bytes());
        hasher.update(body);
        let result = hasher.finalize();

        // Timing-safe comparison.
        use subtle::ConstantTimeEq;
        match hex::decode(x_lark_signature) {
            Ok(sig_bytes) => {
                if sig_bytes.len() != result.len() {
                    return false;
                }
                result.as_slice().ct_eq(&sig_bytes).into()
            }
            Err(_) => false,
        }
    }

    // ── Markdown post construction ───────────────────────────────

    /// Build post rows with `tag: "md"` for rich message display.
    /// When the content contains code blocks (```), they are isolated into
    /// separate rows so that the Feishu Markdown renderer doesn't swallow
    /// text after a fence boundary.
    fn build_markdown_post_rows(content: &str) -> Vec<Vec<Value>> {
        if content.is_empty() {
            return vec![vec![json!({ "tag": "md", "text": "" })]];
        }
        if !content.contains("```") {
            return vec![vec![json!({ "tag": "md", "text": content })]];
        }

        // Scan line-by-line; split at fence boundaries into isolated rows.
        let mut rows: Vec<Vec<Value>> = Vec::new();
        let mut current_lines: Vec<&str> = Vec::new();
        let mut in_code_block = false;

        macro_rules! flush {
            ($lines:expr, $rows:expr) => {
                if !$lines.is_empty() {
                    let segment = $lines.join("\n");
                    if !segment.trim().is_empty() {
                        $rows.push(vec![json!({ "tag": "md", "text": segment })]);
                    }
                    $lines.clear();
                }
            };
        }

        for raw_line in content.lines() {
            let stripped = raw_line.trim();
            let is_fence = stripped.starts_with("```");

            if is_fence {
                if !in_code_block {
                    flush!(current_lines, rows);
                }
                current_lines.push(raw_line);
                in_code_block = !in_code_block;
                if !in_code_block {
                    flush!(current_lines, rows);
                }
                continue;
            }
            current_lines.push(raw_line);
        }
        flush!(current_lines, rows);

        if rows.is_empty() {
            vec![vec![json!({ "tag": "md", "text": content })]]
        } else {
            rows
        }
    }

    // ── Message parsing ───────────────────────────────────────────

    /// Parse a Feishu message event into an InboundMessage.
    fn parse_message_event(event: &Value) -> Option<InboundMessage> {
        let msg = event.get("message")?;
        let sender = event.get("sender")?;
        let sender_id = sender
            .get("sender_id")
            .and_then(|id| {
                id.get("union_id")
                    .or_else(|| id.get("user_id"))
                    .or_else(|| id.get("open_id"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let chat_id = msg
            .get("chat_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let message_id = msg
            .get("message_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let msg_type = msg
            .get("message_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        // Determine chat type.
        let chat_type = msg
            .get("chat_type")
            .and_then(|v| v.as_str())
            .unwrap_or("p2p");
        let is_group = chat_type == "group";

        // Extract text content based on message type.
        let text = match msg_type {
            "text" => {
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("{}");
                let content_val: Value = serde_json::from_str(content).unwrap_or_default();
                content_val
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string()
            }
            "post" => {
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("{}");
                Self::extract_post_text(content)
            }
            "image" => {
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("{}");
                let content_val: Value = serde_json::from_str(content).unwrap_or_default();
                let image_key = content_val
                    .get("image_key")
                    .and_then(|k| k.as_str())
                    .unwrap_or("");
                if image_key.is_empty() {
                    "[image]".to_string()
                } else {
                    format!("[image: {image_key}]")
                }
            }
            "file" | "audio" | "media" => {
                format!("[{msg_type} attachment]")
            }
            _ => {
                tracing::debug!(msg_type, "unsupported Feishu message type, skipping");
                return None;
            }
        };

        if text.is_empty() {
            return None;
        }

        // Derive chat_id for DM: fall back to sender open_id if chat_id is empty.
        let effective_chat_id = if chat_id.is_empty() {
            sender
                .get("sender_id")
                .and_then(|id| id.get("open_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(&sender_id)
                .to_string()
        } else {
            chat_id
        };

        Some(InboundMessage {
            message_id,
            sender_id,
            chat_id: effective_chat_id,
            text,
            is_group,
            media_urls: vec![],
            media_attachments: vec![],
            req_id: None,
        })
    }

    /// Extract text from a Feishu post (rich text) content.
    fn extract_post_text(content: &str) -> String {
        let content_val: Value = serde_json::from_str(content).unwrap_or_default();
        // Post structure: { "title": "...", "content": [[{tag: "text", text: "..."}, ...]] }
        let mut parts = Vec::new();

        if let Some(title) = content_val.get("title").and_then(|v| v.as_str()) {
            if !title.is_empty() {
                parts.push(title.to_string());
            }
        }

        if let Some(rows) = content_val.get("content").and_then(|c| c.as_array()) {
            for row in rows {
                if let Some(elements) = row.as_array() {
                    let mut line = String::new();
                    for elem in elements {
                        let tag = elem.get("tag").and_then(|t| t.as_str()).unwrap_or("");
                        match tag {
                            "text" => {
                                let text = elem.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                line.push_str(text);
                            }
                            "a" => {
                                let text = elem.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                let href = elem.get("href").and_then(|h| h.as_str()).unwrap_or("");
                                if !href.is_empty() {
                                    line.push_str(&format!("[{text}]({href})"));
                                } else {
                                    line.push_str(text);
                                }
                            }
                            "at" => {
                                let name = elem
                                    .get("user_name")
                                    .or_else(|| elem.get("user_id"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("@user");
                                line.push_str(&format!("@{name}"));
                            }
                            "img" | "image" => {
                                line.push_str("[image]");
                            }
                            "media" | "file" | "audio" | "video" => {
                                line.push_str(&format!("[{tag}]"));
                            }
                            "emoji" | "emotion" => {
                                if let Some(em) = elem.get("emoji").and_then(|e| e.as_str()) {
                                    line.push_str(em);
                                }
                            }
                            "br" => {
                                line.push('\n');
                            }
                            "hr" => {
                                line.push_str("\n---\n");
                            }
                            _ => {
                                // Fallback: try to extract "text" field
                                if let Some(text) = elem.get("text").and_then(|t| t.as_str()) {
                                    line.push_str(text);
                                }
                            }
                        }
                    }
                    if !line.is_empty() {
                        parts.push(line);
                    }
                }
            }
        }

        parts.join("\n")
    }

    // ── Group policy check ────────────────────────────────────────

    /// Returns `true` if a group message should be processed.
    #[allow(dead_code)]
    fn should_process_group_message(&self, chat_id: &str) -> bool {
        match self.config.group_policy {
            GroupPolicy::Disabled => false,
            GroupPolicy::Allowlist => self.config.group_allowlist.iter().any(|g| g == chat_id),
            GroupPolicy::Blacklist => !self.config.group_blacklist.iter().any(|g| g == chat_id),
            // GroupPolicy::Open → allow
            GroupPolicy::Open => true,
        }
    }

    // ── Feishu API calls ──────────────────────────────────────────

    /// Internal: send a new message (no reply) via the Feishu IM API.
    async fn feishu_send_new_message(
        &self,
        chat_id: &str,
        msg_type: &str,
        content: &str,
    ) -> anyhow::Result<SendResult> {
        let token = self.ensure_token().await?;
        let domain = &self.config.domain;
        let url = format!("https://{domain}/open-apis/im/v1/messages?receive_id_type=chat_id");
        let body = json!({
            "receive_id": chat_id,
            "msg_type": msg_type,
            "content": content,
        });
        let resp: Value = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        Self::parse_send_result(&resp)
    }

    /// Send (or reply to) a message via the Feishu IM API.
    async fn feishu_send_message(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> anyhow::Result<SendResult> {
        let token = self.ensure_token().await?;
        let domain = &self.config.domain;

        // Build message payload: use post with tag=md for rich content,
        // plain text for short simple messages.
        let (msg_type, content) = if text.len() > 2000 || text.contains("```") {
            let rows = Self::build_markdown_post_rows(text);
            let post_content = json!({
                "zh_cn": {
                    "title": "",
                    "content": rows,
                }
            });
            ("post", post_content.to_string())
        } else {
            ("text", json!({ "text": text }).to_string())
        };

        // Helper: try reply first; on specific error codes (230011/231003 —
        // reply target withdrawn or missing), fall back to creating a new message.
        if let Some(parent_id) = reply_to {
            let url = format!("https://{domain}/open-apis/im/v1/messages/{parent_id}/reply");
            let body = json!({
                "msg_type": msg_type,
                "content": content,
            });
            let resp: Value = self
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json; charset=utf-8")
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            let result = Self::parse_send_result(&resp);

            // If reply failed with a fallback-eligible error, retry as a new message.
            if let Ok(ref sr) = result {
                if !sr.success {
                    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
                    if FEISHU_REPLY_FALLBACK_CODES.contains(&code) {
                        tracing::warn!(
                            code,
                            parent_id,
                            "Feishu reply failed (target withdrawn/missing), falling back to new message"
                        );
                        let fallback_result = self
                            .feishu_send_new_message(chat_id, msg_type, &content)
                            .await?;
                        if let Some(ref mid) = fallback_result.message_id {
                            self.last_sent_message_ids
                                .lock()
                                .await
                                .insert(chat_id.to_string(), mid.clone());
                        }
                        return Ok(fallback_result);
                    }
                }
            }

            // Track the sent message_id for typing reaction cleanup.
            if let Ok(ref sr) = result {
                if let Some(ref mid) = sr.message_id {
                    self.last_sent_message_ids
                        .lock()
                        .await
                        .insert(chat_id.to_string(), mid.clone());
                }
            }

            return result;
        }

        // Create a new message in chat.
        let result = self
            .feishu_send_new_message(chat_id, msg_type, &content)
            .await?;

        // Try stripping markdown on post-type content format error.
        if !result.success && msg_type == "post" {
            let plain_text = Self::strip_markdown(text);
            if plain_text != text {
                tracing::warn!("Feishu post content rejected, retrying as plain text");
                let fallback_result = self
                    .feishu_send_new_message(
                        chat_id,
                        "text",
                        &json!({ "text": plain_text }).to_string(),
                    )
                    .await?;
                if let Some(ref mid) = fallback_result.message_id {
                    self.last_sent_message_ids
                        .lock()
                        .await
                        .insert(chat_id.to_string(), mid.clone());
                }
                return Ok(fallback_result);
            }
        }

        if let Some(ref mid) = result.message_id {
            self.last_sent_message_ids
                .lock()
                .await
                .insert(chat_id.to_string(), mid.clone());
        }

        Ok(result)
    }

    /// Strip Markdown formatting for a safe plain-text fallback.
    fn strip_markdown(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut in_code_block = false;
        for line in text.lines() {
            if line.trim().starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }
            if in_code_block {
                out.push_str(line);
                out.push('\n');
                continue;
            }
            // Strip inline formatting.
            let stripped = line
                .replace("**", "")
                .replace("__", "")
                .replace("*", "")
                .replace("_", "")
                .replace("`", "")
                .replace("~~", "");
            out.push_str(&stripped);
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }

    fn parse_send_result(resp: &Value) -> anyhow::Result<SendResult> {
        let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = resp
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Ok(SendResult::err(format!(
                "Feishu API error: code={code}, msg={msg}"
            )));
        }
        let message_id = resp
            .get("data")
            .and_then(|d| d.get("message_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(SendResult::ok(message_id))
    }

    /// Add a reaction emoji to the latest sent message in a chat.
    async fn add_reaction(&self, chat_id: &str, emoji: &str) -> anyhow::Result<()> {
        let token = self.ensure_token().await?;
        let message_id = self
            .last_sent_message_ids
            .lock()
            .await
            .get(chat_id)
            .cloned();

        let message_id = match message_id {
            Some(id) => id,
            None => return Ok(()), // No message to react to.
        };

        let domain = &self.config.domain;
        let url = format!("https://{domain}/open-apis/im/v1/messages/{message_id}/reactions");
        let body = json!({ "reaction_type": { "emoji": emoji } });
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            tracing::debug!(status = %resp.status(), "Feishu add-reaction non-2xx (best-effort)");
        }
        Ok(())
    }

    /// Delete a reaction emoji from the latest sent message in a chat.
    async fn delete_reaction(&self, chat_id: &str, emoji: &str) -> anyhow::Result<()> {
        let token = self.ensure_token().await?;
        let message_id = self
            .last_sent_message_ids
            .lock()
            .await
            .get(chat_id)
            .cloned();

        let message_id = match message_id {
            Some(id) => id,
            None => return Ok(()),
        };

        let domain = &self.config.domain;
        let url = format!(
            "https://{domain}/open-apis/im/v1/messages/{message_id}/reactions?reaction_type.emoji={emoji}"
        );
        let resp = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;

        if !resp.status().is_success() {
            tracing::debug!(status = %resp.status(), "Feishu delete-reaction non-2xx (best-effort)");
        }
        Ok(())
    }

    // ── Decrypt Feishu event payload ──────────────────────────────

    /// Decrypt the Feishu event payload when encrypt_key is configured.
    /// Feishu uses AES-256-CBC with key = SHA256(encrypt_key).
    fn decrypt_event_payload(encrypt_key: &str, encrypted_data: &str) -> anyhow::Result<String> {
        use base64::Engine;
        let key = {
            let hash = <Sha256 as Digest>::digest(encrypt_key.as_bytes());
            hash[..].to_vec()
        };
        let encrypted_bytes = base64::engine::general_purpose::STANDARD.decode(encrypted_data)?;

        // AES-256-CBC: IV = first 16 bytes, ciphertext = rest.
        if encrypted_bytes.len() < 16 {
            anyhow::bail!("encrypted payload too short");
        }
        let iv = &encrypted_bytes[..16];
        let ciphertext = &encrypted_bytes[16..];

        use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
        type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

        let decryptor = Aes256CbcDec::new_from_slices(key.as_slice(), iv)?;
        let decrypted = decryptor
            .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
            .map_err(|e| anyhow::anyhow!("AES decrypt failed: {e}"))?;

        String::from_utf8(decrypted)
            .map_err(|e| anyhow::anyhow!("decrypted payload is not valid UTF-8: {e}"))
    }
}

// ── Axum handler types ───────────────────────────────────────────────

/// Shared state accessible from axum handlers.
#[derive(Clone)]
struct AppState {
    inbound_tx: mpsc::Sender<anyhow::Result<InboundMessage>>,
    verification_token: Option<String>,
    encrypt_key: Option<String>,
    /// Dedup map shared from adapter.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    /// Bot identity for self-echo filtering.
    bot_identity: Arc<Mutex<BotIdentity>>,
    // Group policy fields.
    group_policy: GroupPolicy,
    group_allowlist: Vec<String>,
    group_blacklist: Vec<String>,
}

/// Feishu event subscription challenge request.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FeishuChallengeRequest {
    challenge: String,
    token: String,
    #[serde(rename = "type")]
    event_type: String,
}

/// Feishu event payload (outer wrapper).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FeishuEventPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    event: Option<Value>,
    token: Option<String>,
    encrypt: Option<String>,
}

// ── Axum routes ──────────────────────────────────────────────────────

async fn feishu_webhook_post(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    // 0. Verify X-Lark-Signature if encrypt_key is configured.
    if let Some(ref encrypt_key) = state.encrypt_key {
        if !encrypt_key.is_empty() {
            let x_lark_signature = headers
                .get("X-Lark-Signature")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let timestamp = headers
                .get("X-Lark-Request-Timestamp")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let nonce = headers
                .get("X-Lark-Request-Nonce")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if x_lark_signature.is_empty() || timestamp.is_empty() {
                tracing::warn!("Feishu webhook missing X-Lark-Signature or timestamp header");
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "missing signature headers"})),
                );
            }

            if !FeishuAdapter::verify_lark_signature(
                encrypt_key,
                timestamp,
                nonce,
                &body,
                x_lark_signature,
            ) {
                tracing::warn!("Feishu webhook X-Lark-Signature verification failed");
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "signature mismatch"})),
                );
            }
        }
    }

    // 1. Parse body to JSON.
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse Feishu webhook body");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid json"})),
            );
        }
    };

    // 2. Handle encrypted payload.
    let payload = if let Some(encrypted) = payload.get("encrypt").and_then(|v| v.as_str()) {
        match &state.encrypt_key {
            Some(key) if !key.is_empty() => {
                match FeishuAdapter::decrypt_event_payload(key, encrypted) {
                    Ok(decrypted) => match serde_json::from_str::<Value>(&decrypted) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to parse decrypted Feishu payload");
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error": "bad encrypted payload"})),
                            );
                        }
                    },
                    Err(e) => {
                        tracing::warn!(error = %e, "Feishu payload decryption failed");
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error": "decryption failed"})),
                        );
                    }
                }
            }
            _ => {
                tracing::warn!("received encrypted Feishu payload but no encrypt_key configured");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "encryption key not configured"})),
                );
            }
        }
    } else {
        payload
    };

    // 2. Handle URL verification challenge.
    if let (Some(challenge), Some(token), Some(evt_type)) = (
        payload.get("challenge").and_then(|v| v.as_str()),
        payload.get("token").and_then(|v| v.as_str()),
        payload.get("type").and_then(|v| v.as_str()),
    ) {
        if evt_type == "url_verification" {
            // Optionally verify token.
            if let Some(ref expected) = state.verification_token {
                if token != expected {
                    tracing::warn!("Feishu URL verification token mismatch");
                    return (
                        StatusCode::FORBIDDEN,
                        Json(json!({"error": "token mismatch"})),
                    );
                }
            }
            tracing::info!("Feishu URL verification challenge accepted");
            return (StatusCode::OK, Json(json!({ "challenge": challenge })));
        }
    }

    // 3. Verify token for normal events.
    if let Some(ref expected) = state.verification_token {
        if let Some(token) = payload.get("token").and_then(|v| v.as_str()) {
            if token != expected {
                tracing::warn!("Feishu event token mismatch");
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({"error": "token mismatch"})),
                );
            }
        }
    }

    // 4. Dispatch event.
    if let Some(event) = payload.get("event") {
        let event_type = event
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if event_type == "im.message.receive_v1" {
            if let Some(msg) = FeishuAdapter::parse_message_event(event) {
                // Self-echo filtering: skip messages from the bot itself.
                let identity = state.bot_identity.lock().await;
                if FeishuAdapter::is_self_sender(
                    &msg.sender_id,
                    &identity.open_id,
                    &identity.user_id,
                ) {
                    tracing::debug!(sender = %msg.sender_id, "Feishu self-echo filtered");
                    return (StatusCode::OK, Json(json!({})));
                }

                // Dedup: skip already-seen messages.
                if !msg.message_id.is_empty() {
                    let now = Instant::now();
                    let mut seen = state.seen_messages.lock().await;
                    if let Some(t) = seen.get(&msg.message_id) {
                        if now.duration_since(*t).as_secs() < DEDUP_TTL_SECONDS {
                            tracing::debug!(message_id = %msg.message_id, "Feishu duplicate message filtered");
                            return (StatusCode::OK, Json(json!({})));
                        }
                    }
                    seen.insert(msg.message_id.clone(), now);
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

                // Group policy check.
                if msg.is_group {
                    let allowed = match state.group_policy {
                        GroupPolicy::Disabled => false,
                        GroupPolicy::Allowlist => {
                            state.group_allowlist.iter().any(|g| g == &msg.chat_id)
                        }
                        GroupPolicy::Blacklist => {
                            !state.group_blacklist.iter().any(|g| g == &msg.chat_id)
                        }
                        GroupPolicy::Open => true,
                    };
                    if !allowed {
                        tracing::debug!(chat_id = %msg.chat_id, "Feishu group message filtered by policy");
                        return (StatusCode::OK, Json(json!({})));
                    }
                }

                if let Err(e) = state.inbound_tx.send(Ok(msg)).await {
                    tracing::warn!(error = %e, "failed to forward Feishu inbound message");
                }
            }
        } else {
            tracing::debug!(event_type, "ignoring non-message Feishu event");
        }
    }

    (StatusCode::OK, Json(json!({})))
}

// ── PlatformAdapter impl ─────────────────────────────────────────────

#[async_trait::async_trait]
impl PlatformAdapter for FeishuAdapter {
    async fn connect(&mut self) -> anyhow::Result<()> {
        // 1. Fetch initial tenant access token.
        let (token, expire) = Self::fetch_tenant_token(&self.http, &self.config).await?;
        *self.current_token.lock().await = token;
        *self.token_expires_at.lock().await = Instant::now() + Duration::from_secs(expire);
        tracing::info!("Feishu tenant_access_token acquired, expires in {expire}s");

        // 2. Start token refresh background task.
        {
            let http = self.http.clone();
            let config = self.config.clone();
            let current_token = self.current_token.clone();
            let token_expires_at = self.token_expires_at.clone();
            self.token_handle = Some(tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(600)).await;
                    let expires_at = *token_expires_at.lock().await;
                    if Instant::now() + TOKEN_REFRESH_MARGIN >= expires_at {
                        match Self::fetch_tenant_token(&http, &config).await {
                            Ok((t, exp)) => {
                                *current_token.lock().await = t;
                                *token_expires_at.lock().await =
                                    Instant::now() + Duration::from_secs(exp);
                                tracing::debug!("Feishu tenant_access_token refreshed");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Feishu token refresh failed");
                            }
                        }
                    }
                }
            }));
        }

        // 3. Fetch bot identity for self-echo filtering.
        {
            let current_token = self.current_token.lock().await.clone();
            let (open_id, user_id) = match Self::fetch_bot_identity(
                &self.http,
                &self.config,
                &current_token,
            )
            .await
            {
                Ok(ids) => {
                    tracing::info!(open_id = %ids.0, user_id = %ids.1, "Feishu bot identity acquired");
                    ids
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Feishu bot identity fetch failed, self-echo filtering disabled");
                    (String::new(), String::new())
                }
            };
            *self.bot_identity.lock().await = BotIdentity { open_id, user_id };
        }

        // 4. Start axum webhook server.
        let app_state = AppState {
            inbound_tx: self.inbound_tx.clone(),
            verification_token: self.config.verification_token.clone(),
            encrypt_key: self.config.encrypt_key.clone(),
            seen_messages: Arc::clone(&self.seen_messages),
            bot_identity: Arc::clone(&self.bot_identity),
            group_policy: self.config.group_policy,
            group_allowlist: self.config.group_allowlist.clone(),
            group_blacklist: self.config.group_blacklist.clone(),
        };

        let app = Router::new()
            .route("/webhook/feishu", post(feishu_webhook_post))
            .with_state(app_state);

        let addr = format!("{}:{}", self.config.webhook_host, self.config.webhook_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!(addr = %addr, "Feishu webhook server starting");

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        self.shutdown_tx = Some(shutdown_tx);

        self.server_handle = Some(tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap_or_else(|e| {
                    tracing::error!(error = %e, "Feishu webhook server error");
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
        if let Some(h) = self.token_handle.take() {
            h.abort();
        }
        tracing::info!("Feishu adapter disconnected");
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> anyhow::Result<SendResult> {
        // Split if exceeding max length.
        if text.len() > MAX_FEISHU_MESSAGE_LENGTH {
            let chunks =
                crate::gateway::message_formatter::format_and_split(text, Platform::Feishu);
            let mut last_result = SendResult::ok(None);
            for (i, chunk) in chunks.into_iter().enumerate() {
                let reply_to = if i == 0 {
                    None
                } else {
                    last_result.message_id.as_deref()
                };
                last_result = self.feishu_send_message(chat_id, &chunk, reply_to).await?;
                if !last_result.success {
                    return Ok(last_result);
                }
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
            Ok(last_result)
        } else {
            self.feishu_send_message(chat_id, text, None).await
        }
    }

    async fn send_media(
        &self,
        _chat_id: &str,
        _file_path: &std::path::Path,
        _media_type: super::weixin_media::UploadMediaType,
    ) -> anyhow::Result<SendResult> {
        // Media upload via Feishu API requires two steps: upload file then
        // send message with the file_key. This is deferred for a follow-up.
        Ok(SendResult::err("Feishu media sending not yet implemented"))
    }

    async fn send_typing(&self, chat_id: &str) -> anyhow::Result<()> {
        self.add_reaction(chat_id, REACTION_TYPING).await
    }

    async fn stop_typing(&self, chat_id: &str) -> anyhow::Result<()> {
        self.delete_reaction(chat_id, REACTION_TYPING).await
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
                tracing::error!("Feishu poll_messages called more than once; no receiver available");
            }
        })
    }

    fn platform(&self) -> Platform {
        Platform::Feishu
    }
}
