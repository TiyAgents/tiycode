//! WeCom (Enterprise WeChat) AI Bot platform adapter.
//!
//! Connects via WebSocket to `openws.work.weixin.qq.com`, authenticates with
//! `aibot_subscribe`, maintains a 30s heartbeat, and handles message send/recv.
//! Reference: hermes-agent/gateway/platforms/wecom.py

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

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);
/// TTL for message dedup entries (seconds).
const DEDUP_TTL_SECONDS: u64 = 300;
/// Subscribe response wait timeout.
const SUBSCRIBE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum outbound message length for WeCom markdown.
const MAX_WECOM_MESSAGE_LENGTH: usize = 4000;

/// WeCom AI Bot WebSocket adapter.
pub struct WecomAdapter {
    config: WecomConfig,
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
            ws_sink: Arc::new(Mutex::new(None)),
            heartbeat_handle: None,
            inbound_tx,
            inbound_rx: Arc::new(Mutex::new(Some(inbound_rx))),
            reader_handle: None,
            last_req_ids: Arc::new(Mutex::new(HashMap::new())),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Establish WebSocket connection and authenticate.
    async fn connect_ws(&self) -> anyhow::Result<WsStream> {
        let url = format!("wss://{}/", self.config.ws_url);
        tracing::info!(url = %url, "connecting to WeCom WebSocket");

        let (ws_stream, _response) = timeout(CONNECT_TIMEOUT, connect_async(&url))
            .await
            .map_err(|_| anyhow::anyhow!("WebSocket connection timed out"))??;

        tracing::info!("WebSocket connected, sending aibot_subscribe");
        Ok(ws_stream)
    }

    /// Send the `aibot_subscribe` authentication command (reserved for future use).
    #[allow(dead_code)]
    async fn authenticate(sink: &Arc<Mutex<Option<WsSink>>>) -> anyhow::Result<()> {
        // Authentication is sent in connect() after split.
        // This is a placeholder — actual auth message is sent in connect().
        let _ = sink;
        Ok(())
    }

    /// Build the subscribe payload.
    fn subscribe_payload(&self) -> Value {
        json!({
            "command": "aibot_subscribe",
            "data": {
                "bot_id": self.config.bot_id,
                "secret": self.config.secret,
                "device_id": uuid::Uuid::new_v4().to_string(),
            }
        })
    }

    /// Build a heartbeat ping payload.
    fn ping_payload() -> Value {
        json!({
            "command": "aibot_ping",
            "data": {}
        })
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

    /// Parse an inbound WeCom WebSocket frame into an InboundMessage.
    fn parse_message(frame: &str) -> Option<InboundMessage> {
        let v: Value = serde_json::from_str(frame).ok()?;
        let command = v.get("command")?.as_str()?;

        // Only handle aibot_recvmsg (inbound user messages).
        if command != "aibot_recvmsg" {
            return None;
        }

        let data = v.get("data")?;
        let msg = data.get("msg")?;
        let sender_id = msg
            .get("from")
            .and_then(|v| v.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let text = msg
            .get("content")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let message_id = msg
            .get("msg_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let chat_id = msg
            .get("chat_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&sender_id)
            .to_string();
        let is_group = msg
            .get("chat_type")
            .and_then(|v| v.as_str())
            .map(|t| t == "group")
            .unwrap_or(false);
        // req_id is needed for group chat replies via aibot_respond_msg.
        let req_id = data
            .get("req_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

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
            req_id,
        })
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for WecomAdapter {
    async fn connect(&mut self) -> anyhow::Result<()> {
        let ws_stream = self.connect_ws().await?;
        let (mut sink, mut stream) = ws_stream.split();

        // Send subscribe command.
        let subscribe_msg = self.subscribe_payload().to_string();
        sink.send(Message::Text(subscribe_msg.into()))
            .await
            .map_err(|e| anyhow::anyhow!("failed to send subscribe: {e}"))?;

        // Wait for subscribe response to verify authentication.
        let subscribe_result = timeout(SUBSCRIBE_RESPONSE_TIMEOUT, stream.next()).await;
        match subscribe_result {
            Ok(Some(Ok(Message::Text(text)))) => {
                let v: Value = serde_json::from_str(&text).unwrap_or_default();
                let command = v.get("command").and_then(|c| c.as_str()).unwrap_or("");
                let errcode = v
                    .get("data")
                    .and_then(|d| d.get("errcode"))
                    .and_then(|e| e.as_i64())
                    .unwrap_or(-1);

                if command == "aibot_subscribe" && errcode != 0 {
                    let errmsg = v
                        .get("data")
                        .and_then(|d| d.get("errmsg"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown");
                    anyhow::bail!(
                        "subscribe authentication failed: errcode={errcode} errmsg={errmsg}"
                    );
                }
                tracing::info!(
                    command,
                    errcode,
                    "subscribe response received, authenticated"
                );
            }
            Ok(Some(Ok(_))) => {
                // Non-text frame — treat as success (ping/pong).
                tracing::debug!("subscribe: received non-text frame, assuming success");
            }
            Ok(Some(Err(e))) => {
                anyhow::bail!("WebSocket error while waiting for subscribe response: {e}");
            }
            Ok(None) => {
                anyhow::bail!("WebSocket closed before subscribe response");
            }
            Err(_) => {
                // Timeout — log warning but continue (old servers may not respond).
                tracing::warn!(
                    "subscribe response timed out after {:?}, proceeding anyway",
                    SUBSCRIBE_RESPONSE_TIMEOUT
                );
            }
        }

        // Store sink.
        *self.ws_sink.lock().await = Some(sink);

        // Start heartbeat.
        self.heartbeat_handle = Some(self.start_heartbeat(Arc::clone(&self.ws_sink)));

        // Start reader task with debounce for text aggregation.
        let inbound_tx = self.inbound_tx.clone();
        let last_req_ids = Arc::clone(&self.last_req_ids);
        let seen_messages = Arc::clone(&self.seen_messages);
        self.reader_handle = Some(tokio::spawn(async move {
            let mut stream = stream;
            // Debounce buffer: accumulate messages from the same sender within a window.
            let mut pending: Option<InboundMessage> = None;
            let debounce_duration = tokio::time::Duration::from_millis(600);

            loop {
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
                                // Evict expired entries.
                                if seen.len() > 2000 {
                                    seen.retain(|_, t| {
                                        now.duration_since(*t).as_secs() < DEDUP_TTL_SECONDS
                                    });
                                }
                            }

                            // Check if we should merge with pending.
                            if let Some(ref mut p) = pending {
                                if p.sender_id == msg.sender_id && p.chat_id == msg.chat_id {
                                    // Same sender within debounce window — merge text.
                                    p.text.push('\n');
                                    p.text.push_str(&msg.text);
                                    continue;
                                } else {
                                    // Different sender — flush pending, start new.
                                    let flushed = pending.take().unwrap();
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
            // Group chat: must use aibot_respond_msg with req_id.
            json!({
                "command": "aibot_respond_msg",
                "data": {
                    "req_id": rid,
                    "msg": {
                        "content": {
                            "text": content
                        },
                        "msg_type": "markdown"
                    }
                }
            })
        } else {
            // DM: use aibot_send_msg for proactive delivery.
            json!({
                "command": "aibot_send_msg",
                "data": {
                    "chat_id": chat_id,
                    "msg": {
                        "content": {
                            "text": content
                        },
                        "msg_type": "markdown"
                    }
                }
            })
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

/// Reconnection logic with exponential backoff.
pub async fn reconnect_delay(attempt: u32) -> Duration {
    let base = Duration::from_secs(2);
    let delay = base * 2u32.saturating_pow(attempt.min(4));
    let capped = delay.min(MAX_RECONNECT_DELAY);
    sleep(capped).await;
    capped
}
