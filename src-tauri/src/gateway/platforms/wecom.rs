//! WeCom (Enterprise WeChat) AI Bot platform adapter.
//!
//! Connects via WebSocket to `openws.work.weixin.qq.com`, authenticates with
//! `aibot_subscribe`, maintains a 30s heartbeat, and handles message send/recv.
//!
//! Aligned with WeCom AI Bot WebSocket protocol:
//! - Frame structure: `{ cmd, headers: { req_id }, body: { ... } }`
//! - Commands: `ping`, `aibot_subscribe`, `aibot_msg_callback`, `aibot_callback`,
//!   `aibot_respond_msg`, `aibot_send_msg`
//! - Text extraction: text, mixed, voice, appmsg
//! - Split-aware debounce: 600ms default, 2s for chunks ≥3900 chars

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_stream::stream;
use futures::stream::SplitSink;
use futures::{SinkExt, Stream, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::gateway::config::WecomConfig;
use crate::gateway::traits::{InboundMessage, Platform, PlatformAdapter, SendResult};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = SplitSink<WsStream, Message>;

// ── Constants ────────────────────────────────────────────────────────

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Maximum outbound message length for WeCom markdown.
const MAX_WECOM_MESSAGE_LENGTH: usize = 4000;
/// TTL for message dedup entries (seconds).
const DEDUP_TTL_SECONDS: u64 = 300;
/// Max dedup map size before eviction.
const DEDUP_MAX_SIZE: usize = 1000;
/// Default debounce window for text aggregation.
const DEBOUNCE_DELAY_MS: u64 = 600;
/// Extended debounce for chunks near the split threshold (≥3900 chars).
const DEBOUNCE_SPLIT_DELAY_MS: u64 = 2000;
/// WeCom client-side text split threshold (~4000 chars).
const SPLIT_THRESHOLD: usize = 3900;
/// Reconnect backoff sequence (seconds).
const RECONNECT_BACKOFF: &[u64] = &[2, 5, 10, 30, 60];

// ── Command constants ────────────────────────────────────────────────

const CMD_SUBSCRIBE: &str = "aibot_subscribe";
const CMD_PING: &str = "ping";
const CMD_MSG_CALLBACK: &str = "aibot_msg_callback";
const CMD_LEGACY_CALLBACK: &str = "aibot_callback";
const CMD_EVENT_CALLBACK: &str = "aibot_event_callback";
const CMD_RESPOND_MSG: &str = "aibot_respond_msg";
const CMD_SEND_MSG: &str = "aibot_send_msg";

/// WeCom AI Bot WebSocket adapter.
pub struct WecomAdapter {
    config: WecomConfig,
    /// Stable device_id for this adapter instance (persists across reconnects).
    device_id: String,
    ws_sink: Arc<Mutex<Option<WsSink>>>,
    heartbeat_handle: Option<JoinHandle<()>>,
    /// Channel for forwarding inbound messages from the WS reader task.
    inbound_tx: tokio::sync::mpsc::Sender<anyhow::Result<InboundMessage>>,
    inbound_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<anyhow::Result<InboundMessage>>>>>,
    reader_handle: Option<JoinHandle<()>>,
    /// Last req_id per chat_id — used for aibot_respond_msg in group chats.
    last_req_ids: Arc<Mutex<HashMap<String, String>>>,
    /// Message ID dedup map with TTL: msg_id → insert time.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
}

impl WecomAdapter {
    pub fn new(config: WecomConfig) -> Self {
        let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(256);
        Self {
            config,
            device_id: uuid::Uuid::new_v4().to_string().replace('-', ""),
            ws_sink: Arc::new(Mutex::new(None)),
            heartbeat_handle: None,
            inbound_tx,
            inbound_rx: Arc::new(Mutex::new(Some(inbound_rx))),
            reader_handle: None,
            last_req_ids: Arc::new(Mutex::new(HashMap::new())),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────

    /// Generate a new req_id: `{prefix}-{uuid4_hex}`.
    fn new_req_id(prefix: &str) -> String {
        format!(
            "{}-{}",
            prefix,
            uuid::Uuid::new_v4().to_string().replace('-', "")
        )
    }

    /// Build a WeCom protocol frame: `{ cmd, headers: { req_id }, body }`.
    fn build_frame(cmd: &str, req_id: &str, body: Value) -> Value {
        json!({
            "cmd": cmd,
            "headers": { "req_id": req_id },
            "body": body,
        })
    }

    /// Establish WebSocket connection.
    async fn connect_ws(&self) -> anyhow::Result<WsStream> {
        let url = format!("wss://{}/", self.config.ws_url);
        tracing::info!(url = %url, "connecting to WeCom WebSocket");

        let (ws_stream, _response) = timeout(CONNECT_TIMEOUT, connect_async(&url))
            .await
            .map_err(|_| anyhow::anyhow!("WebSocket connection timed out"))??;

        tracing::info!("WebSocket connected");
        Ok(ws_stream)
    }

    /// Build the subscribe payload.
    fn subscribe_payload(&self) -> (String, Value) {
        let req_id = Self::new_req_id("subscribe");
        let frame = Self::build_frame(
            CMD_SUBSCRIBE,
            &req_id,
            json!({
                "bot_id": self.config.bot_id,
                "secret": self.config.secret,
                "device_id": self.device_id,
            }),
        );
        (req_id, frame)
    }

    /// Build a heartbeat ping payload.
    fn ping_payload() -> Value {
        Self::build_frame(CMD_PING, &Self::new_req_id("ping"), json!({}))
    }

    /// Start the heartbeat task.
    fn start_heartbeat(&self, sink: Arc<Mutex<Option<WsSink>>>) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                sleep(HEARTBEAT_INTERVAL).await;
                let payload = Self::ping_payload().to_string();
                let mut guard = sink.lock().await;
                if let Some(ref mut ws) = *guard {
                    if ws.send(Message::Text(payload.into())).await.is_err() {
                        tracing::warn!("heartbeat send failed, connection may be lost");
                        break;
                    }
                } else {
                    break;
                }
            }
        })
    }

    // ── Message extraction ───────────────────────────────────────────

    /// Extract text content from a WeCom message body.
    ///
    /// Supports: `text`, `mixed` (multi-item), `voice` (transcription), `appmsg` (file title).
    /// Returns `(main_text, reply_text)`.
    fn extract_text(body: &Value) -> (String, Option<String>) {
        let msgtype = body.get("msgtype").and_then(|v| v.as_str()).unwrap_or("");

        let mut text_parts: Vec<String> = Vec::new();

        match msgtype {
            "mixed" => {
                // Iterate mixed.msg_item[] and extract text items.
                if let Some(items) = body
                    .get("mixed")
                    .and_then(|m| m.get("msg_item"))
                    .and_then(|v| v.as_array())
                {
                    for item in items {
                        let item_type = item.get("msgtype").and_then(|v| v.as_str()).unwrap_or("");
                        if item_type == "text" {
                            if let Some(content) = item
                                .get("text")
                                .and_then(|t| t.get("content"))
                                .and_then(|c| c.as_str())
                            {
                                let trimmed = content.trim();
                                if !trimmed.is_empty() {
                                    text_parts.push(trimmed.to_string());
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // Standard text.content
                if let Some(content) = body
                    .get("text")
                    .and_then(|t| t.get("content"))
                    .and_then(|c| c.as_str())
                {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        text_parts.push(trimmed.to_string());
                    }
                }

                // Voice transcription: voice.content
                if msgtype == "voice" {
                    if let Some(voice_text) = body
                        .get("voice")
                        .and_then(|v| v.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        let trimmed = voice_text.trim();
                        if !trimmed.is_empty() {
                            text_parts.push(trimmed.to_string());
                        }
                    }
                }

                // Appmsg title (file attachment).
                if msgtype == "appmsg" {
                    if let Some(title) = body
                        .get("appmsg")
                        .and_then(|a| a.get("title"))
                        .and_then(|t| t.as_str())
                    {
                        let trimmed = title.trim();
                        if !trimmed.is_empty() {
                            text_parts.push(trimmed.to_string());
                        }
                    }
                }
            }
        }

        // Quote / reply context.
        let reply_text = body
            .get("quote")
            .and_then(|quote| {
                let quote_type = quote.get("msgtype").and_then(|v| v.as_str()).unwrap_or("");
                match quote_type {
                    "text" => quote
                        .get("text")
                        .and_then(|t| t.get("content"))
                        .and_then(|c| c.as_str()),
                    "voice" => quote
                        .get("voice")
                        .and_then(|v| v.get("content"))
                        .and_then(|c| c.as_str()),
                    _ => None,
                }
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let main_text = text_parts.join("\n");
        (main_text, reply_text)
    }

    // ── Inbound frame dispatch ───────────────────────────────────────

    /// Parse an inbound WeCom WebSocket frame into an InboundMessage.
    ///
    /// Handles `aibot_msg_callback` and `aibot_callback` commands.
    /// Frame structure: `{ cmd, headers: { req_id }, body: { ... } }`.
    fn parse_message(frame: &str) -> Option<InboundMessage> {
        let v: Value = serde_json::from_str(frame).ok()?;
        let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("");

        // Only handle message callback commands.
        if cmd != CMD_MSG_CALLBACK && cmd != CMD_LEGACY_CALLBACK {
            // Silently ignore ping, event_callback, subscribe responses, etc.
            if cmd != CMD_PING && cmd != CMD_EVENT_CALLBACK && cmd != CMD_SUBSCRIBE {
                tracing::debug!(cmd, "ignoring non-callback WeCom frame");
            }
            return None;
        }

        let body = v.get("body")?;
        let headers = v.get("headers").cloned().unwrap_or(json!({}));

        // Extract req_id from headers (for group reply routing).
        let req_id = headers
            .get("req_id")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());

        // Extract sender info: body.from.userid
        let sender_id = body
            .get("from")
            .and_then(|f| f.get("userid"))
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();

        // Message ID: body.msgid, fallback to headers.req_id or generated UUID.
        let message_id = body
            .get("msgid")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .or_else(|| req_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Chat ID: body.chatid, fallback to sender_id.
        let chat_id = body
            .get("chatid")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&sender_id)
            .to_string();

        // Group chat: body.chattype == "group"
        let is_group = body
            .get("chattype")
            .and_then(|c| c.as_str())
            .map(|t| t == "group")
            .unwrap_or(false);

        // Extract text using the comprehensive extractor.
        let (text, _reply_text) = Self::extract_text(body);

        if text.is_empty() {
            return None;
        }

        Some(InboundMessage {
            message_id,
            sender_id,
            chat_id,
            text,
            is_group,
            media_urls: vec![],
            media_attachments: vec![],
            req_id,
        })
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for WecomAdapter {
    async fn connect(&mut self) -> anyhow::Result<()> {
        let ws_stream = self.connect_ws().await?;
        let (mut sink, mut stream) = ws_stream.split();

        // Send subscribe command with req_id for handshake matching.
        let (subscribe_req_id, subscribe_frame) = self.subscribe_payload();
        sink.send(Message::Text(subscribe_frame.to_string().into()))
            .await
            .map_err(|e| anyhow::anyhow!("failed to send subscribe: {e}"))?;

        // Wait for subscribe response, matching by req_id.
        let handshake_deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;

        loop {
            let remaining =
                handshake_deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                // Timeout — log warning but proceed (old servers may not respond).
                tracing::warn!(
                    "subscribe handshake timed out after {:?}, proceeding anyway",
                    CONNECT_TIMEOUT
                );
                break;
            }

            match timeout(remaining, stream.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    let v: Value = serde_json::from_str(&text).unwrap_or_default();
                    let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
                    let frame_req_id = v
                        .get("headers")
                        .and_then(|h| h.get("req_id"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("");

                    // Skip ping frames during handshake.
                    if cmd == CMD_PING {
                        continue;
                    }

                    // Match subscribe response by req_id.
                    if frame_req_id == subscribe_req_id {
                        let errcode = v
                            .get("body")
                            .and_then(|b| b.get("errcode"))
                            .and_then(|e| e.as_i64());

                        match errcode {
                            Some(0) | None => {
                                tracing::info!("subscribe handshake succeeded");
                            }
                            Some(code) => {
                                let errmsg = v
                                    .get("body")
                                    .and_then(|b| b.get("errmsg"))
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("unknown");
                                anyhow::bail!(
                                    "subscribe authentication failed: errcode={code} errmsg={errmsg}"
                                );
                            }
                        }
                        break;
                    }

                    tracing::debug!(cmd, "ignoring pre-auth payload");
                }
                Ok(Some(Ok(_))) => {
                    // Non-text frame (ping/pong) — continue waiting.
                    continue;
                }
                Ok(Some(Err(e))) => {
                    anyhow::bail!("WebSocket error during subscribe handshake: {e}");
                }
                Ok(None) => {
                    anyhow::bail!("WebSocket closed before subscribe response");
                }
                Err(_) => {
                    tracing::warn!(
                        "subscribe handshake timed out after {:?}, proceeding anyway",
                        CONNECT_TIMEOUT
                    );
                    break;
                }
            }
        }

        // Store sink.
        *self.ws_sink.lock().await = Some(sink);

        // Start heartbeat.
        self.heartbeat_handle = Some(self.start_heartbeat(Arc::clone(&self.ws_sink)));

        // Start reader task with split-aware debounce for text aggregation.
        let inbound_tx = self.inbound_tx.clone();
        let last_req_ids = Arc::clone(&self.last_req_ids);
        let seen_messages = Arc::clone(&self.seen_messages);
        self.reader_handle = Some(tokio::spawn(async move {
            let mut stream = stream;
            // Debounce buffer: accumulate messages from the same sender within a window.
            let mut pending: Option<InboundMessage> = None;
            let mut last_chunk_len: usize = 0;

            loop {
                // Compute debounce delay based on last chunk length.
                let debounce_duration = if last_chunk_len >= SPLIT_THRESHOLD {
                    tokio::time::Duration::from_millis(DEBOUNCE_SPLIT_DELAY_MS)
                } else {
                    tokio::time::Duration::from_millis(DEBOUNCE_DELAY_MS)
                };

                let frame = if pending.is_some() {
                    // We have a pending message — wait for more within debounce window.
                    match tokio::time::timeout(debounce_duration, stream.next()).await {
                        Ok(Some(frame)) => Some(frame),
                        Ok(None) => {
                            // Stream ended — flush pending.
                            if let Some(msg) = pending.take() {
                                let _ = inbound_tx.send(Ok(msg)).await;
                            }
                            break;
                        }
                        Err(_) => {
                            // Timeout — flush pending message.
                            if let Some(msg) = pending.take() {
                                // Store req_id for reply routing.
                                if let Some(ref rid) = msg.req_id {
                                    last_req_ids
                                        .lock()
                                        .await
                                        .insert(msg.chat_id.clone(), rid.clone());
                                }
                                if inbound_tx.send(Ok(msg)).await.is_err() {
                                    break;
                                }
                            }
                            last_chunk_len = 0;
                            continue;
                        }
                    }
                } else {
                    // No pending — wait indefinitely for next frame.
                    stream.next().await.map(Some).unwrap_or(None)
                };

                match frame {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(msg) = WecomAdapter::parse_message(&text) {
                            // Dedup by msg_id with TTL.
                            if !msg.message_id.is_empty() {
                                let now = Instant::now();
                                let mut seen = seen_messages.lock().await;
                                if let Some(t) = seen.get(&msg.message_id) {
                                    if now.duration_since(*t).as_secs() < DEDUP_TTL_SECONDS {
                                        continue; // Duplicate, skip.
                                    }
                                }
                                seen.insert(msg.message_id.clone(), now);
                                // Evict expired entries when exceeding max size.
                                if seen.len() > DEDUP_MAX_SIZE {
                                    seen.retain(|_, t| {
                                        now.duration_since(*t).as_secs() < DEDUP_TTL_SECONDS
                                    });
                                }
                            }

                            // Track chunk length for split-aware debounce.
                            last_chunk_len = msg.text.chars().count();

                            // Check if we should merge with pending.
                            if let Some(ref mut p) = pending {
                                if p.sender_id == msg.sender_id && p.chat_id == msg.chat_id {
                                    // Same sender within debounce window — merge text.
                                    p.text.push('\n');
                                    p.text.push_str(&msg.text);
                                    // Update req_id to latest.
                                    if msg.req_id.is_some() {
                                        p.req_id = msg.req_id;
                                    }
                                    continue;
                                } else {
                                    // Different sender — flush pending, start new.
                                    let flushed = pending.take().unwrap();
                                    if let Some(ref rid) = flushed.req_id {
                                        last_req_ids
                                            .lock()
                                            .await
                                            .insert(flushed.chat_id.clone(), rid.clone());
                                    }
                                    if inbound_tx.send(Ok(flushed)).await.is_err() {
                                        break;
                                    }
                                    pending = Some(msg);
                                    continue;
                                }
                            } else {
                                pending = Some(msg);
                                continue;
                            }
                        }
                        continue; // Non-message frame, skip.
                    }
                    Some(Ok(Message::Close(_))) => {
                        if let Some(msg) = pending.take() {
                            let _ = inbound_tx.send(Ok(msg)).await;
                        }
                        tracing::info!("WeCom WebSocket closed by server");
                        break;
                    }
                    Some(Err(e)) => {
                        if let Some(msg) = pending.take() {
                            let _ = inbound_tx.send(Ok(msg)).await;
                        }
                        tracing::warn!(error = %e, "WeCom WebSocket error");
                        let _ = inbound_tx
                            .send(Err(anyhow::anyhow!("WebSocket error: {e}")))
                            .await;
                        break;
                    }
                    None => {
                        // Stream ended.
                        if let Some(msg) = pending.take() {
                            let _ = inbound_tx.send(Ok(msg)).await;
                        }
                        break;
                    }
                    _ => continue, // Ping/Pong/Binary — ignore.
                }
            }
        }));

        tracing::info!("WeCom adapter connected and listening");
        Ok(())
    }

    async fn disconnect(&mut self) {
        if let Some(handle) = self.heartbeat_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }
        let mut guard = self.ws_sink.lock().await;
        if let Some(ref mut sink) = *guard {
            let _ = sink.close().await;
        }
        *guard = None;
        tracing::info!("WeCom adapter disconnected");
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> anyhow::Result<SendResult> {
        // Check if we have a req_id for this chat (group reply routing).
        let req_id = self.last_req_ids.lock().await.get(chat_id).cloned();

        // Truncate to WeCom max markdown length.
        let content = if text.chars().count() > MAX_WECOM_MESSAGE_LENGTH {
            let truncated: String = text.chars().take(MAX_WECOM_MESSAGE_LENGTH - 20).collect();
            format!("{truncated}\n\n…(已截断)")
        } else {
            text.to_string()
        };

        let payload = if let Some(ref rid) = req_id {
            // Group chat: must use aibot_respond_msg with original req_id.
            Self::build_frame(
                CMD_RESPOND_MSG,
                rid,
                json!({
                    "msgtype": "markdown",
                    "markdown": { "content": content },
                }),
            )
        } else {
            // DM: use aibot_send_msg with new req_id.
            Self::build_frame(
                CMD_SEND_MSG,
                &Self::new_req_id(CMD_SEND_MSG),
                json!({
                    "chatid": chat_id,
                    "msgtype": "markdown",
                    "markdown": { "content": content },
                }),
            )
        };

        let mut guard = self.ws_sink.lock().await;
        if let Some(ref mut sink) = *guard {
            sink.send(Message::Text(payload.to_string().into()))
                .await
                .map_err(|e| anyhow::anyhow!("send failed: {e}"))?;
            Ok(SendResult::ok(None))
        } else {
            Ok(SendResult::err("not connected"))
        }
    }

    async fn send_typing(&self, _chat_id: &str) -> anyhow::Result<()> {
        // WeCom AI Bot doesn't have a typing indicator API.
        Ok(())
    }

    async fn stop_typing(&self, _chat_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn poll_messages(
        &self,
    ) -> Pin<Box<dyn Stream<Item = anyhow::Result<InboundMessage>> + Send + '_>> {
        let inbound_rx = Arc::clone(&self.inbound_rx);
        Box::pin(stream! {
            let mut rx = match inbound_rx.lock().await.take() {
                Some(rx) => rx,
                None => {
                    yield Err(anyhow::anyhow!("poll_messages already consumed (can only be called once per adapter instance)"));
                    return;
                }
            };
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        })
    }

    fn platform(&self) -> Platform {
        Platform::Wecom
    }
}

/// Reconnect delay with fixed backoff sequence:
/// `[2, 5, 10, 30, 60]` seconds.
pub async fn reconnect_delay(attempt: u32) -> Duration {
    let idx = (attempt as usize).min(RECONNECT_BACKOFF.len() - 1);
    let delay = Duration::from_secs(RECONNECT_BACKOFF[idx]);
    sleep(delay).await;
    delay
}
