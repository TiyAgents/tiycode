//! Builds a `RuntimeModelPlan` from an `AgentProfileRecord` stored in the database.
//!
//! This mirrors the frontend logic in `src/modules/settings-center/model/run-model-plan.ts`
//! so that ACP (headless) sessions can construct a valid model plan without relying on the
//! frontend to supply one.

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::core::agent_session_types::RuntimeModelRole;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::{ProviderModelRecord, ProviderRecord};
use crate::persistence::repo::{profile_repo, provider_repo, settings_repo};

/// Resolve the currently active profile ID.
///
/// Priority:
/// 1. `active_profile_id` from settings KV table
/// 2. The first profile with `is_default = true` (list_all sorts by is_default DESC)
/// 3. The first profile in the list (alphabetical fallback)
pub async fn resolve_active_profile_id(pool: &SqlitePool) -> Result<Option<String>, AppError> {
    // Try the settings KV store first
    if let Some(record) = settings_repo::get(pool, "active_profile_id").await? {
        // value_json is stored as a JSON string like `"profile-id-here"`
        let id: String = serde_json::from_str(&record.value_json).unwrap_or_default();
        if !id.is_empty() {
            return Ok(Some(id));
        }
    }

    // Fallback: pick the default (or first) profile
    let profiles = profile_repo::list_all(pool).await?;
    Ok(profiles.into_iter().next().map(|p| p.id))
}

/// Build a complete `RuntimeModelPlan` JSON value from a profile ID.
///
/// Returns `Err` if:
/// - The profile does not exist
/// - The profile's primary provider/model is missing or disabled
pub async fn build_model_plan_from_profile(
    pool: &SqlitePool,
    profile_id: &str,
) -> Result<serde_json::Value, AppError> {
    let profile = profile_repo::find_by_id(pool, profile_id)
        .await?
        .ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Settings,
                "acp.profile_not_found",
                format!("Agent profile '{profile_id}' not found"),
            )
        })?;

    // Resolve primary (required)
    let primary_role = resolve_role_from_profile_fields(
        pool,
        profile.primary_provider_id.as_deref(),
        profile.primary_model_id.as_deref(),
    )
    .await?
    .ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "acp.primary_model_unavailable",
            "Profile's primary model is not configured or its provider/model is disabled",
        )
    })?;

    // Resolve auxiliary (optional)
    let auxiliary_role = resolve_role_from_profile_fields(
        pool,
        profile.auxiliary_provider_id.as_deref(),
        profile.auxiliary_model_id.as_deref(),
    )
    .await?;

    // Resolve lightweight with fallback chain: explicit → auxiliary → primary
    let lightweight_role = {
        let explicit = resolve_role_from_profile_fields(
            pool,
            profile.lightweight_provider_id.as_deref(),
            profile.lightweight_model_id.as_deref(),
        )
        .await?;
        explicit
            .or_else(|| auxiliary_role.clone())
            .or_else(|| Some(primary_role.clone()))
    };

    // Build the plan JSON
    let thinking_level = profile
        .thinking_level
        .as_deref()
        .filter(|t| !t.is_empty() && *t != "off")
        .map(|t| t.to_string());

    let plan = serde_json::json!({
        "profileId": profile.id,
        "profileName": profile.name,
        "customInstructions": profile.custom_instructions.as_deref().filter(|s| !s.is_empty()),
        "responseStyle": profile.response_style.as_deref().filter(|s| !s.is_empty()),
        "responseLanguage": profile.response_language.as_deref().filter(|s| !s.is_empty()),
        "thinkingLevel": thinking_level,
        "primary": role_to_json(&primary_role),
        "auxiliary": auxiliary_role.as_ref().map(role_to_json),
        "lightweight": lightweight_role.as_ref().map(role_to_json),
        "toolProfileByMode": { "default": "default_full", "plan": "plan_read_only" },
    });

    Ok(plan)
}

/// Try to resolve a single model role from provider_id + model_id.
/// Returns `None` if either field is empty/None, or if the provider/model is not found or disabled.
async fn resolve_role_from_profile_fields(
    pool: &SqlitePool,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<Option<RuntimeModelRole>, AppError> {
    let provider_id = match provider_id {
        Some(id) if !id.is_empty() => id,
        _ => return Ok(None),
    };
    let model_id = match model_id {
        Some(id) if !id.is_empty() => id,
        _ => return Ok(None),
    };

    let provider = match provider_repo::find_by_id(pool, provider_id).await? {
        Some(p) if p.enabled => p,
        _ => return Ok(None),
    };

    let model = match provider_repo::find_model_by_id(pool, model_id).await? {
        Some(m) if m.enabled && m.provider_id == provider_id => m,
        _ => return Ok(None),
    };

    Ok(Some(build_runtime_model_role(&provider, &model)))
}

/// Construct a `RuntimeModelRole` from provider + model records.
fn build_runtime_model_role(
    provider: &ProviderRecord,
    model: &ProviderModelRecord,
) -> RuntimeModelRole {
    // Parse capabilities from JSON
    let (supports_image_input, supports_reasoning, reasoning_content_constrained) =
        parse_capabilities(model);

    // Parse custom_headers
    let custom_headers: Option<HashMap<String, String>> = provider
        .custom_headers_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .filter(|h: &HashMap<String, String>| !h.is_empty());

    // Parse provider_options
    let provider_options: Option<serde_json::Value> = model
        .provider_options_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .filter(|v: &serde_json::Value| {
            !v.is_null() && v.as_object().map_or(true, |o| !o.is_empty())
        });

    RuntimeModelRole {
        provider_id: provider.id.clone(),
        model_record_id: model.id.clone(),
        provider: Some(provider.provider_type.clone()),
        provider_key: Some(provider.provider_key.clone()),
        provider_type: provider.provider_type.clone(),
        provider_name: Some(provider.display_name.clone()),
        model: model.model_name.clone(),
        model_id: model.model_name.clone(),
        model_display_name: model
            .display_name
            .clone()
            .or_else(|| Some(model.model_name.clone())),
        base_url: provider.base_url.clone(),
        context_window: model.context_window.clone(),
        max_output_tokens: model.max_output_tokens.clone(),
        supports_image_input,
        supports_reasoning,
        reasoning_content_constrained,
        custom_headers,
        provider_options,
    }
}

/// Parse capabilities from the model's `capabilities_json` field.
/// Returns (supports_image_input, supports_reasoning, reasoning_content_constrained).
fn parse_capabilities(model: &ProviderModelRecord) -> (Option<bool>, Option<bool>, Option<bool>) {
    let Some(json_str) = model.capabilities_json.as_deref() else {
        return (None, None, None);
    };
    let Ok(caps) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return (None, None, None);
    };

    let vision = caps.get("vision").and_then(|v| v.as_bool());
    let reasoning = caps.get("reasoning").and_then(|v| v.as_bool());
    let reasoning_constrained = caps
        .get("reasoningContentConstrained")
        .and_then(|v| v.as_bool());

    (vision, reasoning, reasoning_constrained)
}

/// Serialize a `RuntimeModelRole` into a JSON value matching the camelCase DTO format
/// expected by `build_session_spec`.
fn role_to_json(role: &RuntimeModelRole) -> serde_json::Value {
    serde_json::json!({
        "providerId": role.provider_id,
        "modelRecordId": role.model_record_id,
        "provider": role.provider,
        "providerKey": role.provider_key,
        "providerType": role.provider_type,
        "providerName": role.provider_name,
        "model": role.model,
        "modelId": role.model_id,
        "modelDisplayName": role.model_display_name,
        "baseUrl": role.base_url,
        "contextWindow": role.context_window,
        "maxOutputTokens": role.max_output_tokens,
        "supportsImageInput": role.supports_image_input,
        "supportsReasoning": role.supports_reasoning,
        "reasoningContentConstrained": role.reasoning_content_constrained,
        "customHeaders": role.custom_headers,
        "providerOptions": role.provider_options,
    })
}
