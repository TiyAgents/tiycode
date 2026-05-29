//! Gateway configuration structures and TOML loading.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::traits::Platform;

// ── Policy enums ─────────────────────────────────────────────────────

/// Group message policy for Feishu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", try_from = "String")]
pub enum GroupPolicy {
    Open,
    Allowlist,
    Blacklist,
    Disabled,
}

impl Default for GroupPolicy {
    fn default() -> Self {
        GroupPolicy::Open
    }
}

impl std::fmt::Display for GroupPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GroupPolicy::Open => write!(f, "open"),
            GroupPolicy::Allowlist => write!(f, "allowlist"),
            GroupPolicy::Blacklist => write!(f, "blacklist"),
            GroupPolicy::Disabled => write!(f, "disabled"),
        }
    }
}

impl std::str::FromStr for GroupPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(GroupPolicy::Open),
            "allowlist" => Ok(GroupPolicy::Allowlist),
            "blacklist" => Ok(GroupPolicy::Blacklist),
            "disabled" => Ok(GroupPolicy::Disabled),
            other => Err(format!("unknown group_policy: {other}")),
        }
    }
}

impl TryFrom<String> for GroupPolicy {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// DM (direct message) policy for WhatsApp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", try_from = "String")]
pub enum DmPolicy {
    Open,
    Allowlist,
    Disabled,
}

impl Default for DmPolicy {
    fn default() -> Self {
        DmPolicy::Open
    }
}

impl std::fmt::Display for DmPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DmPolicy::Open => write!(f, "open"),
            DmPolicy::Allowlist => write!(f, "allowlist"),
            DmPolicy::Disabled => write!(f, "disabled"),
        }
    }
}

impl std::str::FromStr for DmPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(DmPolicy::Open),
            "allowlist" => Ok(DmPolicy::Allowlist),
            "disabled" => Ok(DmPolicy::Disabled),
            other => Err(format!("unknown dm_policy: {other}")),
        }
    }
}

impl TryFrom<String> for DmPolicy {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// Top-level gateway configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
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

    /// Feishu/Lark Bot configuration.
    pub feishu: Option<FeishuConfig>,

    /// WhatsApp Cloud API Bot configuration.
    pub whatsapp: Option<WhatsAppConfig>,
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
            Platform::Feishu => {
                if self.feishu.is_none() {
                    anyhow::bail!("platform is 'feishu' but [feishu] config section is missing");
                }
            }
            Platform::WhatsApp => {
                if self.whatsapp.is_none() {
                    anyhow::bail!(
                        "platform is 'whatsapp' but [whatsapp] config section is missing"
                    );
                }
            }
        }
        Ok(())
    }
}

/// WeChat iLink Bot configuration.
#[derive(Clone, Deserialize, Serialize)]
pub struct WeixinConfig {
    /// Whether this channel is enabled.
    #[serde(default = "default_false")]
    pub enabled: bool,

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
#[derive(Clone, Deserialize, Serialize)]
pub struct WecomConfig {
    /// Whether this channel is enabled.
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Bot ID for authentication.
    #[serde(default)]
    pub bot_id: String,

    /// Secret for authentication.
    #[serde(default)]
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

/// Feishu/Lark Bot Webhook configuration.
#[derive(Clone, Deserialize, Serialize)]
pub struct FeishuConfig {
    /// Whether this channel is enabled.
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Feishu App ID.
    #[serde(default)]
    pub app_id: String,

    /// Feishu App Secret.
    #[serde(default)]
    pub app_secret: String,

    /// Encrypt key for event payload decryption (optional).
    #[serde(default)]
    pub encrypt_key: Option<String>,

    /// Verification token for event validation (optional).
    #[serde(default)]
    pub verification_token: Option<String>,

    /// Webhook listen host (default: 127.0.0.1).
    #[serde(default = "default_webhook_host")]
    pub webhook_host: String,

    /// Webhook listen port (default: 8765).
    #[serde(default = "default_feishu_webhook_port")]
    pub webhook_port: u16,

    /// Group message policy.
    #[serde(default)]
    pub group_policy: GroupPolicy,

    /// Group allowlist (chat IDs).
    #[serde(default)]
    pub group_allowlist: Vec<String>,

    /// Group blacklist (chat IDs).
    #[serde(default)]
    pub group_blacklist: Vec<String>,

    /// Feishu API domain (default: open.feishu.cn).
    #[serde(default = "default_feishu_domain")]
    pub domain: String,
}

impl std::fmt::Debug for FeishuConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeishuConfig")
            .field("app_id", &self.app_id)
            .field("app_secret", &"[REDACTED]")
            .field("encrypt_key_set", &self.encrypt_key.is_some())
            .field("verification_token_set", &self.verification_token.is_some())
            .field("webhook_host", &self.webhook_host)
            .field("webhook_port", &self.webhook_port)
            .field("group_policy", &self.group_policy)
            .field("domain", &self.domain)
            .finish()
    }
}

/// WhatsApp Cloud API Bot configuration.
#[derive(Clone, Deserialize, Serialize)]
pub struct WhatsAppConfig {
    /// Whether this channel is enabled.
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// WhatsApp Business phone number ID.
    #[serde(default)]
    pub phone_number_id: String,

    /// WhatsApp Cloud API access token.
    #[serde(default)]
    pub access_token: String,

    /// App secret for webhook signature verification.
    #[serde(default)]
    pub app_secret: String,

    /// Verify token for Meta webhook subscription.
    #[serde(default)]
    pub webhook_verify_token: String,

    /// Webhook listen host (default: 127.0.0.1).
    #[serde(default = "default_webhook_host")]
    pub webhook_host: String,

    /// Webhook listen port (default: 8766).
    #[serde(default = "default_whatsapp_webhook_port")]
    pub webhook_port: u16,

    /// DM policy.
    #[serde(default)]
    pub dm_policy: DmPolicy,

    /// DM allowlist (phone numbers).
    #[serde(default)]
    pub allow_from: Vec<String>,

    /// WhatsApp Cloud API version (default: v21.0).
    #[serde(default = "default_whatsapp_api_version")]
    pub api_version: String,
}

impl std::fmt::Debug for WhatsAppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhatsAppConfig")
            .field("phone_number_id", &self.phone_number_id)
            .field("access_token", &"[REDACTED]")
            .field("app_secret_set", &!self.app_secret.is_empty())
            .field("webhook_host", &self.webhook_host)
            .field("webhook_port", &self.webhook_port)
            .field("dm_policy", &self.dm_policy)
            .field("api_version", &self.api_version)
            .finish()
    }
}

/// Resolve the default config file path: `~/.tiy/gateway/config.toml`.
pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tiy/gateway/config.toml")
}

/// Save a gateway config to the default TOML path atomically.
pub fn save_config(config: &GatewayConfig) -> anyhow::Result<()> {
    let path = default_config_path();
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let toml_str = toml::to_string_pretty(config)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &toml_str)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Load the gateway config from default path, or return None if missing.
pub fn load_config() -> Option<GatewayConfig> {
    let path = default_config_path();
    GatewayConfig::load(&path).ok()
}

/// Frontend-safe DTO for the gateway config (secrets masked).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfigDto {
    pub platform: String,
    pub weixin_enabled: bool,
    pub wecom_enabled: bool,
    pub wecom_bot_id: String,
    pub wecom_secret_set: bool,
    pub wecom_ws_url: String,
    pub feishu_enabled: bool,
    pub feishu_app_id: String,
    pub feishu_app_secret_set: bool,
    pub feishu_webhook_host: String,
    pub feishu_webhook_port: u16,
    pub feishu_group_policy: String,
    pub feishu_encrypt_key_set: bool,
    pub feishu_verification_token_set: bool,
    pub whatsapp_enabled: bool,
    pub whatsapp_phone_number_id: String,
    pub whatsapp_access_token_set: bool,
    pub whatsapp_webhook_host: String,
    pub whatsapp_webhook_port: u16,
    pub whatsapp_dm_policy: String,
    pub whatsapp_app_secret_set: bool,
    pub whatsapp_webhook_verify_token_set: bool,
}

impl GatewayConfigDto {
    pub fn from_config(config: &GatewayConfig) -> Self {
        Self {
            platform: config.platform.to_string(),
            weixin_enabled: config.weixin.as_ref().map(|w| w.enabled).unwrap_or(false),
            wecom_enabled: config.wecom.as_ref().map(|w| w.enabled).unwrap_or(false),
            wecom_bot_id: config
                .wecom
                .as_ref()
                .map(|w| w.bot_id.clone())
                .unwrap_or_default(),
            wecom_secret_set: config
                .wecom
                .as_ref()
                .map(|w| !w.secret.is_empty())
                .unwrap_or(false),
            wecom_ws_url: config
                .wecom
                .as_ref()
                .map(|w| w.ws_url.clone())
                .unwrap_or_else(default_wecom_ws_url),
            feishu_enabled: config.feishu.as_ref().map(|f| f.enabled).unwrap_or(false),
            feishu_app_id: config
                .feishu
                .as_ref()
                .map(|f| f.app_id.clone())
                .unwrap_or_default(),
            feishu_app_secret_set: config
                .feishu
                .as_ref()
                .map(|f| !f.app_secret.is_empty())
                .unwrap_or(false),
            feishu_webhook_host: config
                .feishu
                .as_ref()
                .map(|f| f.webhook_host.clone())
                .unwrap_or_else(default_webhook_host),
            feishu_webhook_port: config
                .feishu
                .as_ref()
                .map(|f| f.webhook_port)
                .unwrap_or_else(default_feishu_webhook_port),
            feishu_group_policy: config
                .feishu
                .as_ref()
                .map(|f| f.group_policy.to_string())
                .unwrap_or_else(|| GroupPolicy::default().to_string()),
            feishu_encrypt_key_set: config
                .feishu
                .as_ref()
                .map(|f| f.encrypt_key.is_some())
                .unwrap_or(false),
            feishu_verification_token_set: config
                .feishu
                .as_ref()
                .map(|f| f.verification_token.is_some())
                .unwrap_or(false),
            whatsapp_enabled: config.whatsapp.as_ref().map(|w| w.enabled).unwrap_or(false),
            whatsapp_phone_number_id: config
                .whatsapp
                .as_ref()
                .map(|w| w.phone_number_id.clone())
                .unwrap_or_default(),
            whatsapp_access_token_set: config
                .whatsapp
                .as_ref()
                .map(|w| !w.access_token.is_empty())
                .unwrap_or(false),
            whatsapp_webhook_host: config
                .whatsapp
                .as_ref()
                .map(|w| w.webhook_host.clone())
                .unwrap_or_else(default_webhook_host),
            whatsapp_webhook_port: config
                .whatsapp
                .as_ref()
                .map(|w| w.webhook_port)
                .unwrap_or_else(default_whatsapp_webhook_port),
            whatsapp_dm_policy: config
                .whatsapp
                .as_ref()
                .map(|w| w.dm_policy.to_string())
                .unwrap_or_else(|| DmPolicy::default().to_string()),
            whatsapp_app_secret_set: config
                .whatsapp
                .as_ref()
                .map(|w| !w.app_secret.is_empty())
                .unwrap_or(false),
            whatsapp_webhook_verify_token_set: config
                .whatsapp
                .as_ref()
                .map(|w| !w.webhook_verify_token.is_empty())
                .unwrap_or(false),
        }
    }
}

/// Input DTO for saving gateway config from the frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfigUpdateInput {
    pub weixin_enabled: bool,
    pub wecom_enabled: bool,
    pub wecom_bot_id: String,
    /// Empty string means "don't change the secret".
    pub wecom_secret: String,
    pub wecom_ws_url: String,
    pub feishu_enabled: bool,
    pub feishu_app_id: String,
    /// Empty string means "don't change the app secret".
    pub feishu_app_secret: String,
    pub feishu_webhook_host: String,
    pub feishu_webhook_port: u16,
    pub feishu_group_policy: String,
    /// None means "don't change", Some("") means "clear".
    pub feishu_encrypt_key: Option<String>,
    /// None means "don't change", Some("") means "clear".
    pub feishu_verification_token: Option<String>,
    pub whatsapp_enabled: bool,
    pub whatsapp_phone_number_id: String,
    /// Empty string means "don't change the access token".
    pub whatsapp_access_token: String,
    pub whatsapp_webhook_host: String,
    pub whatsapp_webhook_port: u16,
    pub whatsapp_dm_policy: String,
    /// Empty string means "don't change", omitted means "don't change".
    pub whatsapp_app_secret: Option<String>,
    /// Empty string means "don't change", omitted means "don't change".
    pub whatsapp_webhook_verify_token: Option<String>,
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

fn default_false() -> bool {
    false
}

fn default_webhook_host() -> String {
    "127.0.0.1".to_string()
}

fn default_feishu_webhook_port() -> u16 {
    8765
}

fn default_feishu_domain() -> String {
    "open.feishu.cn".to_string()
}

fn default_whatsapp_webhook_port() -> u16 {
    8766
}

fn default_whatsapp_api_version() -> String {
    "v21.0".to_string()
}
