use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub protocol_type: String,
    pub base_url: String,
    pub api_key_encrypted: Option<String>,
    pub enabled: bool,
    pub custom_headers_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDto {
    pub id: String,
    pub name: String,
    pub protocol_type: String,
    pub base_url: String,
    /// Never expose the raw key; frontend sees whether one is set.
    pub has_api_key: bool,
    pub enabled: bool,
    pub custom_headers: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ProviderRecord> for ProviderDto {
    fn from(r: ProviderRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            protocol_type: r.protocol_type,
            base_url: r.base_url,
            has_api_key: r.api_key_encrypted.is_some(),
            enabled: r.enabled,
            custom_headers: r
                .custom_headers_json
                .and_then(|s| serde_json::from_str(&s).ok()),
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub name: String,
    pub protocol_type: Option<String>,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub custom_headers: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// ProviderModel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProviderModelRecord {
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub capabilities_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelDto {
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub capabilities: Option<serde_json::Value>,
}

impl From<ProviderModelRecord> for ProviderModelDto {
    fn from(r: ProviderModelRecord) -> Self {
        Self {
            id: r.id,
            provider_id: r.provider_id,
            model_name: r.model_name,
            display_name: r.display_name,
            enabled: r.enabled,
            capabilities: r
                .capabilities_json
                .and_then(|s| serde_json::from_str(&s).ok()),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInput {
    pub model_name: String,
    pub display_name: Option<String>,
    pub enabled: Option<bool>,
    pub capabilities: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// AgentProfile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AgentProfileRecord {
    pub id: String,
    pub name: String,
    pub custom_instructions: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub primary_provider_id: Option<String>,
    pub primary_model_id: Option<String>,
    pub auxiliary_provider_id: Option<String>,
    pub auxiliary_model_id: Option<String>,
    pub lightweight_provider_id: Option<String>,
    pub lightweight_model_id: Option<String>,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileDto {
    pub id: String,
    pub name: String,
    pub custom_instructions: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub primary_provider_id: Option<String>,
    pub primary_model_id: Option<String>,
    pub auxiliary_provider_id: Option<String>,
    pub auxiliary_model_id: Option<String>,
    pub lightweight_provider_id: Option<String>,
    pub lightweight_model_id: Option<String>,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<AgentProfileRecord> for AgentProfileDto {
    fn from(r: AgentProfileRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            custom_instructions: r.custom_instructions,
            response_style: r.response_style,
            response_language: r.response_language,
            primary_provider_id: r.primary_provider_id,
            primary_model_id: r.primary_model_id,
            auxiliary_provider_id: r.auxiliary_provider_id,
            auxiliary_model_id: r.auxiliary_model_id,
            lightweight_provider_id: r.lightweight_provider_id,
            lightweight_model_id: r.lightweight_model_id,
            is_default: r.is_default,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileInput {
    pub name: String,
    pub custom_instructions: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub primary_provider_id: Option<String>,
    pub primary_model_id: Option<String>,
    pub auxiliary_provider_id: Option<String>,
    pub auxiliary_model_id: Option<String>,
    pub lightweight_provider_id: Option<String>,
    pub lightweight_model_id: Option<String>,
    pub is_default: Option<bool>,
}
