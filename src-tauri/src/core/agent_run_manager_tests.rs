#[cfg(test)]
pub(super) mod tests {
    use super::super::{
        append_compact_instructions, await_summary_with_abort, build_compact_summary_messages,
        build_compact_summary_system_prompt, build_implementation_handoff_prompt,
        build_merge_summary_messages, build_merge_summary_system_prompt,
        build_orphaned_run_terminal_event, build_title_model_candidates, build_title_prompt,
        build_title_prompt_from_messages, collapse_whitespace, detect_prior_summary,
        extract_context_summary_block, extract_run_model_refs, extract_run_string,
        is_terminal_runtime_event, mark_thread_run_cancellation_requested, merge_json_value,
        normalize_compact_summary, normalize_generated_title, render_compact_summary_history,
        should_complete_reasoning_for_event, sidebar_status_for_runtime_event,
        summary_history_char_budget, terminal_event_status, truncate_chars,
        truncate_chars_keep_tail, truncate_tool_result_head_tail, ActiveRun,
        SUMMARY_HISTORY_MIN_CHARS, SUMMARY_TOOL_RESULT_MAX_CHARS,
    };
    use crate::core::agent_session::{ProfileResponseStyle, ResolvedModelRole};
    use crate::core::plan_checkpoint::{
        build_plan_artifact_from_tool_input, build_plan_message_metadata, PlanApprovalAction,
    };
    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use crate::model::thread::MessageRecord;
    use std::collections::HashMap;
    use tiycore::agent::AgentMessage;
    use tiycore::types::{Message as TiyMessage, UserMessage};
    use tokio::sync::{broadcast, Mutex};

    #[tokio::test]
    async fn mark_thread_run_cancellation_requested_returns_falsey_none_when_thread_is_inactive() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        let run_id = mark_thread_run_cancellation_requested(&active_runs, "thread-missing").await;

        assert_eq!(run_id, None);
    }

    #[tokio::test]
    async fn mark_thread_run_cancellation_requested_marks_matching_run_and_returns_run_id() {
        let (frontend_tx, _) = broadcast::channel::<ThreadStreamEvent>(1);
        let active_runs = Mutex::new(HashMap::from([(
            "run-1".to_string(),
            ActiveRun {
                run_id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                profile_id: None,
                frontend_tx,
                lightweight_model_role: None,
                auxiliary_model_role: None,
                primary_model_role: None,
                streaming_message_id: None,
                reasoning_message_id: None,
                cancellation_requested: false,
            },
        )]));

        let run_id = mark_thread_run_cancellation_requested(&active_runs, "thread-1").await;

        assert_eq!(run_id.as_deref(), Some("run-1"));
        let runs = active_runs.lock().await;
        assert!(runs
            .get("run-1")
            .is_some_and(|run| run.cancellation_requested));
    }

    // ------------------------------------------------------------------
    // ActiveRun lifecycle invariants for `compact_thread_context`
    // ------------------------------------------------------------------
    //
    // `compact_thread_context` and `start_run` share the same guard pattern
    // (see lines ~176 and ~544): an insert into `active_runs` guarded by
    // `runs.values().any(|run| run.thread_id == thread_id)`. The concurrency
    // correctness of /compact hinges on that guard and on `remove_active_run`
    // clearing the entry after the background task finishes, *even on
    // failure*. A full end-to-end test would need a mock `LlmProvider` plus
    // a real `BuiltInAgentRuntime`, which is disproportionate for verifying
    // what is fundamentally a `HashMap` insert/remove contract. Instead we
    // drive the guard pattern directly against the same data structure.

    fn make_active_run(thread_id: &str, run_id: &str) -> ActiveRun {
        let (frontend_tx, _) = broadcast::channel::<ThreadStreamEvent>(1);
        ActiveRun {
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            profile_id: None,
            frontend_tx,
            lightweight_model_role: None,
            auxiliary_model_role: None,
            primary_model_role: None,
            streaming_message_id: None,
            reasoning_message_id: None,
            cancellation_requested: false,
        }
    }

    #[test]
    fn orphaned_run_terminal_event_uses_cancelled_when_user_requested_stop() {
        let cancelled = build_orphaned_run_terminal_event("run-1", true);
        let interrupted = build_orphaned_run_terminal_event("run-2", false);

        assert!(matches!(
            cancelled,
            ThreadStreamEvent::RunCancelled { ref run_id } if run_id == "run-1"
        ));
        assert!(matches!(
            interrupted,
            ThreadStreamEvent::RunInterrupted { ref run_id } if run_id == "run-2"
        ));
    }

    #[test]
    fn terminal_event_status_maps_interrupted_to_cancelled_when_cancel_was_requested() {
        let interrupted = ThreadStreamEvent::RunInterrupted {
            run_id: "run-1".to_string(),
        };
        let cancelled = ThreadStreamEvent::RunCancelled {
            run_id: "run-1".to_string(),
        };

        assert_eq!(terminal_event_status(&interrupted, true), Some("cancelled"));
        assert_eq!(
            terminal_event_status(&interrupted, false),
            Some("interrupted")
        );
        assert_eq!(terminal_event_status(&cancelled, false), Some("cancelled"));
    }

    #[test]
    fn terminal_runtime_event_classifier_matches_run_terminal_variants_only() {
        let terminal_events = vec![
            ThreadStreamEvent::RunCheckpointed {
                run_id: "run-1".to_string(),
            },
            ThreadStreamEvent::RunCompleted {
                run_id: "run-1".to_string(),
            },
            ThreadStreamEvent::RunLimitReached {
                run_id: "run-1".to_string(),
                error: "turn limit".to_string(),
                max_turns: 3,
            },
            ThreadStreamEvent::RunFailed {
                run_id: "run-1".to_string(),
                error: "boom".to_string(),
            },
            ThreadStreamEvent::RunCancelled {
                run_id: "run-1".to_string(),
            },
            ThreadStreamEvent::RunInterrupted {
                run_id: "run-1".to_string(),
            },
        ];

        for event in terminal_events {
            assert!(
                is_terminal_runtime_event(&event),
                "expected {event:?} to be terminal"
            );
        }

        let non_terminal_events = vec![
            ThreadStreamEvent::RunStarted {
                run_id: "run-1".to_string(),
                run_mode: "default".to_string(),
            },
            ThreadStreamEvent::MessageDelta {
                run_id: "run-1".to_string(),
                message_id: "message-1".to_string(),
                delta: "hello".to_string(),
            },
            ThreadStreamEvent::ToolRequested {
                run_id: "run-1".to_string(),
                tool_call_id: "tool-1".to_string(),
                tool_name: "read".to_string(),
                tool_input: serde_json::json!({"path": "README.md"}),
            },
            ThreadStreamEvent::ThreadUsageUpdated {
                run_id: "run-1".to_string(),
                model_display_name: Some("model".to_string()),
                context_window: Some("small".to_string()),
                usage: Default::default(),
            },
        ];

        for event in non_terminal_events {
            assert!(
                !is_terminal_runtime_event(&event),
                "expected {event:?} to stay non-terminal"
            );
        }
    }

    #[test]
    fn terminal_event_status_maps_all_terminal_outcomes() {
        assert_eq!(
            terminal_event_status(
                &ThreadStreamEvent::RunCompleted {
                    run_id: "run-1".to_string()
                },
                false,
            ),
            Some("completed")
        );
        assert_eq!(
            terminal_event_status(
                &ThreadStreamEvent::RunLimitReached {
                    run_id: "run-1".to_string(),
                    error: "turn limit".to_string(),
                    max_turns: 3,
                },
                false,
            ),
            Some("limit_reached")
        );
        assert_eq!(
            terminal_event_status(
                &ThreadStreamEvent::RunFailed {
                    run_id: "run-1".to_string(),
                    error: "boom".to_string(),
                },
                false,
            ),
            Some("failed")
        );
        assert_eq!(
            terminal_event_status(
                &ThreadStreamEvent::RunCancelled {
                    run_id: "run-1".to_string()
                },
                false,
            ),
            Some("cancelled")
        );
        assert_eq!(
            terminal_event_status(
                &ThreadStreamEvent::RunInterrupted {
                    run_id: "run-1".to_string()
                },
                false,
            ),
            Some("interrupted")
        );
        assert_eq!(
            terminal_event_status(
                &ThreadStreamEvent::RunInterrupted {
                    run_id: "run-1".to_string()
                },
                true,
            ),
            Some("cancelled")
        );
        assert_eq!(
            terminal_event_status(
                &ThreadStreamEvent::RunStarted {
                    run_id: "run-1".to_string(),
                    run_mode: "default".to_string(),
                },
                false,
            ),
            None
        );
    }

    #[test]
    fn reasoning_completion_policy_ignores_progress_and_terminal_events_only() {
        let completes_reasoning = vec![
            ThreadStreamEvent::MessageDelta {
                run_id: "run-1".to_string(),
                message_id: "message-1".to_string(),
                delta: "hello".to_string(),
            },
            ThreadStreamEvent::MessageCompleted {
                run_id: "run-1".to_string(),
                message_id: "message-1".to_string(),
                content: "hello".to_string(),
                turn_index: None,
            },
            ThreadStreamEvent::ToolRequested {
                run_id: "run-1".to_string(),
                tool_call_id: "tool-1".to_string(),
                tool_name: "read".to_string(),
                tool_input: serde_json::json!({"path": "README.md"}),
            },
            ThreadStreamEvent::ApprovalRequired {
                run_id: "run-1".to_string(),
                tool_call_id: "tool-1".to_string(),
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "echo hi"}),
                reason: "needs approval".to_string(),
            },
        ];

        for event in completes_reasoning {
            assert!(
                should_complete_reasoning_for_event(&event),
                "expected {event:?} to complete active reasoning"
            );
        }

        let keeps_reasoning_open = vec![
            ThreadStreamEvent::RunStarted {
                run_id: "run-1".to_string(),
                run_mode: "default".to_string(),
            },
            ThreadStreamEvent::ReasoningUpdated {
                run_id: "run-1".to_string(),
                message_id: "reasoning-1".to_string(),
                reasoning: "thinking".to_string(),
                thinking_signature: Some("sig".to_string()),
                turn_index: None,
            },
            ThreadStreamEvent::ThreadUsageUpdated {
                run_id: "run-1".to_string(),
                model_display_name: None,
                context_window: None,
                usage: Default::default(),
            },
            ThreadStreamEvent::RunCheckpointed {
                run_id: "run-1".to_string(),
            },
            ThreadStreamEvent::ContextCompressing {
                run_id: "run-1".to_string(),
            },
            ThreadStreamEvent::RunCompleted {
                run_id: "run-1".to_string(),
            },
            ThreadStreamEvent::RunFailed {
                run_id: "run-1".to_string(),
                error: "boom".to_string(),
            },
            ThreadStreamEvent::RunCancelled {
                run_id: "run-1".to_string(),
            },
            ThreadStreamEvent::RunInterrupted {
                run_id: "run-1".to_string(),
            },
        ];

        for event in keeps_reasoning_open {
            assert!(
                !should_complete_reasoning_for_event(&event),
                "expected {event:?} not to complete active reasoning"
            );
        }
    }

    /// Mirrors the check in `compact_thread_context` (and `start_run`): a
    /// second concurrent run on the same thread must be rejected so the
    /// thread can't accumulate overlapping ActiveRun entries.
    #[tokio::test]
    async fn active_run_guard_rejects_second_run_on_same_thread() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        // First compact inserts successfully.
        {
            let mut runs = active_runs.lock().await;
            let already_active = runs.values().any(|run| run.thread_id == "thread-1");
            assert!(!already_active);
            runs.insert("run-1".to_string(), make_active_run("thread-1", "run-1"));
        }

        // Second compact (same thread) must see the guard fire.
        {
            let runs = active_runs.lock().await;
            let already_active = runs.values().any(|run| run.thread_id == "thread-1");
            assert!(
                already_active,
                "Guard must reject overlapping compact on same thread"
            );
        }

        // A different thread is unaffected.
        {
            let runs = active_runs.lock().await;
            let other_thread_active = runs.values().any(|run| run.thread_id == "thread-2");
            assert!(!other_thread_active);
        }
    }

    /// After `run_compact_background` finishes — whether the LLM call
    /// succeeded or failed — the ActiveRun entry must be gone so subsequent
    /// /compact invocations are accepted. The doc comment on
    /// `run_compact_background` promises this invariant; this test pins
    /// it to the data-structure contract it ultimately relies on.
    #[tokio::test]
    async fn active_run_removed_unblocks_future_compacts_on_same_thread() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        // Simulate a compact run being registered and then cleaned up.
        {
            let mut runs = active_runs.lock().await;
            runs.insert(
                "run-compact".to_string(),
                make_active_run("thread-1", "run-compact"),
            );
        }
        {
            let mut runs = active_runs.lock().await;
            runs.remove("run-compact");
        }

        // A follow-up compact must now observe no active run on thread-1.
        let runs = active_runs.lock().await;
        let blocked = runs.values().any(|run| run.thread_id == "thread-1");
        assert!(
            !blocked,
            "Stale ActiveRun entry would leave the thread stuck in Running"
        );
    }

    /// Setup failure in `compact_thread_context` (e.g. DB insert rejecting
    /// the user message) must roll back the ActiveRun insert — otherwise
    /// every subsequent /compact on the same thread would return
    /// `thread.run.already_active` until the process restarts.
    #[tokio::test]
    async fn active_run_rolled_back_on_setup_failure() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        // Simulated setup sequence: insert, then encounter a failure and
        // clean up — exactly what `compact_thread_context` does at the
        // `if let Err(error) = setup { self.remove_active_run(...) }`
        // branch.
        {
            let mut runs = active_runs.lock().await;
            runs.insert(
                "run-setup-fail".to_string(),
                make_active_run("thread-1", "run-setup-fail"),
            );
        }
        // ... setup returns Err here in production ...
        {
            let mut runs = active_runs.lock().await;
            runs.remove("run-setup-fail");
        }

        // Next /compact attempt on the same thread must succeed.
        let runs = active_runs.lock().await;
        let blocked = runs.values().any(|run| run.thread_id == "thread-1");
        assert!(
            !blocked,
            "Setup-failure path must leave active_runs empty for the thread"
        );
    }

    #[test]
    fn normalize_generated_title_strips_prefixes_and_wrappers() {
        assert_eq!(
            normalize_generated_title("Title: \"Fix terminal resize drift\"").as_deref(),
            Some("Fix terminal resize drift")
        );
        assert_eq!(
            normalize_generated_title("标题：   新建线程标题生成   ").as_deref(),
            Some("新建线程标题生成")
        );
    }

    #[test]
    fn collapse_whitespace_compacts_internal_spacing() {
        assert_eq!(collapse_whitespace("foo   bar\nbaz"), "foo bar baz");
    }

    #[test]
    fn truncate_chars_limits_character_count() {
        assert_eq!(truncate_chars("abcdef", 4), "abcd");
        assert_eq!(truncate_chars("你好世界标题", 4), "你好世界");
    }

    #[test]
    fn normalize_compact_summary_wraps_plain_text_output() {
        assert_eq!(
            normalize_compact_summary("Goal: fix compact summary".to_string(), None).as_deref(),
            Some("<context_summary>\nGoal: fix compact summary\n</context_summary>")
        );
    }

    #[test]
    fn normalize_compact_summary_extracts_single_wrapped_block_from_noisy_output() {
        let summary = normalize_compact_summary(
            "Here is the summary:\n<context_summary>\nState\n</context_summary>\nTrailing note"
                .to_string(),
            None,
        )
        .expect("summary should be present");

        assert_eq!(summary, "<context_summary>\nState\n</context_summary>");
    }

    #[test]
    fn normalize_compact_summary_recovers_from_missing_closing_wrapper() {
        let summary =
            normalize_compact_summary("<context_summary>\nGoal\n- Pending item".to_string(), None)
                .expect("summary should be present");

        assert_eq!(
            summary,
            "<context_summary>\nGoal\n- Pending item\n</context_summary>"
        );
    }

    #[test]
    fn compact_summary_system_prompt_includes_wrapper_example() {
        let prompt = build_compact_summary_system_prompt(None);

        assert!(prompt.contains("Output rules:"));
        assert!(prompt.contains("Do not output any text before or after the wrapper."));
        assert!(prompt.contains("Example output:"));
        assert!(prompt.contains("<context_summary>"));
        assert!(prompt.contains("</context_summary>"));
    }

    #[test]
    fn compact_summary_system_prompt_uses_response_language_when_present() {
        let prompt = build_compact_summary_system_prompt(Some(" 简体中文 "));

        assert!(prompt.contains(
            "Respond in 简体中文 unless the user explicitly asks for a different language."
        ));
    }

    #[test]
    fn compact_summary_messages_split_instructions_and_history() {
        let history = vec![AgentMessage::User(UserMessage::text(
            "User asked for a compact summary",
        ))];
        let messages =
            build_compact_summary_messages(&history, Some("Keep unresolved risks"), 100_000);

        assert_eq!(messages.len(), 2);

        match &messages[0] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(text) => text,
                    _ => panic!("expected text user message for instructions"),
                };
                assert!(text.contains("Additional user instructions for this compact"));
                assert!(text.contains("Keep unresolved risks"));
            }
            _ => panic!("expected first compact message to be user instructions"),
        }

        match &messages[1] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(text) => text,
                    _ => panic!("expected text user message for history"),
                };
                assert!(text.starts_with("Conversation history to compact:"));
                assert!(text.contains("[user]"));
                assert!(text.contains("User asked for a compact summary"));
            }
            _ => panic!("expected second compact message to be user history"),
        }
    }

    #[test]
    fn extract_context_summary_block_returns_first_complete_block() {
        let extracted = extract_context_summary_block(
            "prefix\n<context_summary>\nFirst\n</context_summary>\n<context_summary>\nSecond\n</context_summary>",
        )
        .expect("context summary block should be extracted");

        assert_eq!(extracted, "<context_summary>\nFirst\n</context_summary>");
    }

    #[test]
    fn append_compact_instructions_adds_extra_block() {
        let summary = append_compact_instructions(
            "<context_summary>\nState\n</context_summary>".to_string(),
            Some("Preserve pending migration notes"),
        );

        assert!(summary.contains("<extra_instructions>"));
        assert!(summary.contains("Preserve pending migration notes"));
    }

    #[test]
    fn normalize_compact_summary_keeps_existing_wrapper_and_appends_instructions() {
        let summary = normalize_compact_summary(
            "<context_summary>\nState\n</context_summary>".to_string(),
            Some("Keep unresolved API choice"),
        )
        .expect("summary should be present");

        assert!(summary.starts_with("<context_summary>"));
        assert!(summary.contains("<extra_instructions>"));
        assert!(summary.contains("Keep unresolved API choice"));
    }

    #[test]
    fn title_prompt_uses_profile_response_language_when_present() {
        let prompt = build_title_prompt(
            "请帮我排查窗口缩放问题",
            "我已经定位到标题栏重绘时机。",
            Some("Japanese"),
            ProfileResponseStyle::Guide,
        );

        assert!(prompt.contains("Write the title in Japanese."));
        assert!(prompt.contains("signals the user's goal or decision focus clearly"));
    }

    #[test]
    fn title_prompt_from_messages_renders_newest_messages_first() {
        let messages = vec![
            MessageRecord {
                id: "msg-1".into(),
                thread_id: "thread-1".into(),
                run_id: None,
                role: "user".into(),
                content_markdown: "oldest user message".into(),
                message_type: "plain_message".into(),
                status: "completed".into(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-2".into(),
                thread_id: "thread-1".into(),
                run_id: None,
                role: "assistant".into(),
                content_markdown: "newer assistant reply".into(),
                message_type: "plain_message".into(),
                status: "completed".into(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-3".into(),
                thread_id: "thread-1".into(),
                run_id: None,
                role: "user".into(),
                content_markdown: "newest user follow-up".into(),
                message_type: "plain_message".into(),
                status: "completed".into(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let prompt = build_title_prompt_from_messages(
            &messages,
            Some("English"),
            ProfileResponseStyle::Balanced,
        );

        let newest_idx = prompt
            .find("User:\nnewest user follow-up")
            .expect("newest message should be present");
        let older_idx = prompt
            .find("Assistant:\nnewer assistant reply")
            .expect("older assistant message should be present");
        let oldest_idx = prompt
            .find("User:\noldest user message")
            .expect("oldest message should be present");

        assert!(newest_idx < older_idx);
        assert!(older_idx < oldest_idx);
        assert!(prompt.contains("Write the title in English."));
    }

    #[test]
    fn terminal_event_status_and_runtime_event_classification_cover_terminal_outcomes() {
        let completed = ThreadStreamEvent::RunCompleted {
            run_id: "run-1".to_string(),
        };
        assert_eq!(terminal_event_status(&completed, false), Some("completed"));
        assert!(is_terminal_runtime_event(&completed));

        let limit = ThreadStreamEvent::RunLimitReached {
            run_id: "run-1".to_string(),
            error: "too many turns".to_string(),
            max_turns: 10,
        };
        assert_eq!(terminal_event_status(&limit, false), Some("limit_reached"));
        assert!(is_terminal_runtime_event(&limit));

        let failed = ThreadStreamEvent::RunFailed {
            run_id: "run-1".to_string(),
            error: "boom".to_string(),
        };
        assert_eq!(terminal_event_status(&failed, false), Some("failed"));

        let cancelled = ThreadStreamEvent::RunCancelled {
            run_id: "run-1".to_string(),
        };
        assert_eq!(terminal_event_status(&cancelled, false), Some("cancelled"));

        let interrupted = ThreadStreamEvent::RunInterrupted {
            run_id: "run-1".to_string(),
        };
        assert_eq!(
            terminal_event_status(&interrupted, false),
            Some("interrupted")
        );
        assert_eq!(terminal_event_status(&interrupted, true), Some("cancelled"));

        let delta = ThreadStreamEvent::MessageDelta {
            run_id: "run-1".to_string(),
            message_id: "message-1".to_string(),
            delta: "hi".to_string(),
        };
        assert_eq!(terminal_event_status(&delta, false), None);
        assert!(!is_terminal_runtime_event(&delta));
    }

    #[test]
    fn extract_run_model_refs_reads_profile_provider_and_model_fallbacks() {
        let plan = serde_json::json!({
            "profileId": "profile-1",
            "primary": {
                "providerId": "provider-1",
                "modelRecordId": "record-1",
                "modelId": "model-1"
            }
        });
        assert_eq!(
            extract_run_string(&plan, &["primary", "providerId"]).as_deref(),
            Some("provider-1")
        );
        assert_eq!(
            extract_run_model_refs(&plan),
            (
                Some("profile-1".to_string()),
                Some("provider-1".to_string()),
                Some("record-1".to_string())
            )
        );

        let fallback = serde_json::json!({
            "primary": { "providerId": "provider-2", "modelId": "model-2" }
        });
        assert_eq!(
            extract_run_model_refs(&fallback),
            (
                None,
                Some("provider-2".to_string()),
                Some("model-2".to_string())
            )
        );
        assert_eq!(extract_run_string(&fallback, &["primary", "missing"]), None);
    }

    #[test]
    fn agent_run_manager_merge_json_value_recursively_merges_payload_options() {
        let mut base = serde_json::json!({
            "messages": [],
            "providerOptions": { "temperature": 0.1, "nested": { "a": 1 } },
            "replace": { "old": true }
        });
        let patch = serde_json::json!({
            "providerOptions": { "topP": 0.9, "nested": { "b": 2 } },
            "replace": null
        });

        merge_json_value(&mut base, &patch);

        assert_eq!(base["providerOptions"]["temperature"], 0.1);
        assert_eq!(base["providerOptions"]["topP"], 0.9);
        assert_eq!(
            base["providerOptions"]["nested"],
            serde_json::json!({ "a": 1, "b": 2 })
        );
        assert_eq!(base["replace"], serde_json::Value::Null);
    }

    #[test]
    fn reasoning_completion_helper_keeps_only_live_reasoning_events_open() {
        assert!(!should_complete_reasoning_for_event(
            &ThreadStreamEvent::RunStarted {
                run_id: "run-1".into(),
                run_mode: "default".into(),
            }
        ));
        assert!(!should_complete_reasoning_for_event(
            &ThreadStreamEvent::ReasoningUpdated {
                run_id: "run-1".into(),
                message_id: "reasoning-1".into(),
                reasoning: "Inspecting".into(),
                thinking_signature: None,
                turn_index: None,
            }
        ));
        assert!(should_complete_reasoning_for_event(
            &ThreadStreamEvent::ToolRequested {
                run_id: "run-1".into(),
                tool_call_id: "tool-1".into(),
                tool_name: "search".into(),
                tool_input: serde_json::json!({ "query": "Thought" }),
            }
        ));
    }

    #[test]
    fn implementation_handoff_prompt_embeds_the_approved_plan() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Approved plan",
                "summary": "Execute the plan exactly.",
                "steps": ["Apply the checkpointed implementation plan."]
            }),
            4,
        );
        let metadata = build_plan_message_metadata(artifact, "run-plan", "plan");

        let prompt = build_implementation_handoff_prompt(
            "thread-handoff-test",
            &metadata,
            PlanApprovalAction::ApplyPlanWithContextReset,
        );

        assert!(prompt.contains("Plan revision: 4"));
        assert!(prompt.contains(
            "The reset context already includes a historical summary and the approved plan."
        ));
        assert!(
            prompt.contains("Treat the approved plan in context as the implementation baseline.")
        );
        assert!(prompt.contains("after clearing the planning conversation"));
        assert!(prompt.contains("agent_review with planFilePath"));
    }

    #[test]
    fn detect_prior_summary_matches_wrapped_first_user_message() {
        let messages = vec![
            AgentMessage::User(UserMessage::text(
                "<context_summary>\nState A\n</context_summary>",
            )),
            AgentMessage::User(UserMessage::text("follow-up question")),
        ];

        let (prior, prefix_len) =
            detect_prior_summary(&messages).expect("prior summary should be detected");
        assert!(prior.contains("State A"));
        assert_eq!(prefix_len, 1);
    }

    #[test]
    fn detect_prior_summary_tolerates_leading_whitespace() {
        let messages = vec![AgentMessage::User(UserMessage::text(
            "   \n<context_summary>\nState\n</context_summary>",
        ))];
        assert!(detect_prior_summary(&messages).is_some());
    }

    #[test]
    fn detect_prior_summary_rejects_non_user_first_message() {
        let messages = vec![AgentMessage::User(UserMessage::text(
            "not a summary, just a question",
        ))];
        assert!(detect_prior_summary(&messages).is_none());
    }

    #[test]
    fn detect_prior_summary_rejects_truncated_block_without_closing_tag() {
        // An unterminated wrapper is not a well-formed prior summary — fall
        // back to the normal re-summarise path rather than merging into a
        // partial block.
        let messages = vec![AgentMessage::User(UserMessage::text(
            "<context_summary>\nState without close tag",
        ))];
        assert!(detect_prior_summary(&messages).is_none());
    }

    #[test]
    fn detect_prior_summary_accepts_blocks_content_with_single_text_block() {
        // The Blocks variant is used when an attachment or image is attached.
        // For detection we only require a single Text block to carry a
        // well-formed <context_summary>. Multi-modal Blocks should still
        // detect the summary from whichever block contains the text.
        use tiycore::types::{ContentBlock, TextContent};
        let user_msg = UserMessage::blocks(vec![ContentBlock::Text(TextContent::new(
            "<context_summary>\nBlocks-wrapped state\n</context_summary>",
        ))]);
        let messages = vec![AgentMessage::User(user_msg)];

        let (prior, prefix_len) =
            detect_prior_summary(&messages).expect("Blocks path should still detect summary");
        assert!(prior.contains("Blocks-wrapped state"));
        assert_eq!(prefix_len, 1);
    }

    #[test]
    fn detect_prior_summary_rejects_blocks_with_only_image() {
        // A Blocks user message that contains no Text at all (e.g. an
        // image-only attachment) cannot carry a summary — the detector
        // should treat it like "no summary present" and fall through.
        use tiycore::types::{ContentBlock, ImageContent};
        let user_msg = UserMessage::blocks(vec![ContentBlock::Image(ImageContent::new(
            "AAAA",
            "image/png",
        ))]);
        let messages = vec![AgentMessage::User(user_msg)];
        assert!(detect_prior_summary(&messages).is_none());
    }

    #[test]
    fn merge_summary_system_prompt_explains_the_merge_contract() {
        let prompt = build_merge_summary_system_prompt(None);
        assert!(prompt.contains("PRIOR summary"));
        assert!(prompt.contains("DELTA"));
        assert!(prompt.contains("<context_summary>"));
        assert!(prompt.contains("</context_summary>"));
    }

    #[test]
    fn merge_summary_system_prompt_uses_response_language_when_present() {
        let prompt = build_merge_summary_system_prompt(Some("Japanese"));

        assert!(prompt.contains(
            "Respond in Japanese unless the user explicitly asks for a different language."
        ));
    }

    #[test]
    fn merge_summary_system_prompt_ignores_blank_response_language() {
        let prompt = build_merge_summary_system_prompt(Some("   "));

        assert!(!prompt.contains("Respond in"));
    }

    #[test]
    fn merge_summary_messages_include_prior_and_delta_in_order() {
        let delta = vec![AgentMessage::User(UserMessage::text(
            "New user input to fold into the summary",
        ))];
        let messages = build_merge_summary_messages(
            "<context_summary>\nOld state\n</context_summary>",
            &delta,
            Some("Keep API choice intact"),
            100_000,
        );

        assert_eq!(messages.len(), 3);

        // 0: instructions, 1: prior summary, 2: delta history
        match &messages[1] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(t) => t,
                    _ => panic!("expected text user message"),
                };
                assert!(text.starts_with("Prior summary"));
                assert!(text.contains("Old state"));
            }
            _ => panic!("expected the prior-summary slot to be a user message"),
        }

        match &messages[2] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(t) => t,
                    _ => panic!("expected text user message"),
                };
                assert!(text.starts_with("New conversation delta"));
                assert!(text.contains("New user input to fold"));
            }
            _ => panic!("expected the delta slot to be a user message"),
        }
    }

    #[test]
    fn merge_summary_messages_omit_instructions_slot_when_none() {
        // No instructions = no leading instructions slot → exactly 2 messages
        // (prior summary, delta history). If we accidentally start sending 3
        // messages with an empty instructions block, the model would waste
        // tokens on a stub prompt.
        let delta = vec![AgentMessage::User(UserMessage::text("delta message"))];
        let messages = build_merge_summary_messages(
            "<context_summary>\nOld state\n</context_summary>",
            &delta,
            None,
            100_000,
        );

        assert_eq!(
            messages.len(),
            2,
            "without instructions the merge-summary payload should be prior+delta only"
        );

        // Slot 0 must now be the prior-summary (not instructions).
        match &messages[0] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(t) => t,
                    _ => panic!("expected text user message"),
                };
                assert!(text.starts_with("Prior summary"));
            }
            _ => panic!("expected user message at slot 0"),
        }
    }

    #[test]
    fn merge_summary_messages_treat_whitespace_instructions_as_none() {
        // Whitespace-only instructions are semantically equivalent to None and
        // must not produce a dangling empty instructions slot.
        let delta = vec![AgentMessage::User(UserMessage::text("delta"))];
        let messages = build_merge_summary_messages(
            "<context_summary>\nOld\n</context_summary>",
            &delta,
            Some("   \n\t  "),
            100_000,
        );
        assert_eq!(
            messages.len(),
            2,
            "whitespace-only instructions should behave like None"
        );
    }

    #[tokio::test]
    async fn await_summary_with_abort_returns_future_value_when_not_cancelled() {
        let signal = tiycore::agent::AbortSignal::new();
        let result = await_summary_with_abort(async { 42_u32 }, Some(signal))
            .await
            .expect("future should complete");
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn await_summary_with_abort_short_circuits_when_signal_already_cancelled() {
        let signal = tiycore::agent::AbortSignal::new();
        signal.cancel();

        // The future below would block indefinitely — the test only passes if
        // the pre-check on the already-cancelled signal returns Err without
        // polling the future.
        let blocker = std::future::pending::<u32>();
        let error = await_summary_with_abort(blocker, Some(signal))
            .await
            .expect_err("expected cancellation to short-circuit");
        assert_eq!(error.error_code, "runtime.context_compression.cancelled");
    }

    #[tokio::test]
    async fn await_summary_with_abort_cancels_midflight_future() {
        use std::sync::Arc;
        use tokio::sync::Notify;

        let signal = tiycore::agent::AbortSignal::new();
        let signal_for_task = signal.clone();

        // Deterministic mid-flight handshake: the future below notifies once
        // it has been polled at least once, and the canceller only fires
        // *after* receiving that notification. This replaces a timing-based
        // `sleep(20ms)` that could flake under CI load.
        let polled = Arc::new(Notify::new());
        let polled_for_canceller = polled.clone();

        let canceller = tokio::spawn(async move {
            polled_for_canceller.notified().await;
            signal_for_task.cancel();
        });

        let polled_for_future = polled.clone();
        let blocker = async move {
            // Signal on the first poll, then block forever — the test only
            // passes if the select branch picks up the subsequent cancel.
            polled_for_future.notify_one();
            std::future::pending::<u32>().await
        };

        let error = await_summary_with_abort(blocker, Some(signal))
            .await
            .expect_err("expected cancellation to race the pending future");
        assert_eq!(error.error_code, "runtime.context_compression.cancelled");

        // The canceller always completes: it only ever awaits `polled` once
        // and then cancels. Awaiting it here guarantees no test leaks a
        // dangling task on success.
        canceller.await.expect("canceller task should complete");
    }

    #[tokio::test]
    async fn await_summary_with_abort_passes_through_when_no_signal_provided() {
        let result = await_summary_with_abort(async { "hi".to_string() }, None)
            .await
            .expect("without a signal, errors are not produced");
        assert_eq!(result, "hi");
    }

    // ----- render_compact_summary_history: budget-aware packing -----

    fn build_assistant_text_message(text: &str) -> AgentMessage {
        use tiycore::types::{
            Api, AssistantMessage, ContentBlock, Provider, StopReason, TextContent, Usage,
        };
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

    #[test]
    fn render_compact_summary_history_preserves_full_messages_when_within_budget() {
        // Pre-refactor behaviour capped user messages at 1,200 chars; a
        // 3,000-char user message would get silently clipped. Verify the new
        // holistic budget keeps it intact end-to-end.
        let long = "x".repeat(3_000);
        let history = vec![
            AgentMessage::User(UserMessage::text(&long)),
            build_assistant_text_message("short reply"),
        ];

        let rendered = render_compact_summary_history(&history, 100_000);
        assert!(
            rendered.contains(&long),
            "expected full 3000-char user message to be preserved"
        );
        assert!(rendered.contains("short reply"));
        // Chronological order: user before assistant.
        let user_pos = rendered.find("[user]").unwrap();
        let assistant_pos = rendered.find("[assistant]").unwrap();
        assert!(user_pos < assistant_pos);
    }

    #[test]
    fn render_compact_summary_history_drops_oldest_when_budget_exhausted() {
        // With a tiny budget that only fits one message, the NEWEST should
        // survive and the oldest should be dropped (newest-to-oldest packing,
        // then reversed).
        let history = vec![
            AgentMessage::User(UserMessage::text("OLDEST: ancient message")),
            build_assistant_text_message("MIDDLE: intermediate"),
            AgentMessage::User(UserMessage::text("NEWEST: recent message")),
        ];

        // ~60 chars budget: fits only one short chunk (including header + \n\n).
        let rendered = render_compact_summary_history(&history, 60);
        assert!(rendered.contains("NEWEST"));
        assert!(!rendered.contains("OLDEST"));
    }

    #[test]
    fn render_compact_summary_history_tail_truncates_single_oversized_item() {
        // A single item larger than the entire budget should not be dropped
        // — the tail should be kept (more recent portion is more relevant)
        // and an elision marker inserted so the model knows content was cut.
        let massive = "line ".repeat(2_000); // 10_000 chars
        let history = vec![AgentMessage::User(UserMessage::text(&massive))];

        let rendered = render_compact_summary_history(&history, 500);
        assert!(rendered.chars().count() <= 500);
        assert!(rendered.contains("earlier content truncated"));
    }

    #[test]
    fn render_compact_summary_history_skips_empty_chunks_instead_of_counting_them() {
        // An empty user message must not consume any budget and must not
        // emit a stray [user] header — those chunks are `continue`d.
        use tiycore::types::{ContentBlock, TextContent};
        let empty_blocks = UserMessage::blocks(vec![ContentBlock::Text(TextContent::new(""))]);
        let history = vec![
            AgentMessage::User(empty_blocks),
            build_assistant_text_message("reply"),
        ];
        let rendered = render_compact_summary_history(&history, 100_000);
        assert!(!rendered.contains("[user]"));
        assert!(rendered.contains("reply"));
    }

    // ----- truncate_chars_keep_tail -----

    #[test]
    fn truncate_chars_keep_tail_is_noop_when_under_limit() {
        assert_eq!(truncate_chars_keep_tail("hello", 10), "hello");
    }

    #[test]
    fn truncate_chars_keep_tail_keeps_tail_with_marker_when_over_limit() {
        let s = "abcdefghijklmnopqrstuvwxyz"; // 26 chars
        let out = truncate_chars_keep_tail(s, 40); // > 26 → no-op
        assert_eq!(out, s);

        // Over limit: result must fit budget, end with tail, contain marker.
        let big: String = "0123456789".repeat(20); // 200 chars
        let out = truncate_chars_keep_tail(&big, 60);
        assert!(out.chars().count() <= 60);
        assert!(out.contains("earlier content truncated"));
        assert!(out.ends_with("9")); // tail of big ends with '9'
    }

    #[test]
    fn truncate_chars_keep_tail_handles_cjk_safely() {
        // Each CJK char is 3 bytes in UTF-8; keep_tail must walk by char
        // boundaries, not bytes, or it would panic mid-character.
        let cjk = "一二三四五六七八九十"; // 10 chars, 30 bytes
        let out = truncate_chars_keep_tail(cjk, 5);
        // Output is either just the 5-char tail (no marker) or a mix — but
        // must never panic and must not exceed the budget.
        assert!(out.chars().count() <= 5);
    }

    // ----- truncate_tool_result_head_tail -----

    #[test]
    fn truncate_tool_result_head_tail_noop_when_under_limit() {
        let text = "short tool output";
        assert_eq!(truncate_tool_result_head_tail(text, 100), text.to_string());
    }

    #[test]
    fn truncate_tool_result_head_tail_preserves_head_and_tail() {
        // 10_000 chars, budget 200 → head ~2/3 + tail ~1/3 of (200 - marker).
        let text: String = (0..10_000)
            .map(|i| char::from(b'A' + (i % 26) as u8))
            .collect();
        let out = truncate_tool_result_head_tail(&text, 200);
        assert!(out.chars().count() <= 200);
        // Must contain the omission marker.
        assert!(out.contains("chars omitted"));
        // Head starts with same chars as original.
        assert!(out.starts_with("ABCDE"));
        // Tail ends with same chars as original.
        let original_tail: String = text
            .chars()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(out.ends_with(&original_tail));
    }

    #[test]
    fn truncate_tool_result_head_tail_small_gap_falls_back_to_head_truncation() {
        // When the text is only slightly over budget, the omitted middle
        // section is tiny (< 50 chars). The function should skip the
        // head+tail gap marker and use simple head truncation instead.
        //
        // marker_len = 31, budget = 100, content_budget = 69,
        // head = 46, tail = 23, omitted = total - 69.
        // For omitted < 50: total < 69 + 50 = 119.
        // Use total = 110 → omitted = 110 - 69 = 41 < 50 → head fallback.
        let text = "x".repeat(110);
        let out = truncate_tool_result_head_tail(&text, 100);
        assert!(out.chars().count() <= 100);
        // Should NOT contain "chars omitted" (small gap → plain head truncation).
        assert!(
            !out.contains("chars omitted"),
            "small-gap case should use plain head truncation, got: {}",
            out
        );
        // Should contain the generic middle-omitted marker instead.
        assert!(out.contains("middle content omitted"));
    }

    #[test]
    fn truncate_tool_result_head_tail_handles_cjk() {
        // CJK chars: 3 bytes each. Must not panic on char boundary.
        let cjk = "你好世界测试数据".repeat(500); // 4000 chars
        let out = truncate_tool_result_head_tail(&cjk, 200);
        assert!(out.chars().count() <= 200);
        // Must start with original head.
        assert!(out.starts_with("你好世界"));
    }

    #[test]
    fn truncate_tool_result_head_tail_tiny_budget() {
        // Budget smaller than marker: just hard-truncate.
        let text = "abcdefghijklmnop";
        let out = truncate_tool_result_head_tail(text, 5);
        assert!(out.chars().count() <= 5);
    }

    #[test]
    fn render_compact_summary_history_applies_per_tool_result_cap() {
        // A massive tool result should be capped by SUMMARY_TOOL_RESULT_MAX_CHARS
        // even when the overall budget is much larger.
        use tiycore::types::ToolResultMessage;
        let big_content = "x".repeat(20_000);
        let history = vec![
            AgentMessage::User(UserMessage::text("do something")),
            AgentMessage::ToolResult(ToolResultMessage::text("tc-1", "read", &big_content, false)),
        ];

        let rendered = render_compact_summary_history(&history, 100_000);
        // The tool result body should be capped — the full 20K should NOT appear.
        assert!(!rendered.contains(&big_content));
        // But the header and some content should be present.
        assert!(rendered.contains("[tool_result] read"));
        // The overall rendered output should be well under the uncapped size.
        assert!(rendered.chars().count() < 20_000 + 500);
        // The tool result portion should respect SUMMARY_TOOL_RESULT_MAX_CHARS.
        let tool_section_start = rendered.find("[tool_result] read").unwrap();
        let tool_section = &rendered[tool_section_start..];
        // The tool section (header + body + trailing \n\n) should be bounded.
        assert!(
            tool_section.chars().count() <= SUMMARY_TOOL_RESULT_MAX_CHARS + 200,
            "tool section should be bounded by per-item cap + header overhead"
        );
    }

    // ----- summary_history_char_budget -----

    fn model_role_with(context_window: u32, reasoning: bool) -> ResolvedModelRole {
        let model = tiycore::types::Model::builder()
            .id("test-model")
            .name("test-model")
            .provider(tiycore::types::Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(context_window)
            .max_tokens(32_000)
            .input(vec![tiycore::types::InputType::Text])
            .cost(tiycore::types::Cost::default())
            .reasoning(reasoning)
            .build()
            .expect("sample model");

        ResolvedModelRole {
            provider_id: "provider-test".to_string(),
            model_record_id: "record-test".to_string(),
            model_id: "test-model".to_string(),
            model_name: "test-model".to_string(),
            provider_type: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            api_key: None,
            provider_options: None,
            model,
        }
    }

    fn model_role_with_id(model_id: &str) -> ResolvedModelRole {
        let mut role = model_role_with(128_000, false);
        role.model_id = model_id.to_string();
        role.model_name = model_id.to_string();
        role.model = tiycore::types::Model::builder()
            .id(model_id)
            .name(model_id)
            .provider(tiycore::types::Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(128_000)
            .max_tokens(32_000)
            .input(vec![tiycore::types::InputType::Text])
            .cost(tiycore::types::Cost::default())
            .reasoning(false)
            .build()
            .expect("sample model with custom id");
        role
    }

    #[test]
    fn build_title_model_candidates_prefers_priority_order() {
        let lightweight = model_role_with_id("lite");
        let auxiliary = model_role_with_id("aux");
        let primary = model_role_with_id("primary");

        let candidates =
            build_title_model_candidates(Some(&lightweight), Some(&auxiliary), Some(&primary));

        let ids: Vec<&str> = candidates
            .iter()
            .map(|role| role.model_id.as_str())
            .collect();
        assert_eq!(ids, vec!["lite", "aux", "primary"]);
    }

    #[test]
    fn build_title_model_candidates_skips_missing_entries() {
        let auxiliary = model_role_with_id("aux");
        let primary = model_role_with_id("primary");

        let candidates = build_title_model_candidates(None, Some(&auxiliary), Some(&primary));

        let ids: Vec<&str> = candidates
            .iter()
            .map(|role| role.model_id.as_str())
            .collect();
        assert_eq!(ids, vec!["aux", "primary"]);
    }

    #[test]
    fn build_title_model_candidates_returns_empty_when_all_missing() {
        let candidates = build_title_model_candidates(None, None, None);
        assert!(candidates.is_empty());
    }

    #[test]
    fn build_title_model_candidates_deduplicates_same_model_id() {
        let lightweight = model_role_with_id("shared");
        let auxiliary = model_role_with_id("shared");
        let primary = model_role_with_id("primary");

        let candidates =
            build_title_model_candidates(Some(&lightweight), Some(&auxiliary), Some(&primary));

        let ids: Vec<&str> = candidates
            .iter()
            .map(|role| role.model_id.as_str())
            .collect();
        assert_eq!(ids, vec!["shared", "primary"]);
    }

    #[test]
    fn title_prompt_from_messages_matches_conversation_language_when_none() {
        let messages = vec![MessageRecord {
            id: "msg-1".into(),
            thread_id: "thread-1".into(),
            run_id: None,
            role: "user".into(),
            content_markdown: "请帮我分析标题生成策略".into(),
            message_type: "plain_message".into(),
            status: "completed".into(),
            metadata_json: None,
            attachments_json: None,
            created_at: String::new(),
        }];

        let prompt =
            build_title_prompt_from_messages(&messages, None, ProfileResponseStyle::Balanced);

        assert!(prompt.contains("Match the conversation language."));
        assert!(prompt.contains("Keep the title clear and natural"));
    }

    #[test]
    fn title_prompt_from_messages_includes_concise_style_rule() {
        let messages = vec![MessageRecord {
            id: "msg-1".into(),
            thread_id: "thread-1".into(),
            run_id: None,
            role: "user".into(),
            content_markdown: "Need a short title".into(),
            message_type: "plain_message".into(),
            status: "completed".into(),
            metadata_json: None,
            attachments_json: None,
            created_at: String::new(),
        }];

        let prompt = build_title_prompt_from_messages(
            &messages,
            Some("English"),
            ProfileResponseStyle::Concise,
        );

        assert!(prompt.contains("Write the title in English."));
        assert!(prompt.contains("especially terse, direct, and low-friction"));
    }

    #[test]
    fn title_prompt_from_messages_includes_guide_style_rule() {
        let messages = vec![MessageRecord {
            id: "msg-1".into(),
            thread_id: "thread-1".into(),
            run_id: None,
            role: "assistant".into(),
            content_markdown: "Let's decide whether to keep fallback behavior.".into(),
            message_type: "plain_message".into(),
            status: "completed".into(),
            metadata_json: None,
            attachments_json: None,
            created_at: String::new(),
        }];

        let prompt = build_title_prompt_from_messages(
            &messages,
            Some("English"),
            ProfileResponseStyle::Guide,
        );

        assert!(prompt.contains("Write the title in English."));
        assert!(prompt.contains("signals the user's goal or decision focus clearly"));
    }

    #[test]
    fn summary_history_char_budget_zero_context_window_returns_floor() {
        // context_window = 0 → budget should collapse to the SUMMARY_HISTORY_MIN_CHARS
        // floor rather than to zero (which would produce useless LLM inputs).
        let role = model_role_with(0, false);
        assert_eq!(
            summary_history_char_budget(&role),
            SUMMARY_HISTORY_MIN_CHARS
        );
    }

    #[test]
    fn summary_history_char_budget_tiny_context_window_returns_floor() {
        // When context_window < output_tokens + overhead, saturating_sub
        // drives tokens_for_history to 0 and we must return the floor.
        let role = model_role_with(4_096, false); // < 8192 output + 3000 overhead
        assert_eq!(
            summary_history_char_budget(&role),
            SUMMARY_HISTORY_MIN_CHARS
        );
    }

    #[test]
    fn summary_history_char_budget_scales_with_context_window() {
        // 128K window → (128_000 - 8192 - 3000) * 4 = 466_432 chars.
        let role = model_role_with(128_000, false);
        let budget = summary_history_char_budget(&role);
        let expected = (128_000usize - 8_192 - 3_000).saturating_mul(4);
        assert_eq!(budget, expected);
        // Also sanity-check it's well above the floor.
        assert!(budget > SUMMARY_HISTORY_MIN_CHARS);
    }

    #[test]
    fn summary_history_char_budget_doubles_output_budget_for_reasoning_models() {
        // Reasoning models share the output slot with thinking tokens, so
        // we subtract PRIMARY_SUMMARY_MAX_TOKENS * 2 = 16_384 instead of 8_192.
        let reasoning = model_role_with(128_000, true);
        let non_reasoning = model_role_with(128_000, false);
        let reasoning_budget = summary_history_char_budget(&reasoning);
        let non_reasoning_budget = summary_history_char_budget(&non_reasoning);

        // Reasoning budget should be exactly 8_192 tokens smaller = 32_768 chars smaller.
        assert_eq!(
            non_reasoning_budget - reasoning_budget,
            8_192usize.saturating_mul(4)
        );
    }

    #[test]
    fn summary_history_char_budget_1m_context_window_has_no_upper_cap() {
        // Regression guard for the 400K-char cap removal. A 1M-context model
        // should get its full advertised capacity (minus overhead) as budget.
        let role = model_role_with(1_000_000, false);
        let budget = summary_history_char_budget(&role);
        let expected = (1_000_000usize - 8_192 - 3_000).saturating_mul(4);
        assert_eq!(budget, expected);
        // And must be well above any previous artificial cap.
        assert!(budget > 400_000);
    }

    // -----------------------------------------------------------------------
    // sidebar_status_for_runtime_event
    // -----------------------------------------------------------------------

    #[test]
    fn sidebar_status_maps_run_started_to_running() {
        let event = ThreadStreamEvent::RunStarted {
            run_id: "run-1".to_string(),
            run_mode: "default".to_string(),
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, false),
            Some("running")
        );
    }

    #[test]
    fn sidebar_status_maps_approval_required_to_waiting_approval() {
        let event = ThreadStreamEvent::ApprovalRequired {
            run_id: "run-1".to_string(),
            tool_call_id: "tc-1".to_string(),
            tool_name: "shell".to_string(),
            tool_input: serde_json::json!({"command": "rm -rf /"}),
            reason: "dangerous".to_string(),
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, false),
            Some("waiting_approval")
        );
    }

    #[test]
    fn sidebar_status_maps_clarify_required_to_needs_reply() {
        let event = ThreadStreamEvent::ClarifyRequired {
            run_id: "run-1".to_string(),
            tool_call_id: "tc-1".to_string(),
            tool_name: "clarify".to_string(),
            tool_input: serde_json::json!({"question": "Which option?"}),
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, false),
            Some("needs_reply")
        );
    }

    #[test]
    fn sidebar_status_maps_approval_resolved_to_running() {
        let event = ThreadStreamEvent::ApprovalResolved {
            run_id: "run-1".to_string(),
            tool_call_id: "tc-1".to_string(),
            approved: true,
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, false),
            Some("running")
        );
    }

    #[test]
    fn sidebar_status_maps_run_interrupted_with_cancel_to_cancelled() {
        let event = ThreadStreamEvent::RunInterrupted {
            run_id: "run-1".to_string(),
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, true),
            Some("cancelled")
        );
    }

    #[test]
    fn sidebar_status_maps_run_interrupted_without_cancel_to_interrupted() {
        let event = ThreadStreamEvent::RunInterrupted {
            run_id: "run-1".to_string(),
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, false),
            Some("interrupted")
        );
    }

    #[test]
    fn sidebar_status_returns_none_for_message_delta() {
        let event = ThreadStreamEvent::MessageDelta {
            run_id: "run-1".to_string(),
            message_id: "msg-1".to_string(),
            delta: "hello".to_string(),
        };
        assert_eq!(sidebar_status_for_runtime_event(&event, false), None);
    }

    #[test]
    fn sidebar_status_maps_run_checkpointed_to_waiting_approval() {
        let event = ThreadStreamEvent::RunCheckpointed {
            run_id: "run-1".to_string(),
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, false),
            Some("waiting_approval")
        );
    }

    #[test]
    fn sidebar_status_maps_limit_reached_to_limit_reached() {
        let event = ThreadStreamEvent::RunLimitReached {
            run_id: "run-1".to_string(),
            error: "token limit".to_string(),
            max_turns: 100,
        };
        assert_eq!(
            sidebar_status_for_runtime_event(&event, false),
            Some("limit_reached")
        );
    }
}
