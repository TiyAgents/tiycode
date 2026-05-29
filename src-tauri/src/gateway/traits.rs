//! Platform adapter trait and shared message types for the IM gateway.

use std::path::Path;
use std::pin::Pin;

use futures::Stream;

use super::platforms::weixin_media::{MediaAttachment, UploadMediaType};

/// Inbound message received from an IM platform.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// Platform-specific unique message identifier (used for dedup).
    pub message_id: String,
    /// Sender identifier on the platform (e.g. wxid, wecom user_id).
    pub sender_id: String,
    /// Chat/conversation identifier (DM = sender_id, group = group_id).
    pub chat_id: String,
    /// Text content of the message.
    pub text: String,
    /// Whether this message was sent in a group chat.
    pub is_group: bool,
    /// Optional media attachment URLs (CDN download URLs for media items).
    pub media_urls: Vec<String>,
    /// Structured media attachments with full metadata (type, AES key, size, etc.).
    pub media_attachments: Vec<MediaAttachment>,
    /// WeCom request ID — required for group chat replies via `aibot_respond_msg`.
    pub req_id: Option<String>,
}

/// Result of a send operation.
#[derive(Debug, Clone)]
pub struct SendResult {
    pub success: bool,
    pub message_id: Option<String>,
    pub error: Option<String>,
}

impl SendResult {
    pub fn ok(message_id: Option<String>) -> Self {
        Self {
            success: true,
            message_id,
            error: None,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            message_id: None,
            error: Some(error.into()),
        }
    }
}

/// Platform identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Weixin,
    Wecom,
    Feishu,
    WhatsApp,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Weixin => write!(f, "weixin"),
            Platform::Wecom => write!(f, "wecom"),
            Platform::Feishu => write!(f, "feishu"),
            Platform::WhatsApp => write!(f, "whatsapp"),
        }
    }
}

impl std::str::FromStr for Platform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "weixin" | "wechat" => Ok(Platform::Weixin),
            "wecom" | "wework" => Ok(Platform::Wecom),
            "feishu" | "lark" => Ok(Platform::Feishu),
            "whatsapp" => Ok(Platform::WhatsApp),
            other => Err(format!("unknown platform: {other}")),
        }
    }
}

/// Trait that each IM platform adapter must implement.
///
/// The gateway runner drives the adapter through connect → poll_messages →
/// send_text lifecycle. Adapters handle all platform-specific protocol details
/// (authentication, heartbeat, reconnection, etc.) internally.
#[async_trait::async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Establish connection to the platform (authenticate, start polling, etc.).
    async fn connect(&mut self) -> anyhow::Result<()>;

    /// Gracefully disconnect from the platform.
    async fn disconnect(&mut self);

    /// Send a text message to the specified chat.
    async fn send_text(&self, chat_id: &str, text: &str) -> anyhow::Result<SendResult>;

    /// Send a media file to the specified chat.
    ///
    /// Default implementation returns an unsupported error. Platforms that
    /// support media upload (e.g. WeChat CDN) should override this.
    async fn send_media(
        &self,
        _chat_id: &str,
        _file_path: &Path,
        _media_type: UploadMediaType,
    ) -> anyhow::Result<SendResult> {
        Ok(SendResult::err(
            "media sending not supported on this platform",
        ))
    }

    /// Send typing indicator to the specified chat (best-effort, errors ignored).
    async fn send_typing(&self, chat_id: &str) -> anyhow::Result<()>;

    /// Stop typing indicator (best-effort).
    async fn stop_typing(&self, chat_id: &str) -> anyhow::Result<()>;

    /// Return a stream of inbound messages from the platform.
    ///
    /// The stream should handle reconnection internally and only terminate
    /// when the adapter is disconnected or encounters a fatal error.
    fn poll_messages(
        &self,
    ) -> Pin<Box<dyn Stream<Item = anyhow::Result<InboundMessage>> + Send + '_>>;

    /// The platform identifier.
    fn platform(&self) -> Platform;
}
