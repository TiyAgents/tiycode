//! WeCom (Enterprise WeChat) AI Bot platform adapter.
//!
//! Connects via WebSocket to `openws.work.weixin.qq.com`, authenticates with
//! `aibot_subscribe`, maintains dual-layer heartbeat (protocol-level Ping +
//! application-level cmd:ping), and handles message send/recv.
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
use futures::{SinkExt, Stream, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};

use crate::gateway::config::WecomConfig;
use crate::gateway::traits::{InboundMessage, Platform, PlatformAdapter, SendResult};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ── Constants ────────────────────────────────────────────────────────

/// Application-level heartbeat interval (sends `cmd: "ping"` JSON frame).
/// Kept well under the server's idle timeout (~30s) so the first ping
/// arrives before the server can drop the connection.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// Maximum time to wait for inbound activity before considering the connection
/// dead. As long as the server answers our JSON `cmd:"ping"` (or sends anything)
/// within this window, the connection is treated as alive.
const PONG_TIMEOUT: Duration = Duration::from_secs(60);
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
/// Timeout for awaiting a server ack to an outbound send frame.
const SEND_ACK_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Sender for outbound frames. All writes to the WebSocket go through this
    /// channel and are serialized by the single I/O task that owns the stream;
    /// no other task ever touches the TLS stream directly. Set on `connect`.
    outbound_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<Message>>>>,
    heartbeat_handle: Option<JoinHandle<()>>,
    /// Channel for forwarding inbound messages from the WS reader task.
    inbound_tx: tokio::sync::mpsc::Sender<anyhow::Result<InboundMessage>>,
    inbound_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<anyhow::Result<InboundMessage>>>>>,
    reader_handle: Option<JoinHandle<()>>,
    /// Last req_id per chat_id — used for aibot_respond_msg in group chats.
    last_req_ids: Arc<Mutex<HashMap<String, String>>>,
    /// Message ID dedup map with TTL: msg_id → insert time.
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    /// Pending outbound send acks keyed by req_id → oneshot sender for the
    /// server's response frame. The reader task resolves these when a matching
    /// ack frame arrives.
    pending_acks: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    /// Timestamp of the last WebSocket Pong frame received from the server.
    /// Used by the heartbeat task to detect unresponsive connections.
    last_pong_received: Arc<Mutex<Instant>>,
    /// Notifies the reader task that the heartbeat detected a dead connection.
    /// When the heartbeat fails to send, it signals here so the reader can
    /// break out and trigger a runner-level reconnect.
    connection_lost_tx: tokio::sync::watch::Sender<bool>,
    connection_lost_rx: Arc<tokio::sync::watch::Receiver<bool>>,
}

impl WecomAdapter {
    pub fn new(config: WecomConfig) -> Self {
        let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(256);
        let (connection_lost_tx, connection_lost_rx) = tokio::sync::watch::channel(false);
        Self {
            config,
            device_id: uuid::Uuid::new_v4().to_string().replace('-', ""),
            outbound_tx: Arc::new(Mutex::new(None)),
            heartbeat_handle: None,
            inbound_tx,
            inbound_rx: Arc::new(Mutex::new(Some(inbound_rx))),
            reader_handle: None,
            last_req_ids: Arc::new(Mutex::new(HashMap::new())),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            pending_acks: Arc::new(Mutex::new(HashMap::new())),
            last_pong_received: Arc::new(Mutex::new(Instant::now())),
            connection_lost_tx,
            connection_lost_rx: Arc::new(connection_lost_rx),
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

        // Standard tokio-tungstenite, no protocol relaxations. WeCom's AI bot
        // long-connection protocol is standard RFC 6455 JSON-over-WebSocket
        // (verified against the official Python/Node SDKs and Rust ports), so
        // no frame-level tolerance is needed.
        let (ws_stream, _response) = timeout(
            CONNECT_TIMEOUT,
            connect_async_with_config(&url, None, false),
        )
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
    ///
    /// Aligned with the official WeCom AI bot protocol, which specifies an
    /// **application-level** heartbeat: the client periodically sends a
    /// `{"cmd":"ping"}` JSON frame and the server replies with a `pong` JSON
    /// frame (recommended interval ~30s). We do NOT use WebSocket protocol-level
    /// Ping/Pong frames — the server does not reliably answer those, and relying
    /// on them previously caused false "connection dead" detections.
    ///
    /// Liveness is tracked via `last_activity` (renamed semantics): the I/O task
    /// updates it on ANY inbound frame, so as long as the server keeps answering
    /// our JSON pings (or sends anything), the connection is considered alive.
    /// When the heartbeat fails to enqueue, or no inbound activity is seen within
    /// `PONG_TIMEOUT`, the connection is considered dead and a reconnect is
    /// signalled via `connection_lost_tx`.
    fn start_heartbeat(
        &self,
        outbound_tx: tokio::sync::mpsc::Sender<Message>,
        connection_lost_tx: tokio::sync::watch::Sender<bool>,
        last_pong_received: Arc<Mutex<Instant>>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut app_tick = tokio::time::interval(HEARTBEAT_INTERVAL);
            // Consume the first immediate tick.
            app_tick.tick().await;

            loop {
                app_tick.tick().await;

                // Detect a dead connection: no inbound activity within timeout.
                {
                    let last = *last_pong_received.lock().await;
                    if last.elapsed() > PONG_TIMEOUT {
                        tracing::warn!(
                            elapsed_secs = last.elapsed().as_secs(),
                            "no inbound activity within timeout, connection is dead"
                        );
                        let _ = connection_lost_tx.send(true);
                        break;
                    }
                }

                // Application-level ping: send cmd:"ping" JSON frame.
                let payload = Self::ping_payload().to_string();
                if outbound_tx
                    .send(Message::Text(payload.into()))
                    .await
                    .is_err()
                {
                    tracing::warn!("app-level heartbeat send failed, connection may be lost");
                    let _ = connection_lost_tx.send(true);
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

    /// Try to resolve a pending outbound-send ack from an inbound text frame.
    ///
    /// WeCom replies to `aibot_send_msg` / `aibot_respond_msg` with a frame that
    /// echoes the request's `req_id` in `headers.req_id`. If that req_id matches
    /// a waiting sender in `pending_acks`, the full frame is delivered to it and
    /// `true` is returned so the reader skips normal message parsing.
    async fn try_resolve_ack(
        text: &str,
        pending_acks: &Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    ) -> bool {
        let v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return false,
        };

        // Callback / push commands are inbound user messages or server pushes,
        // never outbound-send acks. Exclude all known non-ack commands so a
        // genuine inbound frame is never mistaken for an ack even on the
        // (astronomically unlikely) event of a req_id collision.
        let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
        if matches!(
            cmd,
            CMD_MSG_CALLBACK | CMD_LEGACY_CALLBACK | CMD_EVENT_CALLBACK | CMD_SUBSCRIBE | CMD_PING
        ) {
            return false;
        }

        let req_id = match v
            .get("headers")
            .and_then(|h| h.get("req_id"))
            .and_then(|r| r.as_str())
        {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => return false,
        };

        let sender = pending_acks.lock().await.remove(&req_id);
        match sender {
            Some(tx) => {
                let _ = tx.send(v);
                true
            }
            None => false,
        }
    }

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
        let mut ws_stream = self.connect_ws().await?;

        // Send subscribe command with req_id for handshake matching. During the
        // handshake we own `ws_stream` directly and drive read+write inline on a
        // single task, so there is no cross-task access to the TLS stream.
        let (subscribe_req_id, subscribe_frame) = self.subscribe_payload();
        ws_stream
            .send(Message::Text(subscribe_frame.to_string().into()))
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

            match timeout(remaining, ws_stream.next()).await {
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

        // Create the outbound channel. Every writer (heartbeat, send_text, and
        // the I/O task's own Pong replies) pushes frames here; only the single
        // I/O task below ever writes to the TLS stream, eliminating the
        // cross-task read/write contention that previously stalled reads.
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel::<Message>(64);
        *self.outbound_tx.lock().await = Some(outbound_tx.clone());

        // Reset Pong liveness tracker BEFORE starting heartbeat so the first
        // Pong-timeout check doesn't fire against a stale timestamp.
        *self.last_pong_received.lock().await = Instant::now();

        // Start heartbeat (writes only via the outbound channel).
        self.heartbeat_handle = Some(self.start_heartbeat(
            outbound_tx.clone(),
            self.connection_lost_tx.clone(),
            Arc::clone(&self.last_pong_received),
        ));

        // Start the single I/O task: it solely owns `ws_stream` and multiplexes
        // inbound reads, outbound writes, and connection-loss signals with one
        // `select!`. No other task touches the stream.
        let inbound_tx = self.inbound_tx.clone();
        let last_req_ids = Arc::clone(&self.last_req_ids);
        let seen_messages = Arc::clone(&self.seen_messages);
        let pending_acks = Arc::clone(&self.pending_acks);
        let last_pong_received = Arc::clone(&self.last_pong_received);
        let pong_tx = outbound_tx.clone();
        let mut connection_lost_rx = (*self.connection_lost_rx).clone();
        self.reader_handle = Some(tokio::spawn(async move {
            let mut ws_stream = ws_stream;
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
                    // We have a pending message — wait for more within debounce window,
                    // service outbound writes, and watch for connection loss.
                    tokio::select! {
                        biased;
                        out = outbound_rx.recv() => {
                            match out {
                                Some(msg) => {
                                    if let Err(e) = ws_stream.send(msg).await {
                                        tracing::warn!(error = %e, "outbound send failed");
                                    }
                                    continue;
                                }
                                None => break,
                            }
                        }
                        frame_result = tokio::time::timeout(debounce_duration, ws_stream.next()) => {
                            match frame_result {
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
                        }
                        _ = connection_lost_rx.changed() => {
                            tracing::warn!("heartbeat detected connection loss, stopping reader");
                            if let Some(msg) = pending.take() {
                                let _ = inbound_tx.send(Ok(msg)).await;
                            }
                            let _ = inbound_tx
                                .send(Err(anyhow::anyhow!("connection lost (heartbeat failure)")))
                                .await;
                            break;
                        }
                    }
                } else {
                    // No pending — service outbound writes, read next frame, and
                    // watch for connection loss.
                    tokio::select! {
                        biased;
                        out = outbound_rx.recv() => {
                            match out {
                                Some(msg) => {
                                    if let Err(e) = ws_stream.send(msg).await {
                                        tracing::warn!(error = %e, "outbound send failed");
                                    }
                                    continue;
                                }
                                None => break,
                            }
                        }
                        frame_opt = ws_stream.next() => frame_opt.map(Some).unwrap_or(None),
                        _ = connection_lost_rx.changed() => {
                            tracing::warn!("heartbeat detected connection loss, stopping reader");
                            let _ = inbound_tx
                                .send(Err(anyhow::anyhow!("connection lost (heartbeat failure)")))
                                .await;
                            break;
                        }
                    }
                };

                match frame {
                    Some(Ok(Message::Text(text))) => {
                        // Any inbound frame counts as liveness activity.
                        *last_pong_received.lock().await = Instant::now();
                        let text = text.to_string();
                        // First, try to resolve a pending outbound send ack.
                        // Send-response frames echo back the request's req_id in
                        // headers; route them to the waiting sender if matched.
                        if WecomAdapter::try_resolve_ack(&text, &pending_acks).await {
                            continue;
                        }
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
                    Some(Ok(other)) => {
                        // Ping/Pong/Binary/Frame. tokio-tungstenite automatically
                        // answers protocol-level Ping with Pong internally, so we
                        // only need to record liveness here. `pong_tx` is retained
                        // for parity but unused in normal flow.
                        let _ = &pong_tx;
                        let _ = &other;
                        *last_pong_received.lock().await = Instant::now();
                        continue;
                    }
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
        // Dropping the outbound sender lets the I/O task observe channel close.
        *self.outbound_tx.lock().await = None;
        // Drop any pending send-ack waiters so callers stop blocking.
        self.pending_acks.lock().await.clear();
        // Reset connection-lost flag so a fresh connect() starts with a clean state.
        let _ = self.connection_lost_tx.send(false);
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

        let (frame_req_id, payload) = if let Some(ref rid) = req_id {
            // Group chat: must use aibot_respond_msg with original req_id.
            (
                rid.clone(),
                Self::build_frame(
                    CMD_RESPOND_MSG,
                    rid,
                    json!({
                        "msgtype": "markdown",
                        "markdown": { "content": content },
                    }),
                ),
            )
        } else {
            // DM: use aibot_send_msg with new req_id.
            let rid = Self::new_req_id(CMD_SEND_MSG);
            (
                rid.clone(),
                Self::build_frame(
                    CMD_SEND_MSG,
                    &rid,
                    json!({
                        "chatid": chat_id,
                        "msgtype": "markdown",
                        "markdown": { "content": content },
                    }),
                ),
            )
        };

        // Register a pending ack waiter before sending so the reader task can
        // resolve it when the server's response frame arrives.
        //
        // The ack is correlated by frame req_id. For DM (`aibot_send_msg`) the
        // req_id is freshly generated and unique. For group chats
        // (`aibot_respond_msg`) the WeCom protocol requires reusing the inbound
        // req_id, so two concurrent sends to the same chat would collide on the
        // same key. Callers send sequentially (one awaited chunk at a time), but
        // to stay robust we detect an existing waiter and fail the older one
        // explicitly instead of silently dropping its sender.
        let (ack_tx, ack_rx) = oneshot::channel::<Value>();
        if let Some(stale) = self
            .pending_acks
            .lock()
            .await
            .insert(frame_req_id.clone(), ack_tx)
        {
            tracing::warn!(
                chat_id,
                req_id = %frame_req_id,
                "overlapping WeCom send reused the same req_id; previous ack waiter dropped"
            );
            drop(stale);
        }

        // Send the frame via the outbound channel. The single I/O task owns the
        // stream and serializes the actual write, so send_text never touches the
        // TLS stream directly (which previously stalled the reader).
        let send_error: Option<anyhow::Result<SendResult>> = {
            let guard = self.outbound_tx.lock().await;
            match guard.as_ref() {
                Some(tx) => match tx.send(Message::Text(payload.to_string().into())).await {
                    Ok(()) => None,
                    Err(e) => Some(Err(anyhow::anyhow!("send failed: {e}"))),
                },
                None => Some(Ok(SendResult::err("not connected"))),
            }
        };
        if let Some(result) = send_error {
            self.pending_acks.lock().await.remove(&frame_req_id);
            return result;
        }

        // Await the server ack with a timeout, then validate errcode.
        match timeout(SEND_ACK_TIMEOUT, ack_rx).await {
            Ok(Ok(resp)) => {
                let errcode = resp
                    .get("body")
                    .and_then(|b| b.get("errcode"))
                    .and_then(|e| e.as_i64())
                    .or_else(|| resp.get("errcode").and_then(|e| e.as_i64()))
                    .unwrap_or(0);
                if errcode == 0 {
                    Ok(SendResult::ok(Some(frame_req_id)))
                } else {
                    let errmsg = resp
                        .get("body")
                        .and_then(|b| b.get("errmsg"))
                        .and_then(|m| m.as_str())
                        .or_else(|| resp.get("errmsg").and_then(|m| m.as_str()))
                        .unwrap_or("unknown");
                    tracing::warn!(chat_id, errcode, errmsg, "WeCom send rejected by server");
                    Ok(SendResult::err(format!(
                        "send rejected: errcode={errcode} errmsg={errmsg}"
                    )))
                }
            }
            Ok(Err(_)) => {
                // Sender dropped (reader task ended) — treat as transport loss.
                Ok(SendResult::err("ack channel closed (reader stopped)"))
            }
            Err(_) => {
                // Timed out — clean up the waiter and report failure.
                self.pending_acks.lock().await.remove(&frame_req_id);
                tracing::warn!(chat_id, req_id = %frame_req_id, "WeCom send ack timed out");
                Ok(SendResult::err("send ack timed out"))
            }
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
