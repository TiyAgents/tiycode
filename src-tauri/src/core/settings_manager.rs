use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use sqlx::SqlitePool;
use tiy_core::catalog::{
    enrich_manual_model, list_models, list_models_with_enrichment, load_catalog_metadata_store,
    refresh_catalog_snapshot, CatalogMetadataStore, CatalogRemoteConfig, EmptyCatalogMetadataStore,
    FileCatalogMetadataStore, UnifiedModelInfo,
};
use tiy_core::provider::get_provider;
use tiy_core::types::{
    Context as TiyContext, Cost as TiyCost, InputType, Message as TiyMessage, Model as TiyModel,
    Provider as TiyProvider, StopReason, StreamOptions as TiyStreamOptions, UserMessage,
};

use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::{
    AgentProfileInput, AgentProfileRecord, CustomProviderCreateInput, ProviderCatalogEntryDto,
    ProviderKind, ProviderModelConnectionTestResultDto, ProviderModelInput, ProviderModelRecord,
    ProviderRecord, ProviderSettingsDto, ProviderSettingsUpdateInput,
};
use crate::model::settings::SettingRecord;
use crate::persistence::repo::{profile_repo, provider_repo, settings_repo};

const PROVIDER_SCHEMA_VERSION_KEY: &str = "providers.schema_version";
const PROVIDER_SCHEMA_VERSION: u32 = 3;
const TIY_CATALOG_SNAPSHOT_FILE: &str = "catalog.json";
const PROVIDER_MODEL_TEST_PROMPT: &str = "Ping from Tiy Agent.";
const PROVIDER_MODEL_TEST_MAX_TOKENS: u32 = 8;
const PROVIDER_MODEL_TEST_CONTEXT_WINDOW_FALLBACK: u32 = 8_192;
const PROVIDER_MODEL_TEST_MAX_OUTPUT_TOKENS_FALLBACK: u32 = 4_096;
const PROVIDER_MODEL_TEST_TIMEOUT: Duration = Duration::from_secs(20);

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

fn catalog_snapshot_path() -> PathBuf {
    dirs::home_dir()
        .expect("cannot resolve HOME directory")
        .join(".tiy")
        .join("catalog")
        .join(TIY_CATALOG_SNAPSHOT_FILE)
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_custom_headers_map(custom_headers_json: Option<&str>) -> Option<HashMap<String, String>> {
    custom_headers_json.and_then(|json| {
        serde_json::from_str::<HashMap<String, String>>(json)
            .map_err(|error| {
                tracing::warn!(error = %error, "failed to parse provider custom headers for model catalog request");
                error
            })
            .ok()
    })
}

fn normalize_catalog_token(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace('_', "-")
        .replace(' ', "-")
}

fn catalog_capability_overrides(model: &UnifiedModelInfo) -> Option<serde_json::Value> {
    let capabilities = model
        .capabilities
        .as_ref()
        .map(|values| {
            values
                .iter()
                .map(|value| normalize_catalog_token(value))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let modalities = model
        .modalities
        .as_ref()
        .map(|values| {
            values
                .iter()
                .map(|value| normalize_catalog_token(value))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let has_capability = |candidates: &[&str]| {
        candidates
            .iter()
            .any(|candidate| capabilities.iter().any(|value| value == candidate))
    };
    let has_modality = |candidate: &str| modalities.iter().any(|value| value == candidate);

    let mut overrides = serde_json::Map::new();

    if has_modality("image") || has_capability(&["vision", "multimodal", "image-input"]) {
        overrides.insert("vision".to_string(), serde_json::Value::Bool(true));
    }

    if has_capability(&[
        "image-output",
        "image-generation",
        "images",
        "image-generation-model",
    ]) {
        overrides.insert("imageOutput".to_string(), serde_json::Value::Bool(true));
    }

    if has_capability(&[
        "tools",
        "tool-calling",
        "tool-calls",
        "function-calling",
        "functions",
    ]) {
        overrides.insert("toolCalling".to_string(), serde_json::Value::Bool(true));
    }

    if has_capability(&["reasoning", "thinking"]) {
        overrides.insert("reasoning".to_string(), serde_json::Value::Bool(true));
    }

    if has_capability(&["embedding", "embeddings"]) {
        overrides.insert("embedding".to_string(), serde_json::Value::Bool(true));
    }

    if overrides.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(overrides))
    }
}

fn serialize_optional_json(value: Option<serde_json::Value>) -> Option<String> {
    value.map(|value| value.to_string())
}

#[derive(Debug, Clone)]
struct ProviderModelConnectionTestRequest {
    model: TiyModel,
    context: TiyContext,
    options: TiyStreamOptions,
    unsupported: bool,
}

fn parse_positive_u32(value: Option<&str>, fallback: u32) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn capability_flag_enabled(capabilities_json: Option<&str>, key: &str) -> bool {
    capabilities_json
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|value| value.get(key).and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

fn infer_embedding_model(model_name: &str) -> bool {
    let normalized = model_name.trim().to_lowercase();
    normalized.contains("embedding") || normalized.contains("embeddings") || normalized.contains("embed")
}

fn is_embedding_model(model: &ProviderModelRecord) -> bool {
    capability_flag_enabled(model.capabilities_json.as_deref(), "embedding")
        || infer_embedding_model(&model.model_name)
}

fn build_provider_model_test_request(
    provider: &ProviderRecord,
    model: &ProviderModelRecord,
) -> ProviderModelConnectionTestRequest {
    if is_embedding_model(model) {
        return ProviderModelConnectionTestRequest {
            model: TiyModel::builder()
                .id(&model.model_name)
                .name(
                    model
                        .display_name
                        .as_deref()
                        .unwrap_or(model.model_name.as_str()),
                )
                .provider(TiyProvider::from(provider.provider_type.clone()))
                .context_window(PROVIDER_MODEL_TEST_CONTEXT_WINDOW_FALLBACK)
                .max_tokens(PROVIDER_MODEL_TEST_MAX_OUTPUT_TOKENS_FALLBACK)
                .input(vec![InputType::Text])
                .cost(TiyCost::default())
                .build()
                .expect("embedding placeholder test model should be valid"),
            context: TiyContext::new(),
            options: TiyStreamOptions::default(),
            unsupported: true,
        };
    }

    let provider_type = TiyProvider::from(provider.provider_type.clone());
    let model_name = model
        .display_name
        .clone()
        .unwrap_or_else(|| model.model_name.clone());
    let context_window = parse_positive_u32(
        model.context_window.as_deref(),
        PROVIDER_MODEL_TEST_CONTEXT_WINDOW_FALLBACK,
    );
    let max_output_tokens = parse_positive_u32(
        model.max_output_tokens.as_deref(),
        PROVIDER_MODEL_TEST_MAX_OUTPUT_TOKENS_FALLBACK,
    );

    let built_model = TiyModel::builder()
        .id(&model.model_name)
        .name(&model_name)
        .provider(provider_type)
        .base_url(&provider.base_url)
        .context_window(context_window)
        .max_tokens(max_output_tokens)
        .input(vec![InputType::Text])
        .cost(TiyCost::default())
        .build()
        .expect("provider model test request should always build");

    let context = TiyContext {
        system_prompt: None,
        messages: vec![TiyMessage::User(UserMessage::text(
            PROVIDER_MODEL_TEST_PROMPT,
        ))],
        tools: None,
    };

    let options = TiyStreamOptions {
        temperature: None,
        max_tokens: Some(PROVIDER_MODEL_TEST_MAX_TOKENS),
        api_key: provider.api_key_encrypted.clone(),
        base_url: normalize_optional_string(Some(provider.base_url.clone())),
        headers: parse_custom_headers_map(provider.custom_headers_json.as_deref()),
        session_id: None,
        security: None,
        on_payload: None,
        transport: None,
        max_retry_delay_ms: None,
    };

    ProviderModelConnectionTestRequest {
        model: built_model,
        context,
        options,
        unsupported: false,
    }
}

fn build_model_record_from_catalog(
    provider_id: &str,
    existing: Option<&ProviderModelRecord>,
    model: &UnifiedModelInfo,
    sort_index: i64,
) -> ProviderModelRecord {
    ProviderModelRecord {
        id: existing
            .map(|record| record.id.clone())
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
        provider_id: provider_id.to_string(),
        model_name: model.raw_id.clone(),
        sort_index,
        display_name: normalize_optional_string(model.display_name.clone()).or_else(|| {
            existing.and_then(|record| normalize_optional_string(record.display_name.clone()))
        }),
        enabled: existing.map(|record| record.enabled).unwrap_or(false),
        context_window: model
            .context_window
            .map(|value| value.to_string())
            .or_else(|| {
                existing.and_then(|record| normalize_optional_string(record.context_window.clone()))
            }),
        max_output_tokens: model
            .max_output_tokens
            .map(|value| value.to_string())
            .or_else(|| {
                existing
                    .and_then(|record| normalize_optional_string(record.max_output_tokens.clone()))
            }),
        capabilities_json: serialize_optional_json(catalog_capability_overrides(model))
            .or_else(|| existing.and_then(|record| record.capabilities_json.clone())),
        provider_options_json: existing.and_then(|record| record.provider_options_json.clone()),
        is_manual: false,
        created_at: String::new(),
    }
}

fn should_enrich_manual_model(
    existing: Option<&ProviderModelRecord>,
    model: &ProviderModelInput,
) -> bool {
    if !model.is_manual.unwrap_or(true) {
        return false;
    }

    if existing.map(|record| record.model_name.as_str()) != Some(model.model_id.as_str()) {
        return true;
    }

    normalize_optional_string(model.display_name.clone()).is_none()
        || normalize_optional_string(model.context_window.clone()).is_none()
        || normalize_optional_string(model.max_output_tokens.clone()).is_none()
        || model
            .capability_overrides
            .as_ref()
            .map(|value| value.as_object().map(|map| map.is_empty()).unwrap_or(false))
            .unwrap_or(true)
}

fn build_model_record_from_input(
    provider_id: &str,
    provider_type: &TiyProvider,
    existing: Option<&ProviderModelRecord>,
    model: ProviderModelInput,
    metadata_store: Option<&dyn CatalogMetadataStore>,
    sort_index: i64,
) -> ProviderModelRecord {
    let display_name = normalize_optional_string(model.display_name);
    let context_window = normalize_optional_string(model.context_window);
    let max_output_tokens = normalize_optional_string(model.max_output_tokens);
    let capability_overrides = model.capability_overrides.and_then(|value| {
        let is_empty = value.as_object().map(|map| map.is_empty()).unwrap_or(false);
        if is_empty {
            None
        } else {
            Some(value)
        }
    });
    let is_manual = model.is_manual.unwrap_or(true);

    let enriched = if is_manual {
        metadata_store.map(|store| {
            enrich_manual_model(
                provider_type.clone(),
                model.model_id.clone(),
                display_name.clone(),
                store,
            )
        })
    } else {
        None
    };

    ProviderModelRecord {
        id: model.id.unwrap_or_else(|| {
            existing
                .map(|record| record.id.clone())
                .unwrap_or_else(|| uuid::Uuid::now_v7().to_string())
        }),
        provider_id: provider_id.to_string(),
        model_name: model.model_id,
        sort_index: existing
            .map(|record| record.sort_index)
            .unwrap_or(sort_index),
        display_name: display_name.or_else(|| {
            enriched
                .as_ref()
                .and_then(|value| normalize_optional_string(value.display_name.clone()))
        }),
        enabled: model.enabled.unwrap_or(true),
        context_window: context_window.or_else(|| {
            enriched
                .as_ref()
                .and_then(|value| value.context_window.map(|number| number.to_string()))
        }),
        max_output_tokens: max_output_tokens.or_else(|| {
            enriched
                .as_ref()
                .and_then(|value| value.max_output_tokens.map(|number| number.to_string()))
        }),
        capabilities_json: serialize_optional_json(capability_overrides).or_else(|| {
            enriched
                .as_ref()
                .and_then(catalog_capability_overrides)
                .map(|value| value.to_string())
        }),
        provider_options_json: model.provider_options.map(|value| value.to_string()),
        is_manual,
        created_at: String::new(),
    }
}

pub struct SettingsManager {
    pool: SqlitePool,
}

impl SettingsManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn refresh_catalog_snapshot_silently() {
        let snapshot_path = catalog_snapshot_path();
        match refresh_catalog_snapshot(&snapshot_path, &CatalogRemoteConfig::default()).await {
            Ok(result) => {
                tracing::info!(path = %snapshot_path.display(), ?result, "catalog snapshot refresh completed");
            }
            Err(error) => {
                tracing::warn!(path = %snapshot_path.display(), error = %error, "catalog snapshot refresh failed");
            }
        }
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
        let builtin_entries =
            BUILTIN_PROVIDER_CATALOG
                .iter()
                .map(|entry| ProviderCatalogEntryDto {
                    provider_key: entry.provider_key.to_string(),
                    provider_type: entry.provider_type.to_string(),
                    display_name: entry.display_name.to_string(),
                    builtin: true,
                    supports_custom: false,
                    default_base_url: entry.default_base_url.to_string(),
                });

        let custom_entries =
            CUSTOM_PROVIDER_TYPE_CATALOG
                .iter()
                .map(|entry| ProviderCatalogEntryDto {
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

    pub async fn fetch_provider_models(&self, id: &str) -> Result<ProviderSettingsDto, AppError> {
        self.ensure_provider_state_ready().await?;

        let provider = provider_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;
        let provider_type = TiyProvider::from(provider.provider_type.clone());
        let request = tiy_core::catalog::FetchModelsRequest {
            provider: provider_type,
            api_key: provider.api_key_encrypted.clone(),
            base_url: Some(provider.base_url.clone()),
            headers: parse_custom_headers_map(provider.custom_headers_json.as_deref()),
        };
        let store = self.load_catalog_store_best_effort(true).await;
        let list_result = if let Some(store) = store.as_ref() {
            list_models_with_enrichment(request, store).await
        } else {
            list_models(request).await
        }
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.provider.fetch_models_failed",
                format!("Failed to fetch provider models: {error}"),
            )
        })?;

        let existing_models = provider_repo::list_models(&self.pool, &provider.id).await?;
        self.merge_fetched_provider_models(&provider.id, &existing_models, list_result.models)
            .await?;

        let refreshed = provider_repo::find_by_id(&self.pool, &provider.id)
            .await?
            .ok_or_else(|| {
                AppError::internal(ErrorSource::Settings, "failed to reload provider")
            })?;

        self.provider_settings_from_record(refreshed).await
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
            .ok_or_else(|| {
                AppError::internal(ErrorSource::Settings, "failed to reload provider")
            })?;

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
            .ok_or_else(|| {
                AppError::internal(ErrorSource::Settings, "failed to reload provider")
            })?;

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
            .ok_or_else(|| {
                AppError::internal(ErrorSource::Settings, "failed to reload provider")
            })?;

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

    pub async fn test_provider_model_connection(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<ProviderModelConnectionTestResultDto, AppError> {
        self.ensure_provider_state_ready().await?;

        let provider = provider_repo::find_by_id(&self.pool, provider_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;
        let model = provider_repo::find_model_by_id(&self.pool, model_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider model"))?;

        if model.provider_id != provider.id {
            return Err(AppError::not_found(ErrorSource::Settings, "provider model"));
        }

        let request = build_provider_model_test_request(&provider, &model);
        if request.unsupported {
            return Ok(ProviderModelConnectionTestResultDto {
                success: false,
                unsupported: true,
                message: "Embedding model test is not supported yet.".to_string(),
                detail: None,
            });
        }

        let provider_impl = get_provider(&request.model.provider).ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.provider.test_connection_provider_missing",
                format!(
                    "Provider type '{}' is not registered in tiy-core.",
                    provider.provider_type
                ),
            )
        })?;

        let stream = provider_impl.stream(&request.model, &request.context, request.options);
        let completion = stream.try_result(PROVIDER_MODEL_TEST_TIMEOUT).await;

        let result = match completion {
            Some(message) if message.stop_reason == StopReason::Error => {
                let detail = message.error_message.clone().or_else(|| {
                    let text = message.text_content();
                    if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                });
                ProviderModelConnectionTestResultDto {
                    success: false,
                    unsupported: false,
                    message: "Connection test failed.".to_string(),
                    detail,
                }
            }
            Some(message) => {
                let text = message.text_content();
                ProviderModelConnectionTestResultDto {
                    success: true,
                    unsupported: false,
                    message: "Connection test succeeded.".to_string(),
                    detail: if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    },
                }
            }
            None => ProviderModelConnectionTestResultDto {
                success: false,
                unsupported: false,
                message: "Connection test failed.".to_string(),
                detail: Some("The provider did not finish responding before the timeout.".to_string()),
            },
        };

        Ok(result)
    }

    async fn load_catalog_store_best_effort(
        &self,
        refresh: bool,
    ) -> Option<FileCatalogMetadataStore> {
        let snapshot_path = catalog_snapshot_path();
        let existing_store = match load_catalog_metadata_store(&snapshot_path) {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(path = %snapshot_path.display(), error = %error, "failed to load catalog snapshot");
                None
            }
        };

        if !refresh {
            return existing_store;
        }

        match refresh_catalog_snapshot(&snapshot_path, &CatalogRemoteConfig::default()).await {
            Ok(_) => match load_catalog_metadata_store(&snapshot_path) {
                Ok(store) => store.or(existing_store),
                Err(error) => {
                    tracing::warn!(path = %snapshot_path.display(), error = %error, "failed to reload catalog snapshot after refresh");
                    existing_store
                }
            },
            Err(error) => {
                tracing::warn!(path = %snapshot_path.display(), error = %error, "catalog snapshot refresh failed");
                existing_store
            }
        }
    }

    async fn merge_fetched_provider_models(
        &self,
        provider_id: &str,
        existing_models: &[ProviderModelRecord],
        fetched_models: Vec<UnifiedModelInfo>,
    ) -> Result<(), AppError> {
        let existing_by_model_name = existing_models
            .iter()
            .map(|record| (record.model_name.clone(), record))
            .collect::<HashMap<_, _>>();
        let fetched_ids = fetched_models
            .iter()
            .map(|model| model.raw_id.clone())
            .collect::<HashSet<_>>();

        for (sort_index, model) in fetched_models.into_iter().enumerate() {
            let record = build_model_record_from_catalog(
                provider_id,
                existing_by_model_name.get(&model.raw_id).copied(),
                &model,
                sort_index as i64,
            );
            provider_repo::upsert_model(&self.pool, &record).await?;
        }

        for existing in existing_models {
            if !existing.is_manual && !fetched_ids.contains(&existing.model_name) {
                provider_repo::delete_model(&self.pool, &existing.id).await?;
            }
        }

        Ok(())
    }

    async fn ensure_provider_state_ready(&self) -> Result<(), AppError> {
        let current_version =
            match settings_repo::get(&self.pool, PROVIDER_SCHEMA_VERSION_KEY).await? {
                Some(record) => serde_json::from_str::<u32>(&record.value_json).unwrap_or(0),
                None => 0,
            };

        if current_version < 2 {
            provider_repo::delete_all(&self.pool).await?;
        }

        if current_version < 3 {
            sqlx::query("DELETE FROM provider_models WHERE is_manual = 0")
                .execute(&self.pool)
                .await?;
        }

        if current_version != PROVIDER_SCHEMA_VERSION {
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
        let builtin_keys = BUILTIN_PROVIDER_CATALOG
            .iter()
            .map(|entry| entry.provider_key)
            .collect::<HashSet<_>>();

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
                provider_id
            };

            let _ = provider_id;
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
        let existing_by_id = existing_models
            .iter()
            .map(|model| (model.id.clone(), model))
            .collect::<HashMap<_, _>>();
        let existing_ids = existing_models
            .iter()
            .map(|model| model.id.clone())
            .collect::<HashSet<_>>();
        let incoming_ids = models
            .iter()
            .filter_map(|model| model.id.clone())
            .collect::<HashSet<_>>();

        for existing_id in existing_ids.difference(&incoming_ids) {
            provider_repo::delete_model(&self.pool, existing_id).await?;
        }

        let requires_manual_enrichment = models.iter().any(|model| {
            let existing = model
                .id
                .as_ref()
                .and_then(|id| existing_by_id.get(id).copied());
            should_enrich_manual_model(existing, model)
        });
        let catalog_store = if requires_manual_enrichment {
            let existing_store = self.load_catalog_store_best_effort(false).await;
            if existing_store.is_some() {
                existing_store
            } else {
                self.load_catalog_store_best_effort(true).await
            }
        } else {
            None
        };
        let empty_store = EmptyCatalogMetadataStore;
        let metadata_store = catalog_store
            .as_ref()
            .map(|store| store as &dyn CatalogMetadataStore)
            .unwrap_or(&empty_store);

        let provider_record = provider_repo::find_by_id(&self.pool, provider_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;
        let provider_type = TiyProvider::from(provider_record.provider_type);
        let mut next_manual_sort_index = existing_models
            .iter()
            .map(|model| model.sort_index)
            .max()
            .unwrap_or(-1)
            + 1;

        for model in models {
            let existing = model
                .id
                .as_ref()
                .and_then(|id| existing_by_id.get(id).copied());
            let sort_index = existing.map(|record| record.sort_index).unwrap_or_else(|| {
                let value = next_manual_sort_index;
                next_manual_sort_index += 1;
                value
            });
            let record = if should_enrich_manual_model(existing, &model) {
                build_model_record_from_input(
                    provider_id,
                    &provider_type,
                    existing,
                    model,
                    Some(metadata_store),
                    sort_index,
                )
            } else {
                build_model_record_from_input(
                    provider_id,
                    &provider_type,
                    existing,
                    model,
                    None,
                    sort_index,
                )
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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tiy_core::catalog::{CatalogModelMetadata, InMemoryCatalogMetadataStore};

    use super::*;

    fn sample_store() -> InMemoryCatalogMetadataStore {
        InMemoryCatalogMetadataStore::new(vec![CatalogModelMetadata {
            canonical_model_key: "openai:gpt-4.1".to_string(),
            aliases: vec!["openai/gpt-4.1".to_string()],
            display_name: Some("GPT-4.1".to_string()),
            description: Some("General-purpose flagship".to_string()),
            context_window: Some(1_000_000),
            max_output_tokens: Some(32_768),
            max_input_tokens: Some(1_000_000),
            modalities: Some(vec!["text".to_string(), "image".to_string()]),
            capabilities: Some(vec!["tools".to_string(), "reasoning".to_string()]),
            pricing: Some(json!({"input": "2.0", "output": "8.0"})),
            source: "openrouter".to_string(),
            raw: json!({}),
        }])
    }

    #[test]
    fn manual_model_enrichment_fills_missing_fields() {
        let store = sample_store();
        let record = build_model_record_from_input(
            "provider-1",
            &TiyProvider::OpenAI,
            None,
            ProviderModelInput {
                id: None,
                model_id: "openai/gpt-4.1".to_string(),
                display_name: None,
                enabled: Some(true),
                context_window: None,
                max_output_tokens: None,
                capability_overrides: None,
                provider_options: None,
                is_manual: Some(true),
            },
            Some(&store),
            7,
        );

        assert_eq!(record.display_name.as_deref(), Some("GPT-4.1"));
        assert_eq!(record.context_window.as_deref(), Some("1000000"));
        assert_eq!(record.max_output_tokens.as_deref(), Some("32768"));
        assert_eq!(
            record.capabilities_json.as_deref(),
            Some(r#"{"reasoning":true,"toolCalling":true,"vision":true}"#),
        );
    }

    #[test]
    fn manual_model_enrichment_preserves_user_values() {
        let store = sample_store();
        let record = build_model_record_from_input(
            "provider-1",
            &TiyProvider::OpenAI,
            None,
            ProviderModelInput {
                id: None,
                model_id: "openai/gpt-4.1".to_string(),
                display_name: Some("My GPT".to_string()),
                enabled: Some(true),
                context_window: Some("2048".to_string()),
                max_output_tokens: Some("1024".to_string()),
                capability_overrides: Some(json!({ "embedding": true })),
                provider_options: Some(json!({ "tier": "manual" })),
                is_manual: Some(true),
            },
            Some(&store),
            8,
        );

        assert_eq!(record.display_name.as_deref(), Some("My GPT"));
        assert_eq!(record.context_window.as_deref(), Some("2048"));
        assert_eq!(record.max_output_tokens.as_deref(), Some("1024"));
        assert_eq!(
            record.capabilities_json.as_deref(),
            Some(r#"{"embedding":true}"#)
        );
        assert_eq!(
            record.provider_options_json.as_deref(),
            Some(r#"{"tier":"manual"}"#),
        );
    }

    #[test]
    fn fetched_model_merge_preserves_existing_state() {
        let existing = ProviderModelRecord {
            id: "model-1".to_string(),
            provider_id: "provider-1".to_string(),
            model_name: "gpt-4.1".to_string(),
            sort_index: 4,
            display_name: Some("Old Name".to_string()),
            enabled: false,
            context_window: Some("8192".to_string()),
            max_output_tokens: Some("4096".to_string()),
            capabilities_json: Some(r#"{"toolCalling":true}"#.to_string()),
            provider_options_json: Some(r#"{"tier":"existing"}"#.to_string()),
            is_manual: true,
            created_at: String::new(),
        };
        let fetched = UnifiedModelInfo {
            provider: TiyProvider::OpenAI,
            raw_id: "gpt-4.1".to_string(),
            canonical_model_key: Some("openai:gpt-4.1".to_string()),
            display_name: Some("GPT-4.1".to_string()),
            description: None,
            context_window: Some(1_000_000),
            max_output_tokens: Some(32_768),
            max_input_tokens: None,
            created_at: None,
            modalities: Some(vec!["text".to_string(), "image".to_string()]),
            capabilities: Some(vec!["tools".to_string(), "reasoning".to_string()]),
            pricing: None,
            match_confidence: Some(1.0),
            metadata_sources: vec!["openrouter".to_string()],
            raw: json!({}),
        };

        let record = build_model_record_from_catalog("provider-1", Some(&existing), &fetched, 0);

        assert_eq!(record.id, "model-1");
        assert!(!record.enabled);
        assert_eq!(record.provider_options_json, existing.provider_options_json);
        assert!(!record.is_manual);
        assert_eq!(record.sort_index, 0);
        assert_eq!(record.display_name.as_deref(), Some("GPT-4.1"));
        assert_eq!(record.context_window.as_deref(), Some("1000000"));
    }

    #[test]
    fn fetched_model_defaults_to_disabled_for_new_entries() {
        let fetched = UnifiedModelInfo {
            provider: TiyProvider::OpenAI,
            raw_id: "gpt-4.1".to_string(),
            canonical_model_key: Some("openai:gpt-4.1".to_string()),
            display_name: Some("GPT-4.1".to_string()),
            description: None,
            context_window: Some(1_000_000),
            max_output_tokens: Some(32_768),
            max_input_tokens: None,
            created_at: None,
            modalities: Some(vec!["text".to_string(), "image".to_string()]),
            capabilities: Some(vec!["tools".to_string(), "reasoning".to_string()]),
            pricing: None,
            match_confidence: Some(1.0),
            metadata_sources: vec!["openrouter".to_string()],
            raw: json!({}),
        };

        let record = build_model_record_from_catalog("provider-1", None, &fetched, 0);

        assert!(!record.enabled);
        assert!(!record.is_manual);
        assert_eq!(record.sort_index, 0);
    }

    #[test]
    fn embedding_detection_supports_capabilities_and_name_fallback() {
        let capability_model = ProviderModelRecord {
            id: "model-embedding".to_string(),
            provider_id: "provider-1".to_string(),
            model_name: "custom-model".to_string(),
            sort_index: 0,
            display_name: None,
            enabled: true,
            context_window: None,
            max_output_tokens: None,
            capabilities_json: Some(r#"{"embedding":true}"#.to_string()),
            provider_options_json: None,
            is_manual: true,
            created_at: String::new(),
        };
        let inferred_model = ProviderModelRecord {
            id: "model-embedding-2".to_string(),
            provider_id: "provider-1".to_string(),
            model_name: "text-embedding-3-small".to_string(),
            sort_index: 1,
            display_name: None,
            enabled: true,
            context_window: None,
            max_output_tokens: None,
            capabilities_json: None,
            provider_options_json: None,
            is_manual: true,
            created_at: String::new(),
        };

        assert!(is_embedding_model(&capability_model));
        assert!(is_embedding_model(&inferred_model));
    }

    #[test]
    fn provider_model_test_request_returns_unsupported_for_embedding_models() {
        let provider = ProviderRecord {
            id: "provider-1".to_string(),
            provider_kind: ProviderKind::Custom,
            provider_key: "provider-1".to_string(),
            provider_type: "openai-compatible".to_string(),
            display_name: "My Gateway".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key_encrypted: Some("sk-test".to_string()),
            enabled: true,
            mapping_locked: false,
            custom_headers_json: None,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let model = ProviderModelRecord {
            id: "model-1".to_string(),
            provider_id: "provider-1".to_string(),
            model_name: "text-embedding-3-small".to_string(),
            sort_index: 0,
            display_name: Some("Text Embedding".to_string()),
            enabled: true,
            context_window: None,
            max_output_tokens: None,
            capabilities_json: Some(r#"{"embedding":true}"#.to_string()),
            provider_options_json: None,
            is_manual: true,
            created_at: String::new(),
        };

        let request = build_provider_model_test_request(&provider, &model);

        assert!(request.unsupported);
    }

    #[test]
    fn provider_model_test_request_uses_ping_prompt_and_max_tokens_limit() {
        let provider = ProviderRecord {
            id: "provider-1".to_string(),
            provider_kind: ProviderKind::Builtin,
            provider_key: "openai".to_string(),
            provider_type: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key_encrypted: Some("sk-test".to_string()),
            enabled: true,
            mapping_locked: true,
            custom_headers_json: Some(r#"{"X-Test":"1"}"#.to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let model = ProviderModelRecord {
            id: "model-1".to_string(),
            provider_id: "provider-1".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            sort_index: 0,
            display_name: Some("GPT-4o Mini".to_string()),
            enabled: true,
            context_window: Some("128000".to_string()),
            max_output_tokens: Some("16384".to_string()),
            capabilities_json: Some(r#"{"toolCalling":true}"#.to_string()),
            provider_options_json: None,
            is_manual: false,
            created_at: String::new(),
        };

        let request = build_provider_model_test_request(&provider, &model);

        assert!(!request.unsupported);
        assert_eq!(request.options.max_tokens, Some(PROVIDER_MODEL_TEST_MAX_TOKENS));
        assert_eq!(request.model.max_tokens, 16_384);
        assert_eq!(request.model.context_window, 128_000);
        assert_eq!(request.options.base_url.as_deref(), Some("https://api.openai.com/v1"));
        assert_eq!(
            request
                .options
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-Test"))
                .map(String::as_str),
            Some("1")
        );

        let prompt = match &request.context.messages[0] {
            TiyMessage::User(message) => match &message.content {
                tiy_core::types::UserContent::Text(text) => text.as_str(),
                _ => panic!("expected text user message"),
            },
            _ => panic!("expected user message"),
        };

        assert_eq!(prompt, PROVIDER_MODEL_TEST_PROMPT);
    }
}
