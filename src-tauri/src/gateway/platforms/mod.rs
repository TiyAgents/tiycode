//! IM gateway platforms — WeChat (iLink Bot), WeCom (AI Bot WebSocket),
//! Feishu/Lark (Webhook), and WhatsApp (Cloud API Webhook).
//!
//! Each platform adapter implements `PlatformAdapter` and handles all
//! platform-specific protocol details internally.

pub mod feishu;
pub mod wecom;
pub mod weixin;
pub mod weixin_auth;
pub mod weixin_media;
pub mod whatsapp;
