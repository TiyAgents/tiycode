//! WeChat (iLink Bot) platform adapter.
//!
//! Connects via HTTP long-polling to the iLink Bot API, handles context_token
//! management, message dedup, and text chunking for outbound messages.
//! Implements the iLink Bot HTTP long-poll protocol for message exchange.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_stream::stream;
use base64::Engine;
use futures::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::gateway::config::WeixinConfig;
use crate::gateway::traits::{InboundMessage, Platform, PlatformAdapter, SendResult};

const POLL_TIMEOUT_SECONDS: u64 = 35;
const MAX_SEND_RETRIES: u32 = 4;
const SEND_RETRY_DELAY: Duration = Duration::from_millis(1000);
const MAX_MESSAGE_LENGTH: usize = 2000;
/// Channel protocol version.
const CHANNEL_VERSION: &str = "2.2.0";
/// TTL for message dedup entries (seconds).
const DEDUP_TTL_SECONDS: u64 = 300;
/// Long backoff when session is expired/stale (seconds).
/// 600s long backoff to avoid hammering a dead session.
const SESSION_EXPIRED_BACKOFF_SECONDS: u64 = 600;
/// Rate-limit retry multiplier for errcode=-2.
const RATE_LIMIT_RETRY_MULTIPLIER: u32 = 3;

/// Persistence directory for WeChat state: `~/.tiy/gateway/weixin/`
fn weixin_state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tiy/gateway/weixin")
}

/// WeChat iLink Bot adapter using HTTP long-polling.
pub struct WeixinAdapter {
    config: WeixinConfig,
    http_client: Client,
    /// Resolved bearer token (from config or session file).
    token: Arc<Mutex<String>>,
    /// Resolved account ID (from config or session file).
    account_id: String,
    /// Context token cache: peer_id → token (required for outbound messages).
    context_tokens: Arc<Mutex<HashMap<String, String>>>,
    /// Long-polling sync cursor (persisted across poll cycles).
    sync_buf: Arc<Mutex<Option<String>>>,
    /// Message ID dedup map with TTL: message_id → insert time.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    /// Content fingerprint dedup: hash(text) → insert time.
    /// Catches iLink re-delivery with different msg_id but same content.
    seen_content: Arc<Mutex<HashMap<u64, Instant>>>,
    /// Directory for persisted state files.
    state_dir: PathBuf,
    /// Cached typing_ticket per user (fetched from getconfig API).
    typing_tickets: Arc<Mutex<HashMap<String, String>>>,
    /// Random X-WECHAT-UIN value (stable per adapter instance).
    wechat_uin: String,
}

impl WeixinAdapter {
    pub fn new(config: WeixinConfig) -> Self {
        let state_dir = weixin_state_dir();

        // Load persisted state.
        let sync_buf = Self::load_sync_buf(&state_dir);
        let context_tokens = Self::load_context_tokens(&state_dir);

        // Random u32 → decimal string → base64 for X-WECHAT-UIN.
        let uin_int = u32::from_be_bytes(
            uuid::Uuid::now_v7().as_bytes()[0..4]
                .try_into()
                .unwrap_or([0x12, 0x34, 0x56, 0x78]),
        );
        let wechat_uin =
            base64::engine::general_purpose::STANDARD.encode(uin_int.to_string().as_bytes());

        // Resolve effective token and account_id (config > session file).
        let token = config.effective_token().unwrap_or_default();
        let account_id = config.effective_account_id();

        Self {
            config,
            http_client: Client::builder()
                .timeout(Duration::from_secs(POLL_TIMEOUT_SECONDS + 10))
                .build()
                .unwrap_or_default(),
            token: Arc::new(Mutex::new(token)),
            account_id,
            context_tokens: Arc::new(Mutex::new(context_tokens)),
            sync_buf: Arc::new(Mutex::new(sync_buf)),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            seen_content: Arc::new(Mutex::new(HashMap::new())),
            state_dir,
            typing_tickets: Arc::new(Mutex::new(HashMap::new())),
            wechat_uin,
        }
    }

    // --- Persistence helpers ---

    fn sync_buf_path(dir: &PathBuf) -> PathBuf {
        dir.join("sync_buf.txt")
    }

    fn context_tokens_path(dir: &PathBuf) -> PathBuf {
        dir.join("context_tokens.json")
    }

    fn load_sync_buf(dir: &PathBuf) -> Option<String> {
        std::fs::read_to_string(Self::sync_buf_path(dir))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn load_context_tokens(dir: &PathBuf) -> HashMap<String, String> {
        std::fs::read_to_string(Self::context_tokens_path(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn persist_sync_buf(&self, buf: &str) {
        let _ = std::fs::create_dir_all(&self.state_dir);
        let path = Self::sync_buf_path(&self.state_dir);
        // Atomic write: write to temp then rename.
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, buf).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    fn persist_context_tokens(&self, tokens: &HashMap<String, String>) {
        let _ = std::fs::create_dir_all(&self.state_dir);
        let path = Self::context_tokens_path(&self.state_dir);
        let tmp = path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string(tokens) {
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    /// Build the standard iLink API headers.
    async fn api_headers(&self) -> reqwest::header::HeaderMap {
        let token = self.token.lock().await;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", *token).parse().unwrap(),
        );
        headers.insert("AuthorizationType", "ilink_bot_token".parse().unwrap());
        headers.insert("iLink-App-Id", "bot".parse().unwrap());
        // Version encoding: (2<<16)|(2<<8)|0
        headers.insert("iLink-App-ClientVersion", "131584".parse().unwrap());
        // Random UIN for session identity.
        headers.insert("X-WECHAT-UIN", self.wechat_uin.parse().unwrap());
        headers
    }

    /// Build the full API URL for an endpoint.
    fn api_url(&self, endpoint: &str) -> String {
        format!("https://{}/ilink/bot/{}", self.config.base_url, endpoint)
    }

    /// Perform a single long-poll request to getupdates.
    async fn poll_once(&self) -> anyhow::Result<Vec<InboundMessage>> {
        let sync_buf = self.sync_buf.lock().await.clone();
        let body = json!({
            "base_info": {
                "channel_version": CHANNEL_VERSION,
            },
            "get_updates_buf": sync_buf.as_deref().unwrap_or(""),
        });

        let url = self.api_url("getupdates");
        tracing::info!(
            url = %url,
            has_sync_buf = sync_buf.is_some(),
            "starting getupdates long-poll"
        );

        let resp = self
            .http_client
            .post(&url)
            .headers(self.api_headers().await)
            .json(&body)
            .send()
            .await?;

        let data: Value = resp.json().await?;

        // Log response summary for diagnostics.
        let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
        let ret = data.get("ret").and_then(|v| v.as_i64()).unwrap_or(0);
        let msg_count = data
            .get("msgs")
            .and_then(|v| v.as_array())
            .map_or(0, |a| a.len());
        tracing::info!(errcode, ret, msg_count, "getupdates response");

        if errcode != 0 || (ret != 0 && ret != errcode) {
            let errmsg = data
                .get("errmsg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            if errcode == -14 {
                // Session expired — need re-authentication.
                anyhow::bail!("[session_expired] errcode=-14: {errmsg}");
            }
            if errcode == -2 && errmsg.contains("unknown error") {
                // Stale session (ret=-2 + "unknown error").
                anyhow::bail!("[session_expired] stale session errcode=-2: {errmsg}");
            }
            if errcode == -2 {
                // Rate-limited — signal for extended backoff.
                anyhow::bail!("[rate_limited] errcode=-2: {errmsg}");
            }
            anyhow::bail!("getupdates error {errcode}: {errmsg}");
        }

        // Update sync cursor from response.
        if let Some(new_sync) = data.get("get_updates_buf").and_then(|v| v.as_str()) {
            if !new_sync.is_empty() {
                *self.sync_buf.lock().await = Some(new_sync.to_string());
                self.persist_sync_buf(new_sync);
            }
        }

        // Parse messages from "msgs" array.
        let msgs = data
            .get("msgs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut messages = Vec::new();
        let mut seen = self.seen_messages.lock().await;
        let mut seen_content = self.seen_content.lock().await;

        for update in msgs {
            if let Some(msg) = self.parse_update(&update, &mut seen, &mut seen_content) {
                tracing::info!(
                    msg_id = %msg.message_id,
                    sender = %msg.sender_id,
                    chat_id = %msg.chat_id,
                    text_len = msg.text.len(),
                    "inbound message parsed"
                );
                // Update context token for this sender.
                // Key uses account_id:peer_id composite to support multi-account.
                if let Some(token) = update.get("context_token").and_then(|v| v.as_str()) {
                    let mut tokens = self.context_tokens.lock().await;
                    let key = format!("{}:{}", self.account_id, msg.sender_id);
                    tokens.insert(key, token.to_string());
                    self.persist_context_tokens(&tokens);
                } else {
                    tracing::warn!(msg_id = %msg.message_id, "no context_token in update");
                }
                messages.push(msg);
            }
        }

        Ok(messages)
    }

    /// Parse a single update JSON into an InboundMessage (with dedup).
    fn parse_update(
        &self,
        update: &Value,
        seen: &mut HashMap<String, Instant>,
        seen_content: &mut HashMap<u64, Instant>,
    ) -> Option<InboundMessage> {
        // iLink returns message_id as a number; accept both number and string.
        let msg_id = update.get("message_id").and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        });
        let msg_id = match msg_id {
            Some(id) if !id.is_empty() => id,
            _ => {
                tracing::debug!("parse_update: missing or empty message_id, skipping");
                return None;
            }
        };

        // TTL-based dedup: check if message_id exists and is within TTL window.
        let now = Instant::now();
        if let Some(inserted_at) = seen.get(&msg_id) {
            if now.duration_since(*inserted_at).as_secs() < DEDUP_TTL_SECONDS {
                return None;
            }
        }
        seen.insert(msg_id.clone(), now);

        // Evict expired entries periodically (when map grows large).
        if seen.len() > 2000 {
            seen.retain(|_, t| now.duration_since(*t).as_secs() < DEDUP_TTL_SECONDS);
        }

        // iLink uses "from_user_id" for the sender; fall back to legacy "sender_id".
        let sender_id = update
            .get("from_user_id")
            .or_else(|| update.get("sender_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Skip self messages (bot's own outbound echoes).
        if sender_id.is_empty() || sender_id == self.account_id {
            return None;
        }

        // iLink nests text content in item_list[0].text_item.text.
        // Fall back to a top-level "text" field for compatibility.
        let text = update
            .get("item_list")
            .and_then(|v| v.as_array())
            .and_then(|items| items.first())
            .and_then(|item| item.get("text_item"))
            .and_then(|ti| ti.get("text"))
            .and_then(|t| t.as_str())
            .or_else(|| update.get("text").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();

        if text.is_empty() {
            tracing::debug!(msg_id, "parse_update: empty text content, skipping");
            return None;
        }

        // Content fingerprint dedup: catch re-delivery with different msg_id.
        let content_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            sender_id.hash(&mut hasher);
            text.hash(&mut hasher);
            hasher.finish()
        };
        if let Some(inserted_at) = seen_content.get(&content_hash) {
            if now.duration_since(*inserted_at).as_secs() < DEDUP_TTL_SECONDS {
                tracing::debug!(msg_id, "content fingerprint dedup hit, skipping");
                return None;
            }
        }
        seen_content.insert(content_hash, now);

        // Evict expired content fingerprints.
        if seen_content.len() > 2000 {
            seen_content.retain(|_, t| now.duration_since(*t).as_secs() < DEDUP_TTL_SECONDS);
        }

        // For DMs the reply target is from_user_id; for groups use group_id/session_id.
        let group_id = update
            .get("group_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let is_group = group_id.is_some();
        let chat_id = if is_group {
            group_id.unwrap_or(&sender_id).to_string()
        } else {
            // DM: reply target is the sender.
            sender_id.clone()
        };

        Some(InboundMessage {
            message_id: msg_id,
            sender_id,
            chat_id,
            text,
            is_group,
            media_urls: vec![],
            req_id: None,
        })
    }

    /// Fetch typing_ticket from the getconfig API (cached per user).
    async fn fetch_typing_ticket(&self, chat_id: &str) -> Option<String> {
        // Return cached ticket if available for this user.
        {
            let cached = self.typing_tickets.lock().await;
            if let Some(ticket) = cached.get(chat_id) {
                return Some(ticket.clone());
            }
        }

        // getconfig requires ilink_user_id; pass context_token only if available.
        let token_key = format!("{}:{}", self.account_id, chat_id);
        let context_token = self.context_tokens.lock().await.get(&token_key).cloned();

        // Build body — omit context_token field entirely when not available
        // (iLink may reject empty string).
        let body = if let Some(ref ct) = context_token {
            json!({
                "ilink_user_id": chat_id,
                "context_token": ct,
                "base_info": { "channel_version": CHANNEL_VERSION },
            })
        } else {
            json!({
                "ilink_user_id": chat_id,
                "base_info": { "channel_version": CHANNEL_VERSION },
            })
        };

        let resp = self
            .http_client
            .post(self.api_url("getconfig"))
            .headers(self.api_headers().await)
            .json(&body)
            .send()
            .await
            .ok()?;

        let data: Value = resp.json().await.ok()?;
        let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = data.get("errmsg").and_then(|v| v.as_str()).unwrap_or("");
            tracing::warn!(
                errcode,
                errmsg,
                chat_id,
                "getconfig failed, typing unavailable"
            );
            return None;
        }

        let ticket = data.get("typing_ticket").and_then(|v| v.as_str())?;
        let ticket_str = ticket.to_string();
        self.typing_tickets
            .lock()
            .await
            .insert(chat_id.to_string(), ticket_str.clone());
        Some(ticket_str)
    }

    /// Call sendtyping API with the cached typing_ticket.
    async fn do_send_typing(&self, chat_id: &str) -> anyhow::Result<()> {
        let ticket = match self.fetch_typing_ticket(chat_id).await {
            Some(t) => t,
            None => return Ok(()), // Silently skip if ticket unavailable.
        };

        let body = json!({
            "ilink_user_id": chat_id,
            "typing_ticket": ticket,
            "status": 1,
            "base_info": { "channel_version": CHANNEL_VERSION },
        });

        let resp = self
            .http_client
            .post(self.api_url("sendtyping"))
            .headers(self.api_headers().await)
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let data: Value = r.json().await.unwrap_or_default();
                let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
                if errcode != 0 {
                    // Ticket may have expired — clear cache for this user.
                    self.typing_tickets.lock().await.remove(chat_id);
                    tracing::debug!(errcode, chat_id, "sendtyping failed, cleared ticket cache");
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "sendtyping request failed");
            }
        }

        Ok(())
    }

    /// Send a single text chunk with retry logic.
    async fn send_text_chunk(&self, chat_id: &str, text: &str) -> anyhow::Result<SendResult> {
        // Look up context_token with composite key (account_id:peer_id).
        let token_key = format!("{}:{}", self.account_id, chat_id);
        let context_token = self.context_tokens.lock().await.get(&token_key).cloned();

        // Generate a unique client_id per message (iLink requires distinct client_id
        // for each independent message to render correctly in the WeChat UI).
        let client_id = format!("tiycode-weixin-{}", uuid::Uuid::now_v7());

        tracing::info!(
            to = %chat_id,
            has_token = context_token.is_some(),
            text_len = text.len(),
            "sending message via iLink"
        );

        for attempt in 0..MAX_SEND_RETRIES {
            // Build iLink sendmessage body.
            let ct = if attempt == 0 {
                context_token.clone().unwrap_or_default()
            } else {
                String::new()
            };

            let body = json!({
                "base_info": {
                    "channel_version": CHANNEL_VERSION,
                },
                "msg": {
                    "from_user_id": "",
                    "to_user_id": chat_id,
                    "client_id": client_id,
                    "message_type": 2,
                    "message_state": 2,
                    "context_token": ct,
                    "item_list": [{
                        "type": 1,
                        "text_item": { "text": text }
                    }]
                }
            });

            let resp = self
                .http_client
                .post(self.api_url("sendmessage"))
                .headers(self.api_headers().await)
                .json(&body)
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let data: Value = r.json().await.unwrap_or_default();
                    let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
                    if errcode == 0 {
                        tracing::info!(to = %chat_id, "message sent successfully");
                        return Ok(SendResult::ok(None));
                    }
                    let errmsg = data
                        .get("errmsg")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    tracing::warn!(
                        to = %chat_id,
                        errcode,
                        errmsg,
                        attempt,
                        "sendmessage failed"
                    );
                    if errcode == -14 && attempt == 0 {
                        // Session expired — retry without context_token.
                        tracing::warn!("context_token expired, retrying without token");
                        continue;
                    }
                    if attempt < MAX_SEND_RETRIES - 1 {
                        sleep(SEND_RETRY_DELAY * (attempt + 1)).await;
                        continue;
                    }
                    return Ok(SendResult::err(format!("send failed: {errcode} {errmsg}")));
                }
                Err(e) => {
                    tracing::warn!(to = %chat_id, error = %e, attempt, "sendmessage transport error");
                    if attempt < MAX_SEND_RETRIES - 1 {
                        sleep(SEND_RETRY_DELAY * (attempt + 1)).await;
                        continue;
                    }
                    return Ok(SendResult::err(format!("send failed: {e}")));
                }
            }
        }

        Ok(SendResult::err("max retries exceeded"))
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for WeixinAdapter {
    async fn connect(&mut self) -> anyhow::Result<()> {
        // Validate that a token is available.
        let token = self.token.lock().await;
        if token.is_empty() {
            anyhow::bail!(
                "No iLink token available. Please complete QR login first \
                 (gateway_weixin_qr_login) or set token in config.toml"
            );
        }
        drop(token);

        tracing::info!(
            account_id = %self.account_id,
            base_url = %self.config.base_url,
            "WeChat iLink Bot adapter connecting"
        );
        // No persistent connection needed — long-polling is stateless.
        Ok(())
    }

    async fn disconnect(&mut self) {
        tracing::info!("WeChat iLink Bot adapter disconnected");
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> anyhow::Result<SendResult> {
        // Split into chunks if needed.
        let chunks = split_text(text, MAX_MESSAGE_LENGTH);
        let mut last_result = SendResult::ok(None);

        for (i, chunk) in chunks.iter().enumerate() {
            last_result = self.send_text_chunk(chat_id, chunk).await?;
            if !last_result.success {
                return Ok(last_result);
            }
            // Delay between chunks (except last).
            if i < chunks.len() - 1 {
                sleep(Duration::from_millis(1500)).await;
            }
        }

        Ok(last_result)
    }

    async fn send_typing(&self, chat_id: &str) -> anyhow::Result<()> {
        self.do_send_typing(chat_id).await
    }

    async fn stop_typing(&self, _chat_id: &str) -> anyhow::Result<()> {
        // iLink has no explicit "stop typing" API — typing auto-expires.
        Ok(())
    }

    fn poll_messages(
        &self,
    ) -> Pin<Box<dyn Stream<Item = anyhow::Result<InboundMessage>> + Send + '_>> {
        Box::pin(stream! {
            tracing::info!("poll_messages stream started, beginning long-poll loop");
            let mut consecutive_errors = 0u32;
            loop {
                match self.poll_once().await {
                    Ok(messages) => {
                        consecutive_errors = 0;
                        for msg in messages {
                            yield Ok(msg);
                        }
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        let err_str = e.to_string();
                        tracing::warn!(
                            error = %err_str,
                            consecutive = consecutive_errors,
                            "poll error"
                        );

                        // Determine backoff based on error type.
                        if err_str.contains("[session_expired]") {
                            // Session expired/stale — long backoff to avoid hammering.
                            tracing::warn!(
                                backoff_secs = SESSION_EXPIRED_BACKOFF_SECONDS,
                                "session expired, entering long backoff"
                            );
                            sleep(Duration::from_secs(SESSION_EXPIRED_BACKOFF_SECONDS)).await;
                        } else if err_str.contains("[rate_limited]") {
                            // Rate-limited — use multiplied backoff.
                            let delay = SEND_RETRY_DELAY * RATE_LIMIT_RETRY_MULTIPLIER * (consecutive_errors.min(10));
                            tracing::warn!(delay_ms = delay.as_millis(), "rate limited, backing off");
                            sleep(delay).await;
                        } else if consecutive_errors >= 3 {
                            // Generic errors — exponential backoff capped at 30s.
                            sleep(Duration::from_secs(30)).await;
                        } else {
                            sleep(Duration::from_secs(2)).await;
                        }

                        // Yield the error to let the runner decide.
                        yield Err(e);
                    }
                }
            }
        })
    }

    fn platform(&self) -> Platform {
        Platform::Weixin
    }
}

/// Split text into chunks respecting character count limit.
/// WeChat limits are in characters (not bytes), important for CJK text.
fn split_text(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.chars().count() <= max_chars {
            chunks.push(remaining.to_string());
            break;
        }
        // Find the byte offset for `max_chars` characters.
        let byte_limit: usize = remaining
            .char_indices()
            .nth(max_chars)
            .map(|(idx, _)| idx)
            .unwrap_or(remaining.len());

        // Find a good split point (prefer newline or space) within the limit.
        let split_at = remaining[..byte_limit]
            .rfind('\n')
            .or_else(|| remaining[..byte_limit].rfind(' '))
            .unwrap_or(byte_limit);
        let split_at = if split_at == 0 { byte_limit } else { split_at };

        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..];
        remaining = remaining.strip_prefix('\n').unwrap_or(remaining);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_short_text() {
        let chunks = split_text("hello", 2000);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn split_long_text_at_newline() {
        let text = format!("{}\n{}", "a".repeat(1800), "b".repeat(500));
        let chunks = split_text(&text, 2000);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= 2000);
        assert!(chunks[1].len() <= 2000);
    }
}
