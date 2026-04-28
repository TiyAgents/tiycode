use std::sync::{Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tiycore::agent::AgentMessage;
use tiycore::types::Usage;

use crate::core::agent_session::AgentSession;
use crate::core::agent_session_history::convert_history_messages;
use crate::core::agent_session_types::ResolvedModelRole;
use crate::core::context_compression::ContextTokenCalibration;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::thread::{MessageRecord, RunSummaryDto, ToolCallDto};

pub(crate) fn effective_prompt_tokens(input_tokens: u64, cache_read_tokens: u64) -> u64 {
    input_tokens.saturating_add(cache_read_tokens)
}

#[derive(Debug, Default)]
pub(crate) struct ContextCompressionRuntimeState {
    calibration: ContextTokenCalibration,
    pub(crate) pending_prompt_estimate: Option<u32>,
}

impl ContextCompressionRuntimeState {
    pub(crate) fn new(initial_calibration: ContextTokenCalibration) -> Self {
        Self {
            calibration: initial_calibration,
            pending_prompt_estimate: None,
        }
    }

    fn calibration(&self) -> ContextTokenCalibration {
        self.calibration
    }

    pub(crate) fn record_pending_prompt_estimate(&mut self, estimated_tokens: u32) {
        self.pending_prompt_estimate = Some(estimated_tokens);
    }

    fn observe_prompt_usage(&mut self, actual_prompt_tokens: u64) {
        let Some(estimated_tokens) = self.pending_prompt_estimate.take() else {
            return;
        };

        self.calibration = self
            .calibration
            .observe(estimated_tokens, actual_prompt_tokens);
    }
}

pub(crate) fn current_context_token_calibration(
    state: &StdMutex<ContextCompressionRuntimeState>,
) -> ContextTokenCalibration {
    state
        .lock()
        .map(|state| state.calibration())
        .unwrap_or_default()
}

pub(crate) fn record_pending_prompt_estimate(
    state: &StdMutex<ContextCompressionRuntimeState>,
    estimated_tokens: u32,
) {
    if let Ok(mut state) = state.lock() {
        state.record_pending_prompt_estimate(estimated_tokens);
    }
}

pub(crate) fn observe_context_usage_calibration(
    state: &StdMutex<ContextCompressionRuntimeState>,
    usage: &Usage,
) {
    let actual_prompt_tokens = effective_prompt_tokens(usage.input, usage.cache_read);
    if actual_prompt_tokens == 0 {
        return;
    }

    if let Ok(mut state) = state.lock() {
        state.observe_prompt_usage(actual_prompt_tokens);
    }
}

pub(crate) fn build_initial_context_token_calibration(
    latest_historical_run: Option<&RunSummaryDto>,
    history_messages: &[MessageRecord],
    history_tool_calls: &[ToolCallDto],
    primary_model: &ResolvedModelRole,
    system_prompt: &str,
) -> ContextTokenCalibration {
    let Some(latest_historical_run) = latest_historical_run else {
        return ContextTokenCalibration::default();
    };

    let historical_prompt_tokens = effective_prompt_tokens(
        latest_historical_run.usage.input_tokens,
        latest_historical_run.usage.cache_read_tokens,
    );
    if historical_prompt_tokens == 0
        || !run_summary_matches_primary_model(latest_historical_run, primary_model)
    {
        return ContextTokenCalibration::default();
    }

    let history =
        convert_history_messages(history_messages, history_tool_calls, &primary_model.model);
    let estimated_tokens = crate::core::context_compression::estimate_total_tokens(&history)
        .saturating_add(crate::core::context_compression::estimate_tokens(
            system_prompt,
        ));

    ContextTokenCalibration::from_observation(estimated_tokens, historical_prompt_tokens)
        .unwrap_or_default()
}

fn run_summary_matches_primary_model(
    run_summary: &RunSummaryDto,
    primary_model: &ResolvedModelRole,
) -> bool {
    run_summary.model_id.as_deref() == Some(primary_model.model_id.as_str())
        || run_summary.model_id.as_deref() == Some(primary_model.model.id.as_str())
}

/// Auto-compression hook body, extracted from the `set_transform_context`
/// closure in [`configure_agent`] so the control flow is testable in isolation
/// and so the closure's capture list stays narrow.
///
/// Contract:
/// - Returns the original `messages` unchanged when compression is not needed.
/// - On success, emits a `ContextCompressing` frontend event, calls the LLM
///   to produce a summary (primary or merge), persists `{reset, summary}`
///   markers to the DB with a conservative boundary id, and returns
///   `[summary, …recent_messages]`.
/// - On LLM error / cancellation, injects a **heuristic** summary (via
///   [`generate_discard_summary`]) at the head, persists the heuristic
///   summary + reset marker, and returns `[heuristic_summary, …recent_messages]`.
///   This prevents the user from losing all earlier context when the LLM
///   call fails.
///
/// The function is `pub(crate)` purely so an integration test can drive it
/// directly rather than via the Agent runtime.
///
/// [`generate_discard_summary`]: crate::core::context_compression::generate_discard_summary
pub(crate) async fn run_auto_compression(
    messages: Vec<AgentMessage>,
    settings: crate::core::context_compression::CompressionSettings,
    model_role: ResolvedModelRole,
    weak: Weak<AgentSession>,
    thread_id: String,
    run_id: String,
    response_language: Option<String>,
) -> Vec<AgentMessage> {
    // Phase 1: check if compression is needed.
    //
    // The hot-path caller (the `set_transform_context` closure) already gates
    // on `should_compress` before cloning the heavy state, so in production
    // this branch should never hit. It stays here defensively so direct
    // callers (e.g. unit tests) still get correct behaviour for under-budget
    // inputs without having to duplicate the check.
    if !crate::core::context_compression::should_compress(&messages, &settings) {
        return messages;
    }

    tracing::info!(
        thread_id = %thread_id,
        message_count = messages.len(),
        "Auto context compression triggered"
    );

    // Phase 2: emit "compressing" event so the frontend shows placeholder
    if let Some(session) = weak.upgrade() {
        let _ = session
            .event_tx
            .send(ThreadStreamEvent::ContextCompressing { run_id });
    }

    // Pick up the session-level abort signal so that a Cancel click during
    // "Compressing context…" short-circuits the LLM call instead of waiting
    // the 90s PRIMARY_SUMMARY_TIMEOUT.
    let abort_signal = weak.upgrade().map(|session| session.abort_signal.clone());
    if abort_signal.as_ref().is_some_and(|s| s.is_cancelled()) {
        // Already cancelled before we even started — skip the LLM call.
        tracing::info!(
            thread_id = %thread_id,
            "Auto compression skipped: cancellation already requested"
        );
        let fallback =
            crate::core::context_compression::compress_context_fallback(messages, &settings);
        // Write back so subsequent turns start from the compressed base.
        if let Some(session) = weak.upgrade() {
            session.agent.replace_messages(fallback.clone());
        }
        return fallback;
    }

    // Phase 3: decide cut point
    let token_estimates: Vec<u32> = messages
        .iter()
        .map(crate::core::context_compression::estimate_message_tokens)
        .collect();
    let cut_point = crate::core::context_compression::find_cut_point(
        &messages,
        &token_estimates,
        settings.keep_recent_tokens,
    );

    let old_messages = &messages[..cut_point];
    let recent_messages = &messages[cut_point..];

    // Skip if nothing to compress. This happens when cut_point is driven all
    // the way to 0 by the tool-call/tool-result boundary adjustment. Falling
    // through to the fallback truncation is better than returning `messages`
    // unchanged (which would exceed the context window on the very next
    // provider call).
    if old_messages.is_empty() {
        tracing::warn!(
            thread_id = %thread_id,
            "Auto compression cut_point == 0 (tool-call boundary prevented compression); using truncation fallback"
        );
        let fallback =
            crate::core::context_compression::compress_context_fallback(messages, &settings);
        // Write back so subsequent turns start from the compressed base.
        if let Some(session) = weak.upgrade() {
            session.agent.replace_messages(fallback.clone());
        }
        return fallback;
    }

    // Phase 4: if the old region already begins with a prior <context_summary>
    // block, merge instead of re-summarise to avoid summary-of-summary quality
    // decay. The prior summary was injected by a previous compression pass and
    // lives at the head of `messages`; the delta to summarise is the rest of
    // `old_messages`.
    let response_language = response_language.as_deref();
    let summary_result = match crate::core::agent_run_manager::detect_prior_summary(old_messages) {
        Some((prior_summary, prefix_len)) if prefix_len < old_messages.len() => {
            let delta = &old_messages[prefix_len..];
            tracing::info!(
                thread_id = %thread_id,
                delta_len = delta.len(),
                "Merging prior <context_summary> with new delta"
            );
            crate::core::agent_run_manager::generate_merge_summary(
                &model_role,
                &prior_summary,
                delta,
                None,
                response_language,
                abort_signal.clone(),
            )
            .await
        }
        Some((prior_summary, _prefix_len)) => {
            // Prior summary with no new delta (unlikely, since
            // should_compress fired). Reuse the prior summary verbatim
            // instead of calling the model for nothing.
            tracing::info!(
                thread_id = %thread_id,
                "Reusing prior <context_summary> — no delta to merge"
            );
            Ok(prior_summary)
        }
        None => {
            crate::core::agent_run_manager::generate_primary_summary(
                &model_role,
                old_messages,
                None,
                response_language,
                abort_signal.clone(),
            )
            .await
        }
    };

    // Boundary buffer used by both the success path (persist markers) and the
    // fallback path (persist markers before truncation). Defined once here so
    // a future tuning change only lands in a single place.
    //
    // Rationale: one DB message may expand to multiple in-memory AgentMessages
    // (a plan/summary marker; a run's tool_calls split into assistant+tool_result
    // pairs), so exact matching isn't feasible. A small buffer lets the boundary
    // id slightly overshoot (include a few more old DB rows in the next reload
    // than strictly necessary) but NEVER undershoot — so no in-memory recent
    // message can get dropped by the next load.
    const BOUNDARY_BUFFER: usize = 16;

    match summary_result {
        Ok(summary) => {
            // Phase 5: persist markers to DB.
            if let Some(session) = weak.upgrade() {
                let boundary_id = resolve_boundary_id(
                    &session.pool,
                    &thread_id,
                    recent_messages.len(),
                    BOUNDARY_BUFFER,
                )
                .await;

                if let Err(e) = session
                    .persist_compression_markers(
                        &thread_id,
                        &summary,
                        "auto",
                        boundary_id.as_deref(),
                    )
                    .await
                {
                    tracing::warn!(
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to persist auto-compression markers, continuing without DB record"
                    );
                }
            } else {
                tracing::warn!(
                    thread_id = %thread_id,
                    "Skipping auto-compression marker persistence: AgentSession dropped mid-compression. \
                    The next run will reload full history and re-trigger compression."
                );
            }

            // Phase 6: build compressed message list
            let result = crate::core::context_compression::build_compressed_messages(
                &summary,
                recent_messages,
            );

            // Phase 6.5: write back compressed messages to Agent internal state
            // so subsequent turns in the same run start from the compressed base
            // instead of re-compressing the full history every turn.
            if let Some(session) = weak.upgrade() {
                session.agent.replace_messages(result.clone());
            }

            tracing::info!(
                thread_id = %thread_id,
                discarded = cut_point,
                kept = result.len(),
                "Auto context compression completed"
            );

            result
        }
        Err(e) => {
            // LLM summary failed — fall back to pure truncation with a
            // **heuristic** summary injected at the head so the user never
            // fully loses the skeleton of earlier context. We also persist
            // that heuristic summary + reset marker to DB so the next run
            // starts from a clean boundary instead of re-loading the full
            // history and triggering compression again in a loop.
            tracing::warn!(
                thread_id = %thread_id,
                error = %e,
                "Auto context compression LLM summary failed, falling back to heuristic summary + truncation"
            );

            // On the fallback path the heuristic summary is much sparser than
            // an LLM-generated one, so the normal 16K recent window would be a
            // double reduction in available information. Recompute the cut
            // point with a larger keep window so users don't lose too much raw
            // recent context when the LLM call fails.
            let fallback_cut_point = crate::core::context_compression::find_cut_point(
                &messages,
                &token_estimates,
                crate::core::context_compression::FALLBACK_KEEP_RECENT_TOKENS,
            );
            // Never widen past the original cut point — the recent slice only
            // ever grows, never shrinks.
            let fallback_cut_point = fallback_cut_point.min(cut_point);
            let old_messages = &messages[..fallback_cut_point];
            let recent_messages = &messages[fallback_cut_point..];

            // Build the heuristic summary once so we can both persist it and
            // hand it to build_compressed_messages.
            // `compress_context_fallback` also generates one internally, but
            // we want the DB record and the in-memory context to agree on the
            // same text.
            let heuristic_summary =
                crate::core::context_compression::generate_discard_summary(old_messages);

            if let Some(session) = weak.upgrade() {
                let boundary_id = resolve_boundary_id(
                    &session.pool,
                    &thread_id,
                    recent_messages.len(),
                    BOUNDARY_BUFFER,
                )
                .await;

                if let Err(persist_err) = session
                    .persist_compression_markers(
                        &thread_id,
                        &heuristic_summary,
                        "auto_fallback",
                        boundary_id.as_deref(),
                    )
                    .await
                {
                    tracing::warn!(
                        thread_id = %thread_id,
                        error = %persist_err,
                        "Failed to persist fallback compression markers"
                    );
                }
            } else {
                tracing::warn!(
                    thread_id = %thread_id,
                    "Skipping fallback compression marker persistence: AgentSession dropped mid-compression. \
                    The next run will reload full history and re-trigger compression."
                );
            }

            let result = crate::core::context_compression::build_compressed_messages(
                &heuristic_summary,
                recent_messages,
            );

            // Write back fallback-compressed messages to Agent internal state
            // so subsequent turns start from the compressed base.
            if let Some(session) = weak.upgrade() {
                session.agent.replace_messages(result.clone());
            }

            tracing::info!(
                thread_id = %thread_id,
                discarded = fallback_cut_point,
                kept = result.len(),
                "Auto context compression fallback completed (heuristic summary)"
            );

            result
        }
    }
}

/// Resolve a conservative DB-backed boundary id for a compression pass.
///
/// Returns the id of the `(recent_len + buffer)`-th message from the end of
/// the thread, or `None` if the lookup fails or there are fewer rows than
/// that in the DB. `None` is always safe: it just means no `boundaryMessageId`
/// will be embedded in the reset marker and `list_since_last_reset` will fall
/// back to the reset row's own id as the lower bound.
///
/// Any error from the query is logged and converted to `None` — we never want
/// a transient DB failure to block compression; the worst-case is a small
/// extra reload on the next run.
async fn resolve_boundary_id(
    pool: &SqlitePool,
    thread_id: &str,
    recent_len: usize,
    buffer: usize,
) -> Option<String> {
    let n_from_end = recent_len.saturating_add(buffer);
    match crate::persistence::repo::message_repo::find_nth_from_end_id(pool, thread_id, n_from_end)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(
                thread_id = %thread_id,
                error = %e,
                "Failed to resolve boundary message id; persisting reset marker without it"
            );
            None
        }
    }
}

/// Free-function form of [`AgentSession::persist_compression_markers`].
///
/// Extracted so unit tests can drive the marker-persistence contract against
/// an in-memory `SqlitePool` without having to stand up a full `AgentSession`
/// (which requires a `ToolGateway`, `HelperAgentOrchestrator`, a model plan,
/// etc.). See the module-level tests in `agent_session::persist_marker_tests`.
///
/// Contract (mirrors the method):
/// - Writes a `context_reset` marker first, then a `context_summary` marker.
///   UUID v7 is time-ordered, so `reset.id < summary.id`, ensuring
///   `list_since_last_reset (WHERE id >= reset_id)` includes the summary row.
/// - When `boundary_message_id` is `Some(non_empty)`, it is attached to the
///   reset marker's metadata as `boundaryMessageId`. `None` or `Some("")` are
///   treated identically — no key is added — so the caller doesn't have to
///   pre-validate.
pub(crate) async fn persist_compression_markers_to_pool(
    pool: &SqlitePool,
    thread_id: &str,
    summary: &str,
    source: &str,
    boundary_message_id: Option<&str>,
) -> Result<(), crate::model::errors::AppError> {
    let summary_metadata = serde_json::json!({
        "kind": "context_summary",
        "source": source,
        "label": "Compacted context summary",
    });
    let mut reset_metadata = serde_json::json!({
        "kind": "context_reset",
        "source": source,
        "label": "Context is now reset",
    });
    if let Some(boundary_id) = boundary_message_id {
        if !boundary_id.is_empty() {
            reset_metadata
                .as_object_mut()
                .expect("reset_metadata is an object literal")
                .insert(
                    "boundaryMessageId".to_string(),
                    serde_json::Value::String(boundary_id.to_string()),
                );
        }
    }

    let reset_id = uuid::Uuid::now_v7().to_string();
    let summary_id = uuid::Uuid::now_v7().to_string();
    let reset_metadata_json = reset_metadata.to_string();
    let summary_metadata_json = summary_metadata.to_string();

    // Wrap both inserts in a single transaction so a mid-way failure (crash,
    // constraint violation, disk error) cannot leave the thread in a state
    // where the reset marker exists without the summary. Without this, a
    // partial write would cause `list_since_last_reset` to load from the
    // boundary but with no accompanying summary — effectively showing the
    // user an uncompressed head with a reset marker dangling.
    //
    // This also reduces WAL lock round-trips from 2 → 1 on success.
    let mut tx = pool.begin().await?;

    const INSERT_SQL: &str = "INSERT INTO messages (id, thread_id, run_id, role, content_markdown,
                message_type, status, metadata_json, attachments_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))";

    sqlx::query(INSERT_SQL)
        .bind(&reset_id)
        .bind(thread_id)
        .bind(None::<String>)
        .bind("system")
        .bind("Context is now reset")
        .bind("summary_marker")
        .bind("completed")
        .bind(&reset_metadata_json)
        .bind(None::<String>)
        .execute(&mut *tx)
        .await?;

    sqlx::query(INSERT_SQL)
        .bind(&summary_id)
        .bind(thread_id)
        .bind(None::<String>)
        .bind("system")
        .bind(summary)
        .bind("summary_marker")
        .bind("completed")
        .bind(&summary_metadata_json)
        .bind(None::<String>)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_resolved_model_role(model_id: &str) -> ResolvedModelRole {
        let model = tiycore::types::Model::builder()
            .id(model_id)
            .name(model_id)
            .provider(tiycore::types::Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(128_000)
            .max_tokens(32_000)
            .input(vec![tiycore::types::InputType::Text])
            .cost(tiycore::types::Cost::default())
            .build()
            .expect("sample resolved model");

        ResolvedModelRole {
            provider_id: format!("provider-{model_id}"),
            model_record_id: format!("record-{model_id}"),
            model_id: model_id.to_string(),
            model_name: model_id.to_string(),
            provider_type: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            api_key: Some("test-key".to_string()),
            provider_options: None,
            model,
        }
    }

    // -----------------------------------------------------------------------
    // persist_compression_markers_to_pool — data-integrity tests
    // -----------------------------------------------------------------------

    mod persist_markers {
        use super::persist_compression_markers_to_pool;
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use sqlx::{Row, SqlitePool};
        use std::str::FromStr;

        async fn setup_pool() -> SqlitePool {
            let options = SqliteConnectOptions::from_str("sqlite::memory:")
                .expect("invalid sqlite options")
                .foreign_keys(true);

            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .expect("failed to create in-memory pool");

            crate::persistence::sqlite::run_migrations(&pool)
                .await
                .expect("migrations failed");

            sqlx::query(
                "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                        is_default, is_git, auto_work_tree, status, created_at, updated_at)
                 VALUES ('ws-1', 'ws', '/tmp', '/tmp', '/tmp', 0, 0, 0, 'ready',
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .execute(&pool)
            .await
            .expect("seed workspace");

            sqlx::query(
                "INSERT INTO threads (id, workspace_id, title, status, created_at, updated_at, last_active_at)
                 VALUES ('t1', 'ws-1', 't', 'idle',
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .execute(&pool)
            .await
            .expect("seed thread");

            pool
        }

        /// Fetch the two markers we wrote, ordered by id ascending (reset then summary
        /// because UUID v7 is time-ordered and `reset` was written first).
        async fn fetch_markers(pool: &SqlitePool) -> Vec<(String, String, String, Option<String>)> {
            sqlx::query(
                "SELECT id, content_markdown, message_type, metadata_json
                   FROM messages
                  WHERE thread_id = 't1'
                    AND message_type = 'summary_marker'
               ORDER BY id ASC",
            )
            .fetch_all(pool)
            .await
            .expect("fetch markers")
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("id"),
                    row.get::<String, _>("content_markdown"),
                    row.get::<String, _>("message_type"),
                    row.get::<Option<String>, _>("metadata_json"),
                )
            })
            .collect()
        }

        #[tokio::test]
        async fn writes_reset_then_summary_with_boundary_id_embedded() {
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState A\n</context_summary>",
                "auto",
                Some("boundary-42"),
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            assert_eq!(rows.len(), 2, "should have written exactly 2 markers");

            let (reset_id, reset_body, reset_type, reset_meta) = &rows[0];
            let (summary_id, summary_body, summary_type, summary_meta) = &rows[1];

            // Invariant: reset written first → lower UUID v7 id.
            assert!(
                reset_id < summary_id,
                "reset ({}) must have a smaller id than summary ({})",
                reset_id,
                summary_id
            );

            assert_eq!(reset_type, "summary_marker");
            assert_eq!(summary_type, "summary_marker");
            assert_eq!(reset_body, "Context is now reset");
            assert_eq!(
                summary_body,
                "<context_summary>\nState A\n</context_summary>"
            );

            let reset_meta_val: serde_json::Value =
                serde_json::from_str(reset_meta.as_ref().expect("reset metadata present"))
                    .expect("reset metadata is valid json");
            assert_eq!(reset_meta_val["kind"], "context_reset");
            assert_eq!(reset_meta_val["source"], "auto");
            assert_eq!(reset_meta_val["boundaryMessageId"], "boundary-42");

            let summary_meta_val: serde_json::Value =
                serde_json::from_str(summary_meta.as_ref().expect("summary metadata present"))
                    .expect("summary metadata is valid json");
            assert_eq!(summary_meta_val["kind"], "context_summary");
            assert_eq!(summary_meta_val["source"], "auto");
            // boundaryMessageId only ever belongs on the reset row.
            assert!(
                summary_meta_val.get("boundaryMessageId").is_none(),
                "summary metadata should not carry boundaryMessageId"
            );
        }

        #[tokio::test]
        async fn omits_boundary_id_when_none() {
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState\n</context_summary>",
                "auto_fallback",
                None,
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            assert_eq!(rows.len(), 2);

            let reset_meta: serde_json::Value =
                serde_json::from_str(rows[0].3.as_ref().expect("reset metadata present"))
                    .expect("reset metadata is valid json");
            assert!(
                reset_meta.get("boundaryMessageId").is_none(),
                "None boundary id must not add any metadata key"
            );
            assert_eq!(reset_meta["source"], "auto_fallback");
        }

        #[tokio::test]
        async fn treats_empty_boundary_id_like_none() {
            // A defensive contract: callers that resolve the boundary via
            // `find_nth_from_end_id` may occasionally hand back an empty string.
            // The function must treat that identically to `None` rather than
            // writing `"boundaryMessageId": ""` to the DB (which would make
            // `list_since_last_reset` try to compare against an empty id).
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState\n</context_summary>",
                "auto",
                Some(""),
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            let reset_meta: serde_json::Value =
                serde_json::from_str(rows[0].3.as_ref().expect("reset metadata present"))
                    .expect("reset metadata is valid json");
            assert!(
                reset_meta.get("boundaryMessageId").is_none(),
                "empty boundary id must be treated identically to None"
            );
        }

        #[tokio::test]
        async fn source_label_flows_through_to_both_markers() {
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState\n</context_summary>",
                "manual_compact",
                Some("boundary-1"),
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            let reset_meta: serde_json::Value =
                serde_json::from_str(rows[0].3.as_ref().unwrap()).unwrap();
            let summary_meta: serde_json::Value =
                serde_json::from_str(rows[1].3.as_ref().unwrap()).unwrap();
            assert_eq!(reset_meta["source"], "manual_compact");
            assert_eq!(summary_meta["source"], "manual_compact");
        }
    }

    // -----------------------------------------------------------------------
    // run_auto_compression — orchestration path coverage
    //
    // These tests drive the extracted run_auto_compression function directly
    // without standing up a full AgentSession. By using `Weak::new()` (an
    // already-dangling weak reference), we cover the paths that do NOT make
    // an LLM call — should_compress early-return and cut_point==0 truncation
    // fallback. Paths that actually invoke a provider need integration-level
    // mocking and are out of scope here.
    // -----------------------------------------------------------------------

    mod run_auto_compression {
        use super::{run_auto_compression, sample_resolved_model_role, AgentSession};
        use std::sync::Weak;
        use tiycore::agent::AgentMessage;
        use tiycore::types::{
            Api, AssistantMessage, ContentBlock, Provider, StopReason, TextContent,
            ToolResultMessage, Usage, UserMessage,
        };

        fn make_user(text: &str) -> AgentMessage {
            AgentMessage::User(UserMessage::text(text))
        }

        fn make_assistant(text: &str) -> AgentMessage {
            AgentMessage::Assistant(
                AssistantMessage::builder()
                    .content(vec![ContentBlock::Text(TextContent::new(text))])
                    .api(Api::OpenAICompletions)
                    .provider(Provider::OpenAI)
                    .model("test")
                    .usage(Usage::default())
                    .stop_reason(StopReason::Stop)
                    .build()
                    .unwrap(),
            )
        }

        fn make_tool_result(name: &str, content: &str) -> AgentMessage {
            AgentMessage::ToolResult(ToolResultMessage::text("tc-1", name, content, false))
        }

        fn settings_for_test(
            context_window: u32,
            reserve_tokens: u32,
            keep_recent_tokens: u32,
        ) -> crate::core::context_compression::CompressionSettings {
            crate::core::context_compression::CompressionSettings {
                context_window,
                reserve_tokens,
                keep_recent_tokens,
            }
        }

        #[tokio::test]
        async fn returns_messages_unchanged_when_under_budget() {
            // With a generous budget, should_compress is false and the function
            // is a pure pass-through — no clone of messages, no LLM call, no
            // DB access. This exercises the most common hot-path behaviour.
            let messages = vec![make_user("hi"), make_assistant("hello")];
            let settings = settings_for_test(128_000, 1_024, 1_024);

            let result = run_auto_compression(
                messages.clone(),
                settings,
                sample_resolved_model_role("primary-model"),
                Weak::<AgentSession>::new(),
                "thread-x".to_string(),
                "run-x".to_string(),
                None,
            )
            .await;

            assert_eq!(result.len(), messages.len());
            // Content should be byte-identical — no summary was injected.
            match (&result[0], &messages[0]) {
                (AgentMessage::User(a), AgentMessage::User(b)) => {
                    let at = match &a.content {
                        tiycore::types::UserContent::Text(t) => t.as_str(),
                        _ => panic!("expected text"),
                    };
                    let bt = match &b.content {
                        tiycore::types::UserContent::Text(t) => t.as_str(),
                        _ => panic!("expected text"),
                    };
                    assert_eq!(at, bt);
                }
                _ => panic!("expected user message at head"),
            }
        }

        #[tokio::test]
        async fn cut_point_zero_falls_back_to_truncation_without_llm() {
            // When cut_point resolves to 0 (e.g., a thread dominated by
            // ToolResult messages with no safe cut boundary), the function
            // returns via compress_context_fallback BEFORE making any LLM
            // call. A dangling Weak reference proves no DB or session state
            // is required on this branch.
            let mut messages = Vec::new();
            // A long sequence of tool results with no user/assistant split —
            // find_cut_point will walk all the way back to 0 and the
            // tool-result boundary adjustment keeps it there.
            for i in 0..40 {
                messages.push(make_tool_result(
                    "read",
                    &format!("contents {}: {}", i, "x".repeat(600)),
                ));
            }

            // Tiny budget forces should_compress = true.
            let settings = settings_for_test(2_000, 500, 500);
            assert!(
                crate::core::context_compression::should_compress(&messages, &settings),
                "precondition: messages should be over budget"
            );

            let result = run_auto_compression(
                messages.clone(),
                settings.clone(),
                sample_resolved_model_role("primary-model"),
                Weak::<AgentSession>::new(),
                "thread-y".to_string(),
                "run-y".to_string(),
                None,
            )
            .await;

            // compress_context_fallback was used — result has fewer messages,
            // or (for all-tool-result threads) in-place truncated content.
            // The crucial property is: the function returned successfully
            // despite the dangling Weak, proving the LLM path was skipped.
            assert!(!result.is_empty());
            assert!(result.len() <= messages.len());
        }
    }
}
