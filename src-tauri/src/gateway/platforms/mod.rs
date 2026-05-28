//! IM gateway platforms — WeChat (iLink Bot) and WeCom (AI Bot WebSocket).
//!
//! Each platform adapter implements `PlatformAdapter` and handles all
//! platform-specific protocol details internally.

pub mod wecom;
pub mod weixin;
