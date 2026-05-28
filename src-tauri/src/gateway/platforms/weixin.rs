//! WeChat (iLink Bot) platform adapter.
//!
//! Connects via HTTP long-polling to the iLink Bot API, handles context_token
//! management, message dedup, and text chunking for outbound messages.
//! Reference: hermes-agent/gateway/platforms/weixin.py

use std::collections::HashMap;
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
/// Channel protocol version aligned with hermes-agent.
const CHANNEL_VERSION: &str = "2.2.0";
/// TTL for message dedup entries (seconds).
const DEDUP_TTL_SECONDS: u64 = 300;

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
    /// Context token cache: peer_id → token (required for outbound messages).
    context_tokens: Arc<Mutex<HashMap<String, String>>>,
    /// Long-polling sync cursor (persisted across poll cycles).
    sync_buf: Arc<Mutex<Option<String>>>,
    /// Message ID dedup map with TTL: message_id → insert time.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    /// Directory for persisted state files.
    state_dir: PathBuf,
    /// Cached typing_ticket (fetched from getconfig API).
    typing_ticket: Arc<Mutex<Option<String>>>,
    /// Client ID for this adapter instance (stable per session).
    client_id: String,
    /// Random X-WECHAT-UIN value (stable per adapter instance).
    wechat_uin: String,
}

impl WeixinAdapter {
    pub fn new(config: WeixinConfig) -> Self {
        let state_dir = weixin_state_dir();

        // Load persisted state.
        let sync_buf = Self::load_sync_buf(&state_dir);
        let context_tokens = Self::load_context_tokens(&state_dir);

        // Generate stable client_id and X-WECHAT-UIN for this session.
        let client_id = format!("hermes-weixin-{}", uuid::Uuid::now_v7());
        let uin_bytes: [u8; 4] = uuid::Uuid::now_v7().as_bytes()[0..4]
            .try_into()
            .unwrap_or([0x12, 0x34, 0x56, 0x78]);
        let wechat_uin = base64::engine::general_purpose::STANDARD.encode(uin_bytes);

        Self {
            config,
            http_client: Client::builder()
                .timeout(Duration::from_secs(POLL_TIMEOUT_SECONDS + 10))
                .build()
                .unwrap_or_default(),
            context_tokens: Arc::new(Mutex::new(context_tokens)),
            sync_buf: Arc::new(Mutex::new(sync_buf)),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            state_dir,
            typing_ticket: Arc::new(Mutex::new(None)),
            client_id,
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
    fn api_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.config.token).parse().unwrap(),
        );
        headers.insert("iLink-App-Id", "bot".parse().unwrap());
        // Version encoding: (2<<16)|(2<<8)|0
        headers.insert("iLink-App-ClientVersion", "131584".parse().unwrap());
        // Random UIN for session identity (aligned with hermes-agent).
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
        let mut body = json!({
            "timeout": POLL_TIMEOUT_SECONDS * 1000,
        });
        if let Some(ref buf) = sync_buf {
            body["sync_buf"] = json!(buf);
        }

        let resp = self
            .http_client
            .post(self.api_url("getupdates"))
            .headers(self.api_headers())
            .json(&body)
            .send()
            .await?;

        let data: Value = resp.json().await?;

        // Check for errors.
        let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
        if errcode != 0 {
            let errmsg = data
                .get("errmsg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            if errcode == -14 {
                // Session expired — need re-authentication.
                anyhow::bail!("session expired (errcode=-14): {errmsg}");
            }
            anyhow::bail!("getupdates error {errcode}: {errmsg}");
        }

        // Update sync_buf cursor.
        if let Some(new_sync) = data.get("sync_buf").and_then(|v| v.as_str()) {
            *self.sync_buf.lock().await = Some(new_sync.to_string());
            self.persist_sync_buf(new_sync);
        }

        // Parse messages.
        let updates = data
            .get("updates")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut messages = Vec::new();
        let mut seen = self.seen_messages.lock().await;

        for update in updates {
            if let Some(msg) = self.parse_update(&update, &mut seen) {
                // Update context token for this sender.
                if let Some(token) = update.get("context_token").and_then(|v| v.as_str()) {
                    let mut tokens = self.context_tokens.lock().await;
                    tokens.insert(msg.sender_id.clone(), token.to_string());
                    self.persist_context_tokens(&tokens);
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
    ) -> Option<InboundMessage> {
        let msg_id = update.get("message_id")?.as_str()?.to_string();

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

        let sender_id = update
            .get("sender_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Skip self messages.
        if sender_id == self.config.account_id {
            return None;
        }

        let text = update
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if text.is_empty() {
            return None;
        }

        let chat_id = update
            .get("chat_id")
            .or_else(|| update.get("room_id"))
            .and_then(|v| v.as_str())
            .unwrap_or(&sender_id)
            .to_string();
        let is_group = update
            .get("room_id")
            .or_else(|| update.get("chat_room_id"))
            .is_some();

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

    /// Fetch typing_ticket from the getconfig API (cached).
    async fn fetch_typing_ticket(&self) -> Option<String> {
        // Return cached ticket if available.
        {
            let cached = self.typing_ticket.lock().await;
            if cached.is_some() {
                return cached.clone();
            }
        }

        let resp = self
            .http_client
            .post(self.api_url("getconfig"))
            .headers(self.api_headers())
            .json(&json!({}))
            .send()
            .await
            .ok()?;

        let data: Value = resp.json().await.ok()?;
        let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
        if errcode != 0 {
            tracing::debug!(errcode, "getconfig returned error, typing unavailable");
            return None;
        }

        let ticket = data.get("typing_ticket").and_then(|v| v.as_str())?;
        let ticket_str = ticket.to_string();
        *self.typing_ticket.lock().await = Some(ticket_str.clone());
        Some(ticket_str)
    }

    /// Call sendtyping API with the cached typing_ticket.
    async fn do_send_typing(&self, chat_id: &str) -> anyhow::Result<()> {
        let ticket = match self.fetch_typing_ticket().await {
            Some(t) => t,
            None => return Ok(()), // Silently skip if ticket unavailable.
        };

        let body = json!({
            "to_user_id": chat_id,
            "typing_ticket": ticket,
        });

        let resp = self
            .http_client
            .post(self.api_url("sendtyping"))
            .headers(self.api_headers())
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let data: Value = r.json().await.unwrap_or_default();
                let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
                if errcode != 0 {
                    // Ticket may have expired — clear cache for next attempt.
                    *self.typing_ticket.lock().await = None;
                    tracing::debug!(errcode, "sendtyping failed, cleared ticket cache");
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
        let context_token = self.context_tokens.lock().await.get(chat_id).cloned();

        for attempt in 0..MAX_SEND_RETRIES {
            // Build hermes-agent compatible sendmessage body.
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
                    "client_id": self.client_id,
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
                .headers(self.api_headers())
                .json(&body)
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let data: Value = r.json().await.unwrap_or_default();
                    let errcode = data.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
                    if errcode == 0 {
                        return Ok(SendResult::ok(None));
                    }
                    if errcode == -14 && attempt == 0 {
                        // Session expired — retry without context_token.
                        tracing::warn!("context_token expired, retrying without token");
                        continue;
                    }
                    if attempt < MAX_SEND_RETRIES - 1 {
                        sleep(SEND_RETRY_DELAY * (attempt + 1)).await;
                        continue;
                    }
                    let errmsg = data
                        .get("errmsg")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    return Ok(SendResult::err(format!("send failed: {errcode} {errmsg}")));
                }
                Err(e) => {
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
        // Validate credentials by making a lightweight request.
        tracing::info!(
            account_id = %self.config.account_id,
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
                        tracing::warn!(
                            error = %e,
                            consecutive = consecutive_errors,
                            "poll error"
                        );
                        if consecutive_errors >= 3 {
                            // Back off for 30s after 3 consecutive errors.
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
