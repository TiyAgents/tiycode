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
    pub sort_index: i64,
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
    pub sort_index: i64,
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
            sort_index: r.sort_index,
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
            models: models
                .into_iter()
                .map(ProviderModelSettingsDto::from)
                .collect(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConnectionTestResultDto {
    pub success: bool,
    pub unsupported: bool,
    pub message: String,
    pub detail: Option<String>,
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
    pub commit_message_prompt: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub commit_message_language: Option<String>,
    pub thinking_level: Option<String>,
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
    pub commit_message_prompt: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub commit_message_language: Option<String>,
    pub thinking_level: Option<String>,
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
            commit_message_prompt: r.commit_message_prompt,
            response_style: r.response_style,
            response_language: r.response_language,
            commit_message_language: r.commit_message_language,
            thinking_level: r.thinking_level,
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
    pub commit_message_prompt: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub commit_message_language: Option<String>,
    pub thinking_level: Option<String>,
    pub primary_provider_id: Option<String>,
    pub primary_model_id: Option<String>,
    pub auxiliary_provider_id: Option<String>,
    pub auxiliary_model_id: Option<String>,
    pub lightweight_provider_id: Option<String>,
    pub lightweight_model_id: Option<String>,
    pub is_default: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_kind_maps_strings_and_defaults_unknown_to_custom() {
        assert_eq!(ProviderKind::Builtin.as_str(), "builtin");
        assert_eq!(ProviderKind::Custom.as_str(), "custom");
        assert_eq!(
            ProviderKind::from("builtin".to_string()),
            ProviderKind::Builtin
        );
        assert_eq!(
            ProviderKind::from("openai".to_string()),
            ProviderKind::Custom
        );
    }

    #[test]
    fn provider_model_settings_dto_parses_valid_json_and_ignores_invalid_json() {
        let dto = ProviderModelSettingsDto::from(ProviderModelRecord {
            id: "model-row-1".to_string(),
            provider_id: "provider-1".to_string(),
            model_name: "claude-sonnet".to_string(),
            sort_index: 7,
            display_name: Some("Claude Sonnet".to_string()),
            enabled: true,
            context_window: Some("200000".to_string()),
            max_output_tokens: Some("8192".to_string()),
            capabilities_json: Some(r#"{"tools":true}"#.to_string()),
            provider_options_json: Some("not-json".to_string()),
            is_manual: false,
            created_at: "2026-04-24T00:00:00Z".to_string(),
        });

        assert_eq!(dto.id, "model-row-1");
        assert_eq!(dto.model_id, "claude-sonnet");
        assert_eq!(dto.sort_index, 7);
        assert_eq!(dto.capability_overrides, Some(json!({ "tools": true })));
        assert_eq!(dto.provider_options, None);
        assert!(!dto.is_manual);
    }

    #[test]
    fn provider_settings_dto_from_record_sets_flags_and_nested_models() {
        let provider = ProviderRecord {
            id: "provider-1".to_string(),
            provider_kind: ProviderKind::Builtin,
            provider_key: "anthropic".to_string(),
            provider_type: "anthropic".to_string(),
            display_name: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            api_key_encrypted: Some("encrypted".to_string()),
            enabled: true,
            mapping_locked: true,
            custom_headers_json: Some(r#"{"x-test":"enabled"}"#.to_string()),
            created_at: "2026-04-24T00:00:00Z".to_string(),
            updated_at: "2026-04-24T01:00:00Z".to_string(),
        };
        let models = vec![ProviderModelRecord {
            id: "model-row-1".to_string(),
            provider_id: "provider-1".to_string(),
            model_name: "claude-sonnet".to_string(),
            sort_index: 0,
            display_name: None,
            enabled: true,
            context_window: None,
            max_output_tokens: None,
            capabilities_json: Some("not-json".to_string()),
            provider_options_json: Some(r#"{"temperature":0.2}"#.to_string()),
            is_manual: true,
            created_at: "2026-04-24T00:00:00Z".to_string(),
        }];

        let dto = ProviderSettingsDto::from_record(provider, models);

        assert_eq!(dto.id, "provider-1");
        assert_eq!(dto.kind, "builtin");
        assert_eq!(dto.provider_key, "anthropic");
        assert!(dto.enabled);
        assert!(dto.locked_mapping);
        assert!(dto.has_api_key);
        assert_eq!(dto.custom_headers, Some(json!({ "x-test": "enabled" })));
        assert_eq!(dto.models.len(), 1);
        assert_eq!(dto.models[0].capability_overrides, None);
        assert_eq!(
            dto.models[0].provider_options,
            Some(json!({ "temperature": 0.2 }))
        );
        assert!(dto.models[0].is_manual);
    }

    #[test]
    fn agent_profile_dto_from_record_preserves_language_style_and_model_layers() {
        let dto = AgentProfileDto::from(AgentProfileRecord {
            id: "profile-1".to_string(),
            name: "Default".to_string(),
            custom_instructions: Some("Be concise".to_string()),
            commit_message_prompt: Some("Write conventional commits".to_string()),
            response_style: Some("concise".to_string()),
            response_language: Some("zh-CN".to_string()),
            commit_message_language: Some("en-US".to_string()),
            thinking_level: Some("high".to_string()),
            primary_provider_id: Some("primary-provider".to_string()),
            primary_model_id: Some("primary-model".to_string()),
            auxiliary_provider_id: Some("aux-provider".to_string()),
            auxiliary_model_id: Some("aux-model".to_string()),
            lightweight_provider_id: Some("light-provider".to_string()),
            lightweight_model_id: Some("light-model".to_string()),
            is_default: true,
            created_at: "2026-04-24T00:00:00Z".to_string(),
            updated_at: "2026-04-24T01:00:00Z".to_string(),
        });

        assert_eq!(dto.name, "Default");
        assert_eq!(dto.custom_instructions.as_deref(), Some("Be concise"));
        assert_eq!(
            dto.commit_message_prompt.as_deref(),
            Some("Write conventional commits")
        );
        assert_eq!(dto.response_style.as_deref(), Some("concise"));
        assert_eq!(dto.response_language.as_deref(), Some("zh-CN"));
        assert_eq!(dto.commit_message_language.as_deref(), Some("en-US"));
        assert_eq!(dto.thinking_level.as_deref(), Some("high"));
        assert_eq!(dto.primary_provider_id.as_deref(), Some("primary-provider"));
        assert_eq!(dto.primary_model_id.as_deref(), Some("primary-model"));
        assert_eq!(dto.auxiliary_provider_id.as_deref(), Some("aux-provider"));
        assert_eq!(dto.auxiliary_model_id.as_deref(), Some("aux-model"));
        assert_eq!(
            dto.lightweight_provider_id.as_deref(),
            Some("light-provider")
        );
        assert_eq!(dto.lightweight_model_id.as_deref(), Some("light-model"));
        assert!(dto.is_default);
    }
}
