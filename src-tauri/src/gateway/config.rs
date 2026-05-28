//! Gateway configuration structures and TOML loading.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::traits::Platform;

/// Top-level gateway configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    /// Which platform to activate.
    pub platform: Platform,

    /// Timeout in seconds for tool approval requests sent via IM (default: 60).
    #[serde(default = "default_approval_timeout")]
    pub approval_timeout_seconds: u64,

    /// Delay between sending message chunks (seconds).
    #[serde(default = "default_send_chunk_delay")]
    pub send_chunk_delay_seconds: f64,

    /// WeChat (iLink Bot) configuration.
    pub weixin: Option<WeixinConfig>,

    /// WeCom (Enterprise WeChat AI Bot) configuration.
    pub wecom: Option<WecomConfig>,
}

impl GatewayConfig {
    /// Load configuration from a TOML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read config file {}: {}", path.display(), e))?;
        let config: Self = toml::from_str(&content).map_err(|e| {
            anyhow::anyhow!("failed to parse config file {}: {}", path.display(), e)
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Resolve the effective send chunk delay for the active platform.
    pub fn send_chunk_delay(&self) -> Duration {
        Duration::from_secs_f64(self.send_chunk_delay_seconds)
    }

    /// Validate that the required platform config section is present.
    fn validate(&self) -> anyhow::Result<()> {
        match self.platform {
            Platform::Weixin => {
                if self.weixin.is_none() {
                    anyhow::bail!("platform is 'weixin' but [weixin] config section is missing");
                }
            }
            Platform::Wecom => {
                if self.wecom.is_none() {
                    anyhow::bail!("platform is 'wecom' but [wecom] config section is missing");
                }
            }
        }
        Ok(())
    }
}

/// WeChat iLink Bot configuration.
#[derive(Clone, Deserialize)]
pub struct WeixinConfig {
    /// The iLink bot account ID (can be empty if obtained via QR login).
    #[serde(default)]
    pub account_id: String,

    /// Bearer token for iLink API authentication.
    /// If empty/missing, the adapter will attempt to load from session file.
    #[serde(default)]
    pub token: Option<String>,

    /// Base URL for the iLink API (default: ilinkai.weixin.qq.com).
    #[serde(default = "default_weixin_base_url")]
    pub base_url: String,

    /// CDN base URL for media upload/download.
    #[serde(default = "default_weixin_cdn_base_url")]
    pub cdn_base_url: String,
}

impl WeixinConfig {
    /// Resolve the effective token: config value > session file.
    ///
    /// Returns `None` if no token is available (QR login required).
    pub fn effective_token(&self) -> Option<String> {
        // 1. Prefer explicit config token.
        if let Some(ref t) = self.token {
            if !t.is_empty() {
                return Some(t.clone());
            }
        }
        // 2. Fall back to persisted session file.
        use super::platforms::weixin_auth;
        weixin_auth::load_session().map(|s| s.token)
    }

    /// Resolve the effective account_id: config value > session file.
    pub fn effective_account_id(&self) -> String {
        if !self.account_id.is_empty() {
            return self.account_id.clone();
        }
        use super::platforms::weixin_auth;
        weixin_auth::load_session()
            .map(|s| s.account_id)
            .unwrap_or_default()
    }
}

impl std::fmt::Debug for WeixinConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeixinConfig")
            .field("account_id", &self.account_id)
            .field("token", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("cdn_base_url", &self.cdn_base_url)
            .finish()
    }
}

/// WeCom AI Bot WebSocket configuration.
#[derive(Clone, Deserialize)]
pub struct WecomConfig {
    /// Bot ID for authentication.
    pub bot_id: String,

    /// Secret for authentication.
    pub secret: String,

    /// WebSocket URL (default: openws.work.weixin.qq.com).
    #[serde(default = "default_wecom_ws_url")]
    pub ws_url: String,
}

impl std::fmt::Debug for WecomConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WecomConfig")
            .field("bot_id", &self.bot_id)
            .field("secret", &"[REDACTED]")
            .field("ws_url", &self.ws_url)
            .finish()
    }
}

/// Resolve the default config file path: `~/.tiy/gateway/config.toml`.
pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tiy/gateway/config.toml")
}

// --- Default value functions ---

fn default_approval_timeout() -> u64 {
    60
}

fn default_send_chunk_delay() -> f64 {
    1.5
}

fn default_weixin_base_url() -> String {
    "ilinkai.weixin.qq.com".to_string()
}

fn default_weixin_cdn_base_url() -> String {
    "novac2c.cdn.weixin.qq.com/c2c".to_string()
}

fn default_wecom_ws_url() -> String {
    "openws.work.weixin.qq.com".to_string()
}
