use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tiycore::provider::get_provider;
use tiycore::types::{
    Context as TiyContext, Message as TiyMessage, StopReason, StreamOptions as TiyStreamOptions,
    UserMessage,
};
use tokio::sync::broadcast;

use crate::core::agent_session::{
    normalize_profile_response_language, normalize_profile_response_style, ProfileResponseStyle,
    ResolvedModelRole,
};
use crate::core::tiycode_default_headers;
use crate::core::tiycode_url_policy;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::MessageRecord;
use crate::persistence::repo::{message_repo, profile_repo, thread_repo};

use super::agent_run_manager::{
    build_provider_options_payload_hook, truncate_chars, TITLE_CONTEXT_MAX_CHARS,
    TITLE_GENERATION_MAX_TOKENS, TITLE_GENERATION_MAX_TOKENS_REASONING, TITLE_GENERATION_TIMEOUT,
};
use crate::ipc::app_events;
use crate::ipc::app_events::ThreadTitleUpdatedPayload;

pub(crate) fn build_title_model_candidates(
    lightweight: Option<&ResolvedModelRole>,
    auxiliary: Option<&ResolvedModelRole>,
    primary: Option<&ResolvedModelRole>,
) -> Vec<ResolvedModelRole> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for candidate in [lightweight, auxiliary, primary].into_iter().flatten() {
        if seen.insert(candidate.model_id.clone()) {
            result.push(candidate.clone());
        }
    }
    result
}

pub(crate) async fn maybe_generate_thread_title(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    profile_id: Option<String>,
    candidates: &[ResolvedModelRole],
    frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    app_handle: AppHandle,
) -> Result<(), AppError> {
    if thread_repo::has_title(pool, thread_id).await? {
        tracing::debug!(
            run_id = %run_id,
            thread_id = %thread_id,
            "skipping thread title generation: thread already has a title"
        );
        return Ok(());
    }

    let context_messages = load_title_context_messages(pool, thread_id).await?;
    if context_messages.is_empty() {
        tracing::debug!(
            run_id = %run_id,
            thread_id = %thread_id,
            "skipping thread title generation: no user/assistant messages in current context"
        );
        return Ok(());
    }

    let profile = match profile_id {
        Some(profile_id) => profile_repo::find_by_id(pool, &profile_id).await?,
        None => None,
    };
    let response_language = profile.as_ref().and_then(|profile| {
        normalize_profile_response_language(profile.response_language.as_deref())
    });
    let response_style = normalize_profile_response_style(
        profile
            .as_ref()
            .and_then(|profile| profile.response_style.as_deref()),
    );

    let mut last_error: Option<AppError> = None;
    for model_role in candidates {
        match generate_thread_title(
            model_role,
            &context_messages,
            response_language.as_deref(),
            response_style,
        )
        .await
        {
            Ok(Some(title)) => {
                thread_repo::update_title(pool, thread_id, &title).await?;

                tracing::info!(
                    run_id = %run_id,
                    thread_id = %thread_id,
                    title = %title,
                    "generated thread title, sending to frontend"
                );

                // Broadcast a global event so the sidebar can pick up the new title even
                // when no per-run stream subscription exists (e.g. inactive threads).
                let _ = app_handle.emit(
                    app_events::THREAD_TITLE_UPDATED,
                    ThreadTitleUpdatedPayload {
                        thread_id: thread_id.to_string(),
                        title: title.clone(),
                    },
                );

                if frontend_tx
                    .send(ThreadStreamEvent::ThreadTitleUpdated {
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        title,
                    })
                    .is_err()
                {
                    tracing::warn!(
                        run_id = %run_id,
                        thread_id = %thread_id,
                        "failed to send ThreadTitleUpdated event: frontend channel closed"
                    );
                }

                return Ok(());
            }
            Ok(None) => {
                tracing::warn!(
                    run_id = %run_id,
                    thread_id = %thread_id,
                    model_id = %model_role.model_id,
                    "title generation returned empty result (timeout or empty response)"
                );
                last_error = Some(AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.regenerate_title.empty",
                    "Title generation returned empty or timed out.",
                ));
            }
            Err(e) => {
                tracing::warn!(
                    run_id = %run_id,
                    thread_id = %thread_id,
                    model_id = %model_role.model_id,
                    error = %e,
                    "title generation failed"
                );
                last_error = Some(e);
            }
        }
    }

    if let Some(e) = last_error {
        return Err(e);
    }

    Ok(())
}

pub(crate) async fn load_title_context_messages(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Vec<MessageRecord>, AppError> {
    let messages = message_repo::list_since_last_reset(pool, thread_id).await?;
    let filtered: Vec<MessageRecord> = messages
        .into_iter()
        .filter(|m| {
            m.message_type == "plain_message" && (m.role == "user" || m.role == "assistant")
        })
        .collect();
    Ok(filtered)
}

pub(crate) async fn generate_thread_title(
    model_role: &ResolvedModelRole,
    messages: &[MessageRecord],
    response_language: Option<&str>,
    response_style: ProfileResponseStyle,
) -> Result<Option<String>, AppError> {
    // Lightweight title generation does not benefit from reasoning/thinking tokens.
    // When the lightweight model is a reasoning-capable model (e.g. DeepSeek R1, o1),
    // the reasoning tokens count against `max_tokens` and can exhaust the entire
    // token budget (TITLE_GENERATION_MAX_TOKENS = 512), leaving no room for the
    // actual title output.
    //
    // Strategy: 1) Explicitly disable reasoning so the protocol layer omits
    // thinking/reasoning parameters from the API request.  2) If the original
    // model had reasoning enabled, bump max_tokens as a fallback — some
    // reasoning-only models (e.g. o1) ignore the disable and still produce
    // reasoning tokens, so the larger budget ensures the title can still be
    // returned.
    let was_reasoning = model_role.model.reasoning;
    let mut model_role = model_role.clone();
    model_role.model.reasoning = false;

    let provider = get_provider(&model_role.model.provider).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.title_generation.provider_missing",
            format!(
                "Provider type '{:?}' is not registered for lightweight title generation.",
                model_role.model.provider
            ),
        )
    })?;

    let prompt = build_title_prompt_from_messages(messages, response_language, response_style);
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
        Some(message) => message,
        None => return Ok(None),
    };

    if message.stop_reason == StopReason::Error {
        let detail = message
            .error_message
            .clone()
            .unwrap_or_else(|| "lightweight title generation failed".to_string());
        return Err(AppError::recoverable(
            ErrorSource::Settings,
            "settings.title_generation.failed",
            detail,
        ));
    }

    Ok(normalize_generated_title(&message.text_content()))
}

pub(crate) fn build_title_prompt_from_messages(
    messages: &[MessageRecord],
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
    // Messages are in chronological order (oldest first); iterate in reverse
    // so the newest messages appear first in the prompt.
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

#[cfg(test)]
pub(crate) fn build_title_prompt(
    user_message: &str,
    assistant_message: &str,
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

    format!(
        "Create a short thread title for this conversation.\n\
Rules:\n\
- {language_rule}\n\
- {style_rule}\n\
- Prefer concrete nouns and actions.\n\
- Max 18 Chinese characters or 7 English words.\n\
- No quotes, no markdown, no prefixes.\n\
\n\
User message:\n{user_message}\n\
\n\
Assistant reply:\n{assistant_message}"
    )
}

pub(crate) fn normalize_generated_title(raw: &str) -> Option<String> {
    let mut title = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .to_string();

    for prefix in ["title:", "Title:", "标题：", "标题:"] {
        if let Some(stripped) = title.strip_prefix(prefix) {
            title = stripped.trim().to_string();
            break;
        }
    }

    let title = collapse_whitespace(&title);
    let title = title
        .trim_matches(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\'' | '`' | '“' | '”' | '‘' | '’' | '[' | ']' | '(' | ')'
                )
        })
        .trim_end_matches(|character: char| {
            matches!(character, '.' | '。' | '!' | '！' | '?' | '？' | ':' | '：')
        })
        .trim()
        .to_string();

    if title.is_empty() {
        return None;
    }

    Some(truncate_chars(&title, 40))
}

pub(crate) fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
