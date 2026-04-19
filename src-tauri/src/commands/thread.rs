use std::time::Duration;

use tauri::State;
use tiycore::provider::get_provider;
use tiycore::types::{
    Context as TiyContext, Message as TiyMessage, StreamOptions as TiyStreamOptions, UserMessage,
};

use crate::core::agent_run_manager::{
    build_provider_options_payload_hook, normalize_generated_title, truncate_chars,
    TITLE_CONTEXT_MAX_CHARS, TITLE_GENERATION_MAX_TOKENS, TITLE_GENERATION_MAX_TOKENS_REASONING,
    TITLE_GENERATION_TIMEOUT,
};
use crate::core::agent_session::{
    normalize_profile_response_language, normalize_profile_response_style,
    resolve_runtime_model_role, ProfileResponseStyle, RuntimeModelPlan, RuntimeModelRole,
};
use crate::core::app_state::AppState;
use crate::core::tiycode_default_headers;
use crate::core::tiycode_url_policy;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{AddMessageInput, MessageDto, ThreadSnapshotDto, ThreadSummaryDto};
use crate::persistence::repo::{message_repo, profile_repo};

#[tauri::command]
pub async fn thread_list(
    state: State<'_, AppState>,
    workspace_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ThreadSummaryDto>, AppError> {
    tracing::debug!(workspace_id = %workspace_id, "⏱ [ipc] thread_list command entered");
    let t0 = std::time::Instant::now();
    let result = state
        .thread_manager
        .list(&workspace_id, limit, offset)
        .await?;
    tracing::debug!(
        elapsed_ms = t0.elapsed().as_millis(),
        count = result.len(),
        "⏱ [ipc] thread_list command done"
    );
    Ok(result)
}

#[tauri::command]
pub async fn thread_create(
    state: State<'_, AppState>,
    workspace_id: String,
    title: Option<String>,
) -> Result<ThreadSummaryDto, AppError> {
    state.thread_manager.create(&workspace_id, title).await
}

#[tauri::command]
pub async fn thread_load(
    state: State<'_, AppState>,
    id: String,
    message_cursor: Option<String>,
    message_limit: Option<i64>,
) -> Result<ThreadSnapshotDto, AppError> {
    state
        .thread_manager
        .load(&id, message_cursor, message_limit)
        .await
}

#[tauri::command]
pub async fn thread_update_title(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<(), AppError> {
    state.thread_manager.update_title(&id, &title).await
}

#[tauri::command]
pub async fn thread_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.agent_run_manager.cancel_run_if_active(&id).await?;
    state
        .agent_run_manager
        .wait_until_thread_inactive(&id, Duration::from_secs(5))
        .await?;
    state.terminal_manager.close_for_thread(&id).await?;
    state.thread_manager.delete(&id).await
}

#[tauri::command]
pub async fn thread_add_message(
    state: State<'_, AppState>,
    thread_id: String,
    input: AddMessageInput,
) -> Result<MessageDto, AppError> {
    state.thread_manager.add_message(&thread_id, input).await
}

#[tauri::command]
pub async fn thread_regenerate_title(
    state: State<'_, AppState>,
    thread_id: String,
    model_plan: serde_json::Value,
) -> Result<String, AppError> {
    let raw_plan: RuntimeModelPlan = serde_json::from_value(model_plan).unwrap_or_default();
    let selected_model = select_title_model_role(&raw_plan)?;
    let mut model_role = resolve_runtime_model_role(&state.pool, selected_model).await?;

    // Disable reasoning for lightweight title generation.
    let was_reasoning = model_role.model.reasoning;
    model_role.model.reasoning = false;

    // Load the profile to resolve language/style preferences.
    let profile = match raw_plan.profile_id {
        Some(ref profile_id) => profile_repo::find_by_id(&state.pool, profile_id).await?,
        None => None,
    };
    let response_language = profile
        .as_ref()
        .and_then(|p| normalize_profile_response_language(p.response_language.as_deref()));
    let response_style = normalize_profile_response_style(
        profile.as_ref().and_then(|p| p.response_style.as_deref()),
    );

    // Load the most recent 5 plain messages for context.
    // `list_recent` returns messages in reverse-chronological order (newest first).
    // We filter and take 5, then reverse in the prompt to show chronological order.
    let messages = message_repo::list_recent(&state.pool, &thread_id, None, 10).await?;
    let relevant: Vec<_> = messages
        .iter()
        .filter(|m| {
            m.message_type == "plain_message" && (m.role == "user" || m.role == "assistant")
        })
        .take(5)
        .collect();

    if relevant.is_empty() {
        return Err(AppError::recoverable(
            ErrorSource::Thread,
            "thread.regenerate_title.no_messages",
            "No messages available to generate a title from.",
        ));
    }

    let prompt =
        build_regenerate_title_prompt(&relevant, response_language.as_deref(), response_style);

    let provider = get_provider(&model_role.model.provider).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.title.provider_missing",
            format!(
                "Provider type '{:?}' is not registered for title generation.",
                model_role.model.provider
            ),
        )
    })?;

    let context = TiyContext {
        system_prompt: Some(
            "You write concise conversation titles. Return only the title text.".to_string(),
        ),
        messages: vec![TiyMessage::User(UserMessage::text(prompt))],
        tools: None,
    };

    let options = TiyStreamOptions {
        api_key: model_role.api_key.clone(),
        max_tokens: Some(if was_reasoning {
            TITLE_GENERATION_MAX_TOKENS_REASONING
        } else {
            TITLE_GENERATION_MAX_TOKENS
        }),
        headers: Some(tiycode_default_headers()),
        on_payload: build_provider_options_payload_hook(model_role.provider_options.clone()),
        security: Some(tiycore::types::SecurityConfig::default().with_url(tiycode_url_policy())),
        ..TiyStreamOptions::default()
    };

    let completion = provider
        .stream(&model_role.model, &context, options)
        .try_result(TITLE_GENERATION_TIMEOUT)
        .await;

    let message = match completion {
        Some(msg) => msg,
        None => {
            return Err(AppError::recoverable(
                ErrorSource::Thread,
                "thread.regenerate_title.timeout",
                "Title generation timed out or returned no result.",
            ))
        }
    };

    if message.stop_reason == tiycore::types::StopReason::Error {
        return Err(AppError::recoverable(
            ErrorSource::Thread,
            "thread.regenerate_title.model_error",
            "The model returned an error during title generation.",
        ));
    }

    let title = normalize_generated_title(&message.text_content()).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Thread,
            "thread.regenerate_title.empty",
            "The model returned an empty or unusable title.",
        )
    })?;

    Ok(title)
}

fn select_title_model_role(raw_plan: &RuntimeModelPlan) -> Result<RuntimeModelRole, AppError> {
    raw_plan
        .lightweight
        .clone()
        .or_else(|| raw_plan.auxiliary.clone())
        .or_else(|| raw_plan.primary.clone())
        .ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.title.model_missing",
                "Select an enabled lightweight, auxiliary, or primary model in the current profile before generating a title.",
            )
        })
}

fn build_regenerate_title_prompt(
    messages: &[&crate::model::thread::MessageRecord],
    response_language: Option<&str>,
    response_style: ProfileResponseStyle,
) -> String {
    let language_rule = match response_language {
        Some(language) => format!("- Write the title in {language}."),
        None => "- Match the conversation language.".to_string(),
    };
    let style_rule = match response_style {
        ProfileResponseStyle::Balanced => {
            "- Keep the title clear and natural, with enough specificity to scan quickly."
        }
        ProfileResponseStyle::Concise => {
            "- Keep the title especially terse, direct, and low-friction."
        }
        ProfileResponseStyle::Guide => {
            "- Prefer a title that signals the user's goal or decision focus clearly."
        }
    };

    let mut conversation = String::new();
    for msg in messages.iter().rev() {
        let role_label = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        let content = truncate_chars(msg.content_markdown.trim(), TITLE_CONTEXT_MAX_CHARS);
        conversation.push_str(&format!("{role_label}:\n{content}\n\n"));
    }

    format!(
        "Create a short thread title for this conversation.\n\
Rules:\n\
{language_rule}\n\
{style_rule}\n\
- Prefer concrete nouns and actions.\n\
- Max 18 Chinese characters or 7 English words.\n\
- No quotes, no markdown, no prefixes.\n\
\n\
Conversation:\n{conversation}"
    )
}
