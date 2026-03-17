use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::{
    AgentProfileInput, AgentProfileRecord, ProviderInput, ProviderModelInput, ProviderModelRecord,
    ProviderRecord,
};
use crate::model::settings::SettingRecord;
use crate::persistence::repo::{profile_repo, provider_repo, settings_repo};

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
        // Validate JSON
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
    // Providers
    // -----------------------------------------------------------------------

    pub async fn list_providers(&self) -> Result<Vec<ProviderRecord>, AppError> {
        provider_repo::list_all(&self.pool).await
    }

    pub async fn create_provider(&self, input: ProviderInput) -> Result<ProviderRecord, AppError> {
        let record = ProviderRecord {
            id: uuid::Uuid::now_v7().to_string(),
            name: input.name,
            protocol_type: input.protocol_type.unwrap_or_else(|| "openai".to_string()),
            base_url: input.base_url,
            api_key_encrypted: input.api_key, // TODO: encrypt with system keychain
            enabled: input.enabled.unwrap_or(true),
            custom_headers_json: input.custom_headers.map(|v| v.to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        };

        provider_repo::insert(&self.pool, &record).await?;
        tracing::info!(provider_id = %record.id, name = %record.name, "provider created");

        // Re-fetch to get server-set timestamps
        provider_repo::find_by_id(&self.pool, &record.id)
            .await?
            .ok_or_else(|| {
                AppError::internal(ErrorSource::Settings, "failed to read back provider")
            })
    }

    pub async fn update_provider(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderRecord, AppError> {
        let existing = provider_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;

        let record = ProviderRecord {
            id: id.to_string(),
            name: input.name,
            protocol_type: input.protocol_type.unwrap_or(existing.protocol_type),
            base_url: input.base_url,
            api_key_encrypted: input.api_key.or(existing.api_key_encrypted),
            enabled: input.enabled.unwrap_or(existing.enabled),
            custom_headers_json: input
                .custom_headers
                .map(|v| v.to_string())
                .or(existing.custom_headers_json),
            created_at: existing.created_at,
            updated_at: String::new(),
        };

        provider_repo::update(&self.pool, &record).await?;
        tracing::info!(provider_id = %id, "provider updated");

        provider_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| {
                AppError::internal(ErrorSource::Settings, "failed to read back provider")
            })
    }

    pub async fn delete_provider(&self, id: &str) -> Result<(), AppError> {
        let deleted = provider_repo::delete(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Settings, "provider"));
        }
        tracing::info!(provider_id = %id, "provider deleted");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Provider Models
    // -----------------------------------------------------------------------

    pub async fn list_models(
        &self,
        provider_id: &str,
    ) -> Result<Vec<ProviderModelRecord>, AppError> {
        provider_repo::list_models(&self.pool, provider_id).await
    }

    pub async fn add_model(
        &self,
        provider_id: &str,
        input: ProviderModelInput,
    ) -> Result<ProviderModelRecord, AppError> {
        let record = ProviderModelRecord {
            id: uuid::Uuid::now_v7().to_string(),
            provider_id: provider_id.to_string(),
            model_name: input.model_name,
            display_name: input.display_name,
            enabled: input.enabled.unwrap_or(true),
            capabilities_json: input.capabilities.map(|v| v.to_string()),
            created_at: String::new(),
        };

        provider_repo::insert_model(&self.pool, &record).await?;
        Ok(record)
    }

    pub async fn remove_model(&self, id: &str) -> Result<(), AppError> {
        let deleted = provider_repo::delete_model(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Settings, "provider model"));
        }
        Ok(())
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

        // If this profile is default, clear other defaults first
        if record.is_default {
            profile_repo::set_default(&self.pool, &record.id).await.ok();
        }

        profile_repo::insert(&self.pool, &record).await?;
        tracing::info!(profile_id = %record.id, name = %record.name, "profile created");

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

        // If setting as default, clear other defaults first
        if record.is_default && !existing.is_default {
            profile_repo::set_default(&self.pool, id).await?;
        }

        profile_repo::update(&self.pool, &record).await?;
        tracing::info!(profile_id = %id, "profile updated");

        profile_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::internal(ErrorSource::Settings, "failed to read back profile"))
    }

    pub async fn delete_profile(&self, id: &str) -> Result<(), AppError> {
        let deleted = profile_repo::delete(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Settings, "agent profile"));
        }
        tracing::info!(profile_id = %id, "profile deleted");
        Ok(())
    }
}
