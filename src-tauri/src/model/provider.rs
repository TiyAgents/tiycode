use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Provider catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogEntryDto {
    pub provider_key: String,
    pub provider_type: String,
    pub display_name: String,
    pub builtin: bool,
    pub supports_custom: bool,
    pub default_base_url: String,
}

// ---------------------------------------------------------------------------
// Provider settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Builtin,
    Custom,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Custom => "custom",
        }
    }
}

impl From<String> for ProviderKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "builtin" => Self::Builtin,
            _ => Self::Custom,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderRecord {
    pub id: String,
    pub provider_kind: ProviderKind,
    pub provider_key: String,
    pub provider_type: String,
    pub display_name: String,
    pub base_url: String,
    pub api_key_encrypted: Option<String>,
    pub enabled: bool,
    pub mapping_locked: bool,
    pub custom_headers_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ProviderModelRecord {
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub context_window: Option<String>,
    pub max_output_tokens: Option<String>,
    pub capabilities_json: Option<String>,
    pub provider_options_json: Option<String>,
    pub is_manual: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelSettingsDto {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub context_window: Option<String>,
    pub max_output_tokens: Option<String>,
    pub capability_overrides: Option<serde_json::Value>,
    pub provider_options: Option<serde_json::Value>,
    pub is_manual: bool,
}

impl From<ProviderModelRecord> for ProviderModelSettingsDto {
    fn from(r: ProviderModelRecord) -> Self {
        Self {
            id: r.id,
            provider_id: r.provider_id,
            model_id: r.model_name,
            display_name: r.display_name,
            enabled: r.enabled,
            context_window: r.context_window,
            max_output_tokens: r.max_output_tokens,
            capability_overrides: r
                .capabilities_json
                .and_then(|s| serde_json::from_str(&s).ok()),
            provider_options: r
                .provider_options_json
                .and_then(|s| serde_json::from_str(&s).ok()),
            is_manual: r.is_manual,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsDto {
    pub id: String,
    pub kind: String,
    pub provider_key: String,
    pub provider_type: String,
    pub display_name: String,
    pub enabled: bool,
    pub locked_mapping: bool,
    pub base_url: String,
    pub has_api_key: bool,
    pub custom_headers: Option<serde_json::Value>,
    pub models: Vec<ProviderModelSettingsDto>,
    pub created_at: String,
    pub updated_at: String,
}

impl ProviderSettingsDto {
    pub fn from_record(record: ProviderRecord, models: Vec<ProviderModelRecord>) -> Self {
        Self {
            id: record.id,
            kind: record.provider_kind.as_str().to_string(),
            provider_key: record.provider_key,
            provider_type: record.provider_type,
            display_name: record.display_name,
            enabled: record.enabled,
            locked_mapping: record.mapping_locked,
            base_url: record.base_url,
            has_api_key: record.api_key_encrypted.is_some(),
            custom_headers: record
                .custom_headers_json
                .and_then(|s| serde_json::from_str(&s).ok()),
            models: models.into_iter().map(ProviderModelSettingsDto::from).collect(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInput {
    pub id: Option<String>,
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: Option<bool>,
    pub context_window: Option<String>,
    pub max_output_tokens: Option<String>,
    pub capability_overrides: Option<serde_json::Value>,
    pub provider_options: Option<serde_json::Value>,
    pub is_manual: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsUpdateInput {
    pub display_name: Option<String>,
    pub provider_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub custom_headers: Option<serde_json::Value>,
    pub models: Option<Vec<ProviderModelInput>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderCreateInput {
    pub display_name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub custom_headers: Option<serde_json::Value>,
    pub models: Option<Vec<ProviderModelInput>>,
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
