use sqlx::SqlitePool;
use tiy_core::models::ModelRegistry;
use tiy_core::types::Provider as TiyProvider;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::{
    AgentProfileInput, AgentProfileRecord, CustomProviderCreateInput, ProviderCatalogEntryDto,
    ProviderKind, ProviderModelInput, ProviderModelRecord, ProviderRecord, ProviderSettingsDto,
    ProviderSettingsUpdateInput,
};
use crate::model::settings::SettingRecord;
use crate::persistence::repo::{profile_repo, provider_repo, settings_repo};

const PROVIDER_SCHEMA_VERSION_KEY: &str = "providers.schema_version";
const PROVIDER_SCHEMA_VERSION: u32 = 2;

struct ProviderCatalogEntry {
    provider_key: &'static str,
    provider_type: &'static str,
    display_name: &'static str,
    default_base_url: &'static str,
}

const BUILTIN_PROVIDER_CATALOG: &[ProviderCatalogEntry] = &[
    ProviderCatalogEntry {
        provider_key: "openai",
        provider_type: "openai",
        display_name: "OpenAI",
        default_base_url: "https://api.openai.com/v1",
    },
    ProviderCatalogEntry {
        provider_key: "anthropic",
        provider_type: "anthropic",
        display_name: "Anthropic",
        default_base_url: "https://api.anthropic.com/v1",
    },
    ProviderCatalogEntry {
        provider_key: "google",
        provider_type: "google",
        display_name: "Google",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta",
    },
    ProviderCatalogEntry {
        provider_key: "ollama",
        provider_type: "ollama",
        display_name: "Ollama",
        default_base_url: "http://localhost:11434/v1",
    },
    ProviderCatalogEntry {
        provider_key: "xai",
        provider_type: "xai",
        display_name: "xAI",
        default_base_url: "https://api.x.ai/v1",
    },
    ProviderCatalogEntry {
        provider_key: "groq",
        provider_type: "groq",
        display_name: "Groq",
        default_base_url: "https://api.groq.com/openai/v1",
    },
    ProviderCatalogEntry {
        provider_key: "openrouter",
        provider_type: "openrouter",
        display_name: "OpenRouter",
        default_base_url: "https://openrouter.ai/api/v1",
    },
    ProviderCatalogEntry {
        provider_key: "minimax",
        provider_type: "minimax",
        display_name: "MiniMax",
        default_base_url: "https://api.minimax.io/anthropic",
    },
    ProviderCatalogEntry {
        provider_key: "minimax-cn",
        provider_type: "minimax-cn",
        display_name: "MiniMax CN",
        default_base_url: "https://api.minimaxi.com/anthropic",
    },
    ProviderCatalogEntry {
        provider_key: "kimi-coding",
        provider_type: "kimi-coding",
        display_name: "Kimi Coding",
        default_base_url: "https://api.kimi.com/coding",
    },
    ProviderCatalogEntry {
        provider_key: "zai",
        provider_type: "zai",
        display_name: "ZAI",
        default_base_url: "https://api.z.ai/api/coding/paas/v4",
    },
    ProviderCatalogEntry {
        provider_key: "deepseek",
        provider_type: "deepseek",
        display_name: "DeepSeek",
        default_base_url: "https://api.deepseek.com",
    },
    ProviderCatalogEntry {
        provider_key: "zenmux",
        provider_type: "zenmux",
        display_name: "ZenMux",
        default_base_url: "https://zenmux.ai/api/v1",
    },
];

const CUSTOM_PROVIDER_TYPE_CATALOG: &[ProviderCatalogEntry] = &[
    ProviderCatalogEntry {
        provider_key: "openai-compatible",
        provider_type: "openai-compatible",
        display_name: "OpenAI Compatible",
        default_base_url: "https://api.example.com/v1",
    },
    ProviderCatalogEntry {
        provider_key: "anthropic",
        provider_type: "anthropic",
        display_name: "Anthropic",
        default_base_url: "https://api.anthropic.com/v1",
    },
    ProviderCatalogEntry {
        provider_key: "google",
        provider_type: "google",
        display_name: "Google",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta",
    },
    ProviderCatalogEntry {
        provider_key: "ollama",
        provider_type: "ollama",
        display_name: "Ollama",
        default_base_url: "http://localhost:11434/v1",
    },
];

pub struct SettingsManager {
    pool: SqlitePool,
}

impl SettingsManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // -----------------------------------------------------------------------
    // Settings KV
    // -----------------------------------------------------------------------

    pub async fn get_setting(&self, key: &str) -> Result<Option<SettingRecord>, AppError> {
        settings_repo::get(&self.pool, key).await
    }

    pub async fn get_all_settings(&self) -> Result<Vec<SettingRecord>, AppError> {
        settings_repo::get_all(&self.pool).await
    }

    pub async fn set_setting(&self, key: &str, value_json: &str) -> Result<(), AppError> {
        serde_json::from_str::<serde_json::Value>(value_json).map_err(|e| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.invalid_json",
                format!("Invalid JSON value: {e}"),
            )
        })?;
        settings_repo::set(&self.pool, key, value_json).await
    }

    // -----------------------------------------------------------------------
    // Policies KV
    // -----------------------------------------------------------------------

    pub async fn get_policy(&self, key: &str) -> Result<Option<SettingRecord>, AppError> {
        settings_repo::policy_get(&self.pool, key).await
    }

    pub async fn get_all_policies(&self) -> Result<Vec<SettingRecord>, AppError> {
        settings_repo::policy_get_all(&self.pool).await
    }

    pub async fn set_policy(&self, key: &str, value_json: &str) -> Result<(), AppError> {
        serde_json::from_str::<serde_json::Value>(value_json).map_err(|e| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.invalid_json",
                format!("Invalid JSON value: {e}"),
            )
        })?;
        settings_repo::policy_set(&self.pool, key, value_json).await
    }

    // -----------------------------------------------------------------------
    // Provider catalog and settings
    // -----------------------------------------------------------------------

    pub async fn list_provider_catalog(&self) -> Result<Vec<ProviderCatalogEntryDto>, AppError> {
        let builtin_entries = BUILTIN_PROVIDER_CATALOG.iter().map(|entry| ProviderCatalogEntryDto {
                provider_key: entry.provider_key.to_string(),
                provider_type: entry.provider_type.to_string(),
                display_name: entry.display_name.to_string(),
                builtin: true,
                supports_custom: false,
                default_base_url: entry.default_base_url.to_string(),
            });

        let custom_entries = CUSTOM_PROVIDER_TYPE_CATALOG.iter().map(|entry| ProviderCatalogEntryDto {
                provider_key: entry.provider_key.to_string(),
                provider_type: entry.provider_type.to_string(),
                display_name: entry.display_name.to_string(),
                builtin: false,
                supports_custom: true,
                default_base_url: entry.default_base_url.to_string(),
            });

        Ok(builtin_entries.chain(custom_entries).collect())
    }

    pub async fn get_all_provider_settings(&self) -> Result<Vec<ProviderSettingsDto>, AppError> {
        self.ensure_provider_state_ready().await?;

        let providers = provider_repo::list_all(&self.pool).await?;
        let mut result = Vec::with_capacity(providers.len());
        for provider in providers {
            result.push(self.provider_settings_from_record(provider).await?);
        }
        Ok(result)
    }

    pub async fn upsert_builtin_provider_settings(
        &self,
        provider_key: &str,
        input: ProviderSettingsUpdateInput,
    ) -> Result<ProviderSettingsDto, AppError> {
        self.ensure_provider_state_ready().await?;

        let existing = provider_repo::find_by_key(&self.pool, provider_key)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "builtin provider"))?;

        if existing.provider_kind != ProviderKind::Builtin {
            return Err(AppError::recoverable(
                ErrorSource::Settings,
                "settings.provider.invalid_builtin",
                "Only built-in providers can be updated with this command",
            ));
        }

        if let Some(provider_type) = &input.provider_type {
            if provider_type != &existing.provider_type {
                return Err(AppError::recoverable(
                    ErrorSource::Settings,
                    "settings.provider.mapping_locked",
                    "Built-in provider mapping cannot be changed",
                ));
            }
        }

        if let Some(display_name) = &input.display_name {
            if display_name != &existing.display_name {
                return Err(AppError::recoverable(
                    ErrorSource::Settings,
                    "settings.provider.name_locked",
                    "Built-in provider display name cannot be changed",
                ));
            }
        }

        let updated = ProviderRecord {
            id: existing.id.clone(),
            provider_kind: ProviderKind::Builtin,
            provider_key: existing.provider_key.clone(),
            provider_type: existing.provider_type.clone(),
            display_name: existing.display_name.clone(),
            base_url: input.base_url.unwrap_or(existing.base_url),
            api_key_encrypted: input.api_key.or(existing.api_key_encrypted),
            enabled: input.enabled.unwrap_or(existing.enabled),
            mapping_locked: true,
            custom_headers_json: input
                .custom_headers
                .map(|value| value.to_string())
                .or(existing.custom_headers_json),
            created_at: existing.created_at,
            updated_at: String::new(),
        };

        provider_repo::update(&self.pool, &updated).await?;

        if let Some(models) = input.models {
            self.sync_provider_models(&updated.id, models).await?;
        }

        let refreshed = provider_repo::find_by_id(&self.pool, &updated.id)
            .await?
            .ok_or_else(|| AppError::internal(ErrorSource::Settings, "failed to reload provider"))?;

        self.provider_settings_from_record(refreshed).await
    }

    pub async fn create_custom_provider(
        &self,
        input: CustomProviderCreateInput,
    ) -> Result<ProviderSettingsDto, AppError> {
        self.ensure_provider_state_ready().await?;
        self.validate_custom_provider_type(&input.provider_type)?;

        let id = uuid::Uuid::now_v7().to_string();
        let record = ProviderRecord {
            id: id.clone(),
            provider_kind: ProviderKind::Custom,
            provider_key: id.clone(),
            provider_type: input.provider_type,
            display_name: input.display_name,
            base_url: input.base_url,
            api_key_encrypted: input.api_key,
            enabled: input.enabled.unwrap_or(false),
            mapping_locked: false,
            custom_headers_json: input.custom_headers.map(|value| value.to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        };

        provider_repo::insert(&self.pool, &record).await?;

        if let Some(models) = input.models {
            self.sync_provider_models(&record.id, models).await?;
        }

        let refreshed = provider_repo::find_by_id(&self.pool, &record.id)
            .await?
            .ok_or_else(|| AppError::internal(ErrorSource::Settings, "failed to reload provider"))?;

        self.provider_settings_from_record(refreshed).await
    }

    pub async fn update_custom_provider(
        &self,
        id: &str,
        input: ProviderSettingsUpdateInput,
    ) -> Result<ProviderSettingsDto, AppError> {
        self.ensure_provider_state_ready().await?;

        let existing = provider_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;

        if existing.provider_kind != ProviderKind::Custom {
            return Err(AppError::recoverable(
                ErrorSource::Settings,
                "settings.provider.locked",
                "Built-in providers cannot be updated with the custom provider command",
            ));
        }

        if let Some(provider_type) = &input.provider_type {
            self.validate_custom_provider_type(provider_type)?;
        }

        let updated = ProviderRecord {
            id: existing.id.clone(),
            provider_kind: ProviderKind::Custom,
            provider_key: existing.provider_key.clone(),
            provider_type: input.provider_type.unwrap_or(existing.provider_type),
            display_name: input.display_name.unwrap_or(existing.display_name),
            base_url: input.base_url.unwrap_or(existing.base_url),
            api_key_encrypted: input.api_key.or(existing.api_key_encrypted),
            enabled: input.enabled.unwrap_or(existing.enabled),
            mapping_locked: false,
            custom_headers_json: input
                .custom_headers
                .map(|value| value.to_string())
                .or(existing.custom_headers_json),
            created_at: existing.created_at,
            updated_at: String::new(),
        };

        provider_repo::update(&self.pool, &updated).await?;

        if let Some(models) = input.models {
            self.sync_provider_models(&updated.id, models).await?;
        }

        let refreshed = provider_repo::find_by_id(&self.pool, &updated.id)
            .await?
            .ok_or_else(|| AppError::internal(ErrorSource::Settings, "failed to reload provider"))?;

        self.provider_settings_from_record(refreshed).await
    }

    pub async fn delete_custom_provider(&self, id: &str) -> Result<(), AppError> {
        self.ensure_provider_state_ready().await?;

        let existing = provider_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;

        if existing.provider_kind != ProviderKind::Custom {
            return Err(AppError::recoverable(
                ErrorSource::Settings,
                "settings.provider.delete_forbidden",
                "Built-in providers cannot be deleted",
            ));
        }

        let deleted = provider_repo::delete(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Settings, "provider"));
        }
        Ok(())
    }

    async fn ensure_provider_state_ready(&self) -> Result<(), AppError> {
        let needs_reset = match settings_repo::get(&self.pool, PROVIDER_SCHEMA_VERSION_KEY).await? {
            Some(record) => serde_json::from_str::<u32>(&record.value_json)
                .map(|version| version != PROVIDER_SCHEMA_VERSION)
                .unwrap_or(true),
            None => true,
        };

        if needs_reset {
            provider_repo::delete_all(&self.pool).await?;
            settings_repo::set(
                &self.pool,
                PROVIDER_SCHEMA_VERSION_KEY,
                &PROVIDER_SCHEMA_VERSION.to_string(),
            )
            .await?;
        }

        self.seed_builtin_providers().await
    }

    async fn seed_builtin_providers(&self) -> Result<(), AppError> {
        let registry = ModelRegistry::with_predefined();
        let builtin_keys = BUILTIN_PROVIDER_CATALOG
            .iter()
            .map(|entry| entry.provider_key)
            .collect::<std::collections::HashSet<_>>();

        for record in provider_repo::list_all(&self.pool).await? {
            if record.provider_kind == ProviderKind::Builtin
                && !builtin_keys.contains(record.provider_key.as_str())
            {
                provider_repo::delete(&self.pool, &record.id).await?;
            }
        }

        for entry in BUILTIN_PROVIDER_CATALOG {
            let existing = provider_repo::find_by_key(&self.pool, entry.provider_key).await?;

            let provider_id = if let Some(record) = existing {
                let updated = ProviderRecord {
                    id: record.id.clone(),
                    provider_kind: ProviderKind::Builtin,
                    provider_key: entry.provider_key.to_string(),
                    provider_type: entry.provider_type.to_string(),
                    display_name: entry.display_name.to_string(),
                    base_url: if record.base_url.trim().is_empty() {
                        entry.default_base_url.to_string()
                    } else {
                        record.base_url
                    },
                    api_key_encrypted: record.api_key_encrypted,
                    enabled: record.enabled,
                    mapping_locked: true,
                    custom_headers_json: record.custom_headers_json,
                    created_at: record.created_at,
                    updated_at: String::new(),
                };
                provider_repo::update(&self.pool, &updated).await?;
                updated.id
            } else {
                let provider_id = uuid::Uuid::now_v7().to_string();
                let created = ProviderRecord {
                    id: provider_id.clone(),
                    provider_kind: ProviderKind::Builtin,
                    provider_key: entry.provider_key.to_string(),
                    provider_type: entry.provider_type.to_string(),
                    display_name: entry.display_name.to_string(),
                    base_url: entry.default_base_url.to_string(),
                    api_key_encrypted: None,
                    enabled: false,
                    mapping_locked: true,
                    custom_headers_json: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                };
                provider_repo::insert(&self.pool, &created).await?;
                self.seed_builtin_models(&provider_id, entry.provider_key, &registry)
                    .await?;
                provider_id
            };

            let _ = provider_id;
        }

        Ok(())
    }

    async fn seed_builtin_models(
        &self,
        provider_id: &str,
        provider_key: &str,
        registry: &ModelRegistry,
    ) -> Result<(), AppError> {
        let provider = TiyProvider::from(provider_key.to_string());
        for model in registry.models_for_provider(&provider) {
            let record = ProviderModelRecord {
                id: uuid::Uuid::now_v7().to_string(),
                provider_id: provider_id.to_string(),
                model_name: model.id.clone(),
                display_name: Some(model.name.clone()),
                enabled: true,
                context_window: Some(model.context_window.to_string()),
                max_output_tokens: Some(model.max_tokens.to_string()),
                capabilities_json: None,
                provider_options_json: None,
                is_manual: false,
                created_at: String::new(),
            };
            provider_repo::upsert_model(&self.pool, &record).await?;
        }
        Ok(())
    }

    async fn provider_settings_from_record(
        &self,
        provider: ProviderRecord,
    ) -> Result<ProviderSettingsDto, AppError> {
        let models = provider_repo::list_models(&self.pool, &provider.id).await?;
        Ok(ProviderSettingsDto::from_record(provider, models))
    }

    async fn sync_provider_models(
        &self,
        provider_id: &str,
        models: Vec<ProviderModelInput>,
    ) -> Result<(), AppError> {
        let existing_models = provider_repo::list_models(&self.pool, provider_id).await?;
        let existing_ids = existing_models
            .iter()
            .map(|model| model.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let incoming_ids = models
            .iter()
            .filter_map(|model| model.id.clone())
            .collect::<std::collections::HashSet<_>>();

        for existing_id in existing_ids.difference(&incoming_ids) {
            provider_repo::delete_model(&self.pool, existing_id).await?;
        }

        for model in models {
            let record = ProviderModelRecord {
                id: model.id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                provider_id: provider_id.to_string(),
                model_name: model.model_id,
                display_name: model.display_name,
                enabled: model.enabled.unwrap_or(true),
                context_window: model.context_window,
                max_output_tokens: model.max_output_tokens,
                capabilities_json: model.capability_overrides.map(|value| value.to_string()),
                provider_options_json: model.provider_options.map(|value| value.to_string()),
                is_manual: model.is_manual.unwrap_or(true),
                created_at: String::new(),
            };
            provider_repo::upsert_model(&self.pool, &record).await?;
        }

        Ok(())
    }

    fn validate_custom_provider_type(&self, provider_type: &str) -> Result<(), AppError> {
        let is_supported = CUSTOM_PROVIDER_TYPE_CATALOG
            .iter()
            .any(|entry| entry.provider_type == provider_type);

        if is_supported {
            Ok(())
        } else {
            Err(AppError::recoverable(
                ErrorSource::Settings,
                "settings.provider.invalid_custom_type",
                "Unsupported custom provider type",
            ))
        }
    }

    // -----------------------------------------------------------------------
    // Agent Profiles
    // -----------------------------------------------------------------------

    pub async fn list_profiles(&self) -> Result<Vec<AgentProfileRecord>, AppError> {
        profile_repo::list_all(&self.pool).await
    }

    pub async fn create_profile(
        &self,
        input: AgentProfileInput,
    ) -> Result<AgentProfileRecord, AppError> {
        let record = AgentProfileRecord {
            id: uuid::Uuid::now_v7().to_string(),
            name: input.name,
            custom_instructions: input.custom_instructions,
            response_style: input.response_style,
            response_language: input.response_language,
            primary_provider_id: input.primary_provider_id,
            primary_model_id: input.primary_model_id,
            auxiliary_provider_id: input.auxiliary_provider_id,
            auxiliary_model_id: input.auxiliary_model_id,
            lightweight_provider_id: input.lightweight_provider_id,
            lightweight_model_id: input.lightweight_model_id,
            is_default: input.is_default.unwrap_or(false),
            created_at: String::new(),
            updated_at: String::new(),
        };

        if record.is_default {
            profile_repo::set_default(&self.pool, &record.id).await.ok();
        }

        profile_repo::insert(&self.pool, &record).await?;

        profile_repo::find_by_id(&self.pool, &record.id)
            .await?
            .ok_or_else(|| AppError::internal(ErrorSource::Settings, "failed to read back profile"))
    }

    pub async fn update_profile(
        &self,
        id: &str,
        input: AgentProfileInput,
    ) -> Result<AgentProfileRecord, AppError> {
        let existing = profile_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "agent profile"))?;

        let record = AgentProfileRecord {
            id: id.to_string(),
            name: input.name,
            custom_instructions: input.custom_instructions.or(existing.custom_instructions),
            response_style: input.response_style.or(existing.response_style),
            response_language: input.response_language.or(existing.response_language),
            primary_provider_id: input.primary_provider_id.or(existing.primary_provider_id),
            primary_model_id: input.primary_model_id.or(existing.primary_model_id),
            auxiliary_provider_id: input
                .auxiliary_provider_id
                .or(existing.auxiliary_provider_id),
            auxiliary_model_id: input.auxiliary_model_id.or(existing.auxiliary_model_id),
            lightweight_provider_id: input
                .lightweight_provider_id
                .or(existing.lightweight_provider_id),
            lightweight_model_id: input.lightweight_model_id.or(existing.lightweight_model_id),
            is_default: input.is_default.unwrap_or(existing.is_default),
            created_at: existing.created_at,
            updated_at: String::new(),
        };

        if record.is_default && !existing.is_default {
            profile_repo::set_default(&self.pool, id).await?;
        }

        profile_repo::update(&self.pool, &record).await?;

        profile_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::internal(ErrorSource::Settings, "failed to read back profile"))
    }

    pub async fn delete_profile(&self, id: &str) -> Result<(), AppError> {
        let deleted = profile_repo::delete(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Settings, "agent profile"));
        }
        Ok(())
    }
}
