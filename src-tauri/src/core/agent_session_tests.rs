#[cfg(test)]
pub(super) mod tests {
    use super::super::{
        build_initial_context_token_calibration, build_profile_response_prompt_parts,
        build_system_prompt, convert_history_messages, current_context_token_calibration,
        handle_agent_event, main_agent_security_config, normalize_profile_response_language,
        normalize_profile_response_style, plan_mode_missing_checkpoint_error,
        record_pending_prompt_estimate, resolve_helper_model_role, resolve_helper_profile,
        resolve_model_plan, resolve_runtime_model_role, response_style_system_instruction,
        runtime_security_config, runtime_tools_for_profile,
        runtime_tools_for_profile_with_extensions, standard_tool_timeout,
        trim_history_to_current_context, ContextCompressionRuntimeState, ProfileResponseStyle,
        ResolvedModelRole, ResolvedRuntimeModelPlan, RuntimeModelPlan, SortKey,
        DEFAULT_FULL_TOOL_PROFILE, MAIN_AGENT_TOOL_TIMEOUT_SECS,
        PLAN_MODE_MISSING_CHECKPOINT_ERROR, PLAN_READ_ONLY_TOOL_PROFILE,
        STANDARD_TOOL_TIMEOUT_SECS, SUBAGENT_TOOL_TIMEOUT_SECS,
    };
    use std::fs;
    use std::sync::Mutex as StdMutex;

    use tempfile::tempdir;
    use tiycore::agent::{AgentEvent, AgentMessage, AgentTool};
    use tiycore::thinking::ThinkingLevel;
    use tiycore::types::{
        Api, AssistantMessage, AssistantMessageEvent, ContentBlock, Provider, StopReason,
        TextContent,
    };
    use tokio::sync::mpsc;

    use crate::core::plan_checkpoint::{
        build_plan_artifact_from_tool_input, build_plan_message_metadata,
    };
    use crate::core::prompt::providers::{
        final_response_structure_system_instruction, run_mode_prompt_body,
    };
    use crate::core::subagent::{RuntimeOrchestrationTool, SubagentProfile};
    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use crate::model::provider::{AgentProfileRecord, ProviderKind, ProviderRecord};
    use crate::model::thread::{MessageRecord, RunSummaryDto, RunUsageDto, ToolCallDto};
    use crate::persistence::init_database;
    use crate::persistence::repo::provider_repo;

    const TEST_CONTEXT_WINDOW: &str = "128000";
    const TEST_MODEL_DISPLAY_NAME: &str = "GPT Test";

    fn sample_profile() -> AgentProfileRecord {
        AgentProfileRecord {
            id: "profile-1".to_string(),
            name: "Default".to_string(),
            custom_instructions: None,
            commit_message_prompt: None,
            response_style: Some("balanced".to_string()),
            response_language: Some("English".to_string()),
            commit_message_language: Some("English".to_string()),
            thinking_level: None,
            primary_provider_id: None,
            primary_model_id: None,
            auxiliary_provider_id: None,
            auxiliary_model_id: None,
            lightweight_provider_id: None,
            lightweight_model_id: None,
            is_default: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn sample_partial_assistant_message() -> AssistantMessage {
        AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .build()
            .expect("partial assistant message")
    }

    fn sample_resolved_model_role_with_inputs(
        model_id: &str,
        input: Vec<tiycore::types::InputType>,
    ) -> ResolvedModelRole {
        let model = tiycore::types::Model::builder()
            .id(model_id)
            .name(model_id)
            .provider(Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(128_000)
            .max_tokens(32_000)
            .input(input)
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
            api_key: Some("sk-test".to_string()),
            provider_options: None,
            model,
        }
    }

    fn sample_resolved_model_role(model_id: &str) -> ResolvedModelRole {
        sample_resolved_model_role_with_inputs(model_id, vec![tiycore::types::InputType::Text])
    }

    fn sample_runtime_model_role(
        provider_id: &str,
        model_record_id: &str,
        model_id: &str,
        supports_reasoning: Option<bool>,
    ) -> super::super::RuntimeModelRole {
        super::super::RuntimeModelRole {
            provider_id: provider_id.to_string(),
            model_record_id: model_record_id.to_string(),
            provider: None,
            provider_key: Some("openai".to_string()),
            provider_type: "openai".to_string(),
            provider_name: Some("OpenAI".to_string()),
            model: model_id.to_string(),
            model_id: model_id.to_string(),
            model_display_name: Some(model_id.to_string()),
            base_url: "https://api.openai.com/v1".to_string(),
            context_window: Some(TEST_CONTEXT_WINDOW.to_string()),
            max_output_tokens: Some("32000".to_string()),
            supports_image_input: Some(false),
            supports_reasoning,
            reasoning_content_constrained: None,
            custom_headers: None,
            provider_options: None,
        }
    }

    fn sample_provider_record(provider_id: &str) -> ProviderRecord {
        ProviderRecord {
            id: provider_id.to_string(),
            provider_kind: ProviderKind::Builtin,
            provider_key: "openai".to_string(),
            provider_type: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key_encrypted: Some("sk-test".to_string()),
            enabled: true,
            mapping_locked: true,
            custom_headers_json: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn sample_resolved_runtime_model_plan(
        auxiliary: Option<ResolvedModelRole>,
    ) -> ResolvedRuntimeModelPlan {
        ResolvedRuntimeModelPlan {
            raw: RuntimeModelPlan::default(),
            primary: sample_resolved_model_role("primary-model"),
            auxiliary,
            lightweight: None,
            thinking_level: ThinkingLevel::Off,
            transport: tiycore::types::Transport::Sse,
        }
    }

    fn make_history_message(id: &str, run_id: &str, role: &str, content: &str) -> MessageRecord {
        MessageRecord {
            id: id.to_string(),
            thread_id: "thread-1".to_string(),
            run_id: Some(run_id.to_string()),
            role: role.to_string(),
            content_markdown: content.to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: None,
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    fn make_run_summary(model_id: &str, input_tokens: u64) -> RunSummaryDto {
        make_run_summary_with_cache(model_id, input_tokens, 0)
    }

    fn make_run_summary_with_cache(
        model_id: &str,
        input_tokens: u64,
        cache_read_tokens: u64,
    ) -> RunSummaryDto {
        RunSummaryDto {
            id: "run-prev".to_string(),
            thread_id: "thread-1".to_string(),
            run_mode: "default".to_string(),
            status: "completed".to_string(),
            model_id: Some(model_id.to_string()),
            model_display_name: Some(model_id.to_string()),
            context_window: Some(TEST_CONTEXT_WINDOW.to_string()),
            error_message: None,
            started_at: "2026-01-01T00:00:00.000Z".to_string(),
            usage: RunUsageDto {
                input_tokens,
                output_tokens: 128,
                cache_read_tokens,
                cache_write_tokens: 0,
                total_tokens: input_tokens + cache_read_tokens + 128,
            },
        }
    }

    fn message_text(message: &AgentMessage) -> String {
        match message {
            AgentMessage::User(user) => match &user.content {
                tiycore::types::UserContent::Text(text) => text.clone(),
                tiycore::types::UserContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|block| match block {
                        tiycore::types::ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            },
            AgentMessage::Assistant(assistant) => assistant.text_content(),
            _ => String::new(),
        }
    }

    fn user_blocks(message: &AgentMessage) -> &[ContentBlock] {
        match message {
            AgentMessage::User(user) => match &user.content {
                tiycore::types::UserContent::Blocks(blocks) => blocks,
                _ => panic!("expected block-based user message"),
            },
            _ => panic!("expected user message"),
        }
    }

    fn handle_test_agent_event(
        run_id: &str,
        event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
        current_message_id: &StdMutex<Option<String>>,
        current_reasoning_message_id: &StdMutex<Option<String>>,
        last_usage: &StdMutex<Option<tiycore::types::Usage>>,
        reasoning_buffer: &StdMutex<String>,
        event: &AgentEvent,
    ) {
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        handle_test_agent_event_with_context_state(
            run_id,
            event_tx,
            current_message_id,
            current_reasoning_message_id,
            last_usage,
            &context_compression_state,
            reasoning_buffer,
            event,
        );
    }

    fn handle_test_agent_event_with_context_state(
        run_id: &str,
        event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
        current_message_id: &StdMutex<Option<String>>,
        current_reasoning_message_id: &StdMutex<Option<String>>,
        last_usage: &StdMutex<Option<tiycore::types::Usage>>,
        context_compression_state: &StdMutex<ContextCompressionRuntimeState>,
        reasoning_buffer: &StdMutex<String>,
        event: &AgentEvent,
    ) {
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_turn_index = StdMutex::new(None::<usize>);
        handle_agent_event(
            run_id,
            event_tx,
            current_message_id,
            &last_completed_message_id,
            current_reasoning_message_id,
            last_usage,
            context_compression_state,
            reasoning_buffer,
            &current_turn_index,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            event,
        );
    }

    #[test]
    fn test_runtime_security_config_extends_helper_tool_timeout() {
        let security = runtime_security_config();

        assert_eq!(
            security.agent.tool_execution_timeout_secs,
            SUBAGENT_TOOL_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_main_agent_security_config_uses_large_timeout() {
        let security = main_agent_security_config();

        assert_eq!(
            security.agent.tool_execution_timeout_secs,
            MAIN_AGENT_TOOL_TIMEOUT_SECS
        );
        // Main agent timeout must be much larger than subagent timeout
        // to avoid killing user-interactive tools like clarify/approval.
        assert!(MAIN_AGENT_TOOL_TIMEOUT_SECS > SUBAGENT_TOOL_TIMEOUT_SECS);
    }

    #[test]
    fn test_standard_tool_timeout_remains_120_seconds() {
        assert_eq!(
            standard_tool_timeout().as_secs(),
            STANDARD_TOOL_TIMEOUT_SECS
        );
    }

    #[test]
    fn profile_response_language_is_trimmed() {
        assert_eq!(
            normalize_profile_response_language(Some("  简体中文  ")).as_deref(),
            Some("简体中文")
        );
        assert_eq!(normalize_profile_response_language(Some("   ")), None);
    }

    #[test]
    fn profile_response_style_defaults_to_balanced() {
        assert_eq!(
            normalize_profile_response_style(Some("guide")),
            ProfileResponseStyle::Guide
        );
        assert_eq!(
            normalize_profile_response_style(Some("concise")),
            ProfileResponseStyle::Concise
        );
        assert_eq!(
            normalize_profile_response_style(Some("unknown")),
            ProfileResponseStyle::Balanced
        );
    }

    #[test]
    fn profile_prompt_parts_include_language_and_style() {
        let mut profile = sample_profile();
        profile.response_language = Some("Japanese".to_string());
        profile.response_style = Some("concise".to_string());

        let parts = build_profile_response_prompt_parts(&profile);

        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("Japanese"));
        assert_eq!(
            parts[1],
            response_style_system_instruction(ProfileResponseStyle::Concise)
        );
    }

    #[test]
    fn response_style_instructions_have_stronger_behavioral_separation() {
        let balanced = response_style_system_instruction(ProfileResponseStyle::Balanced);
        let concise = response_style_system_instruction(ProfileResponseStyle::Concise);
        let guide = response_style_system_instruction(ProfileResponseStyle::Guide);

        assert!(balanced.contains("compact but complete answer"));
        assert!(concise.contains("1-3 short sentences"));
        assert!(concise.contains("hard default"));
        assert!(guide.contains("tradeoffs"));
        assert!(guide.contains("recommended next steps"));
    }

    #[test]
    fn final_response_structure_instruction_matches_task_types_and_markdown_hierarchy() {
        let instruction = final_response_structure_system_instruction();

        assert!(instruction.contains("at most two heading levels"));
        assert!(instruction.contains("avoid turning every sub-point into its own heading"));
        assert!(instruction.contains("Debug or problem analysis"));
        assert!(instruction.contains("Code change or result report"));
        assert!(instruction.contains("Comparison or decision support"));
        assert!(instruction.contains("Direct explanation or question answering"));
        assert!(instruction.contains("structured Markdown presentation"));
        assert!(instruction.contains("Do not overload the reply with inline code formatting"));
    }

    #[test]
    fn final_response_structure_section_is_distinct_from_response_style_rules() {
        let section = format!(
            "## Final Response Structure\n{}",
            final_response_structure_system_instruction()
        );
        let balanced = response_style_system_instruction(ProfileResponseStyle::Balanced);

        assert!(section.starts_with("## Final Response Structure"));
        assert!(section.contains("For simple tasks, you may compress the structure"));
        assert!(balanced.contains("compact but complete answer"));
        assert!(!balanced.contains("reason 1, 2, and 3"));
    }

    #[test]
    fn run_mode_prompt_clarifies_terminal_panel_scope() {
        let plan_prompt = run_mode_prompt_body("plan");
        let default_prompt = run_mode_prompt_body("default");

        assert!(plan_prompt.contains("embedded Terminal panel"));
        assert!(plan_prompt.contains("update_plan"));
        assert!(plan_prompt.contains("pauses for user approval"));
        assert!(plan_prompt.contains("do not inspect your own runtime"));
        assert!(default_prompt.contains("embedded Terminal panel"));
        assert!(default_prompt.contains("do not inspect your own runtime"));
    }

    #[test]
    fn default_full_profile_exposes_mutating_terminal_tools() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"term_write"));
        assert!(tool_names.contains(&"term_restart"));
        assert!(tool_names.contains(&"term_close"));
    }

    #[test]
    fn plan_read_only_profile_includes_shell_excludes_mutating_terminal_tools() {
        let tools = runtime_tools_for_profile(PLAN_READ_ONLY_TOOL_PROFILE);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        // Shell is available in plan mode (follows normal approval policy).
        assert!(tool_names.contains(&"shell"));
        // Write-oriented terminal tools are excluded.
        assert!(!tool_names.contains(&"term_write"));
        assert!(!tool_names.contains(&"term_restart"));
        assert!(!tool_names.contains(&"term_close"));
    }

    #[test]
    fn runtime_file_tools_expose_window_and_limit_parameters() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);

        let read_tool = tools
            .iter()
            .find(|tool| tool.name == "read")
            .expect("read tool should exist");
        let list_tool = tools
            .iter()
            .find(|tool| tool.name == "list")
            .expect("list tool should exist");
        let find_tool = tools
            .iter()
            .find(|tool| tool.name == "find")
            .expect("find tool should exist");

        let read_properties = read_tool.parameters["properties"]
            .as_object()
            .expect("read properties should be object");
        let list_properties = list_tool.parameters["properties"]
            .as_object()
            .expect("list properties should be object");
        let find_properties = find_tool.parameters["properties"]
            .as_object()
            .expect("find properties should be object");

        assert!(read_properties.contains_key("offset"));
        assert!(read_properties.contains_key("limit"));
        assert!(list_properties.contains_key("limit"));
        assert!(find_properties.contains_key("limit"));
    }

    #[tokio::test]
    async fn system_prompt_delegates_post_implementation_verification_to_review_helper() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace_root = temp_dir.path().join("workspace");
        fs::create_dir(&workspace_root).expect("workspace dir");

        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");

        let prompt = build_system_prompt(
            &pool,
            &RuntimeModelPlan::default(),
            workspace_root.to_string_lossy().as_ref(),
            "default",
        )
        .await
        .expect("system prompt");

        assert!(prompt.contains(
            "review helper is responsible for running the necessary type-check and test commands"
        ));
        assert!(prompt.contains(
            "Do not rerun the same verification commands yourself unless the helper explicitly could not run them"
        ));
    }

    #[tokio::test]
    async fn system_prompt_includes_enabled_workspace_skills() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace_root = temp_dir.path().join("workspace");
        let skill_dir = workspace_root.join(".tiy/skills/test-skill");
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: test-skill
description: "Helps with local skill prompt injection tests."
---

# Test Skill

Used for prompt assembly coverage.
"#,
        )
        .expect("write skill");

        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");

        let prompt = build_system_prompt(
            &pool,
            &RuntimeModelPlan::default(),
            workspace_root.to_string_lossy().as_ref(),
            "default",
        )
        .await
        .expect("system prompt");

        assert!(prompt.contains("## Skills"));
        assert!(prompt.contains("### Available skills"));
        assert!(prompt.contains("test-skill: Helps with local skill prompt injection tests."));
        // The prompt path is built by joining workspace_path with ".tiy/skills" and then
        // reading child dirs, which may mix separators on Windows.  Match the production
        // path construction instead of canonicalizing.
        let expected_skill_path =
            std::path::Path::new(&workspace_root.to_string_lossy().into_owned())
                .join(".tiy/skills")
                .join("test-skill")
                .join("SKILL.md");
        assert!(
            prompt.contains(&expected_skill_path.display().to_string()),
            "prompt does not contain skill path.\nExpected: {}\nPrompt skills line: {}",
            expected_skill_path.display(),
            prompt
                .lines()
                .find(|l| l.contains("test-skill"))
                .unwrap_or("(not found)")
        );
        assert!(prompt.contains("### How to use skills"));
    }

    #[tokio::test]
    async fn system_prompt_includes_query_task_recovery_guidance() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace_root = temp_dir.path().join("workspace");
        fs::create_dir(&workspace_root).expect("workspace dir");

        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");

        let prompt = build_system_prompt(
            &pool,
            &RuntimeModelPlan::default(),
            workspace_root.to_string_lossy().as_ref(),
            "default",
        )
        .await
        .expect("system prompt");

        assert!(prompt.contains("call `query_task` first"));
        assert!(prompt.contains("call `query_task` with `scope='active'`"));
        assert!(prompt.contains("Use `query_task` with `scope='all'` only"));
    }

    #[test]
    fn reasoning_blocks_reset_message_id_between_thought_segments() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let partial = sample_partial_assistant_message();

        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                turn_index: 0,
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 0,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                turn_index: 0,
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingDelta {
                    content_index: 0,
                    delta: "first thought".to_string(),
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                turn_index: 0,
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingEnd {
                    content_index: 0,
                    content: "first thought".to_string(),
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                turn_index: 0,
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 1,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                turn_index: 0,
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingDelta {
                    content_index: 1,
                    delta: "second thought".to_string(),
                    partial,
                }),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        let reasoning_events = events
            .into_iter()
            .filter_map(|event| match event {
                ThreadStreamEvent::ReasoningUpdated {
                    message_id,
                    reasoning,
                    ..
                } => Some((message_id, reasoning)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(reasoning_events.len(), 3);
        assert_eq!(reasoning_events[0].1, "first thought");
        assert_eq!(reasoning_events[1].1, "first thought");
        assert_eq!(reasoning_events[2].1, "second thought");
        assert_eq!(reasoning_events[0].0, reasoning_events[1].0);
        assert_ne!(reasoning_events[0].0, reasoning_events[2].0);
    }

    #[test]
    fn empty_reasoning_blocks_do_not_emit_reasoning_events() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let partial = sample_partial_assistant_message();

        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                turn_index: 0,
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 0,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                turn_index: 0,
                message: AgentMessage::Assistant(partial),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingEnd {
                    content_index: 0,
                    content: String::new(),
                    partial: sample_partial_assistant_message(),
                }),
            },
        );

        let reasoning_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|event| matches!(event, ThreadStreamEvent::ReasoningUpdated { .. }))
            .collect::<Vec<_>>();

        assert!(reasoning_events.is_empty());
    }

    #[test]
    fn message_end_emits_usage_updates_once_per_change() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiycore::types::Usage::from_tokens(256, 32))
            .build()
            .expect("assistant message with usage");

        handle_test_agent_event(
            "run-usage",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                turn_index: 0,
                response_id: None,
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_test_agent_event(
            "run-usage",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                turn_index: 0,
                response_id: None,
                message: AgentMessage::Assistant(assistant),
            },
        );

        let usage_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter_map(|event| match event {
                ThreadStreamEvent::ThreadUsageUpdated { usage, .. } => Some(usage),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(usage_events.len(), 1);
        assert_eq!(usage_events[0].input_tokens, 256);
        assert_eq!(usage_events[0].output_tokens, 32);
        assert_eq!(usage_events[0].total_tokens, 288);
    }

    #[test]
    fn message_end_usage_updates_consume_pending_prompt_estimate_once() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiycore::types::Usage::from_tokens(1_500, 32))
            .build()
            .expect("assistant message with usage");

        record_pending_prompt_estimate(&context_compression_state, 1_000);
        handle_test_agent_event_with_context_state(
            "run-usage-calibration",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                turn_index: 0,
                response_id: None,
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_test_agent_event_with_context_state(
            "run-usage-calibration",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                turn_index: 0,
                response_id: None,
                message: AgentMessage::Assistant(assistant),
            },
        );

        let usage_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|event| matches!(event, ThreadStreamEvent::ThreadUsageUpdated { .. }))
            .count();
        let calibration = current_context_token_calibration(&context_compression_state);

        assert_eq!(usage_events, 1);
        assert_eq!(calibration.ratio_basis_points(), 15_000);
        assert!(context_compression_state
            .lock()
            .expect("context compression state")
            .pending_prompt_estimate
            .is_none());
    }

    #[test]
    fn usage_calibration_counts_cache_read_when_input_is_zero() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        let current_turn_index = StdMutex::new(None::<usize>);
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiycore::types::Usage {
                input: 0,
                output: 32,
                cache_read: 1_500,
                cache_write: 0,
                total_tokens: 1_532,
                cost: tiycore::types::UsageCost::default(),
            })
            .build()
            .expect("assistant message with cache-read usage");

        record_pending_prompt_estimate(&context_compression_state, 1_000);
        handle_agent_event(
            "run-cache-read-calibration",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &current_turn_index,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageEnd {
                turn_index: 0,
                response_id: None,
                message: AgentMessage::Assistant(assistant),
            },
        );

        let usage_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|event| matches!(event, ThreadStreamEvent::ThreadUsageUpdated { .. }))
            .count();
        let calibration = current_context_token_calibration(&context_compression_state);

        assert_eq!(usage_events, 1);
        assert_eq!(calibration.ratio_basis_points(), 15_000);
        assert!(context_compression_state
            .lock()
            .expect("context compression state")
            .pending_prompt_estimate
            .is_none());
    }

    #[test]
    fn build_initial_context_token_calibration_seeds_from_matching_historical_run() {
        let primary_model = sample_resolved_model_role("primary-model");
        let history_messages = vec![
            make_history_message("msg-1", "run-prev", "user", &"x".repeat(600)),
            make_history_message("msg-2", "run-prev", "assistant", &"y".repeat(600)),
        ];
        let history = convert_history_messages(&history_messages, &[], &primary_model.model);
        let estimated_tokens = crate::core::context_compression::estimate_total_tokens(&history);
        let run_summary = make_run_summary("primary-model", (estimated_tokens as u64) * 2);

        let calibration = build_initial_context_token_calibration(
            Some(&run_summary),
            &history_messages,
            &[],
            &primary_model,
            "",
        );

        assert_eq!(calibration.ratio_basis_points(), 20_000);
        assert_eq!(
            calibration.apply_to_estimate(estimated_tokens),
            estimated_tokens * 2
        );
    }

    #[test]
    fn build_initial_context_token_calibration_counts_cache_read_tokens() {
        let primary_model = sample_resolved_model_role("primary-model");
        let history_messages = vec![
            make_history_message("msg-1", "run-prev", "user", &"x".repeat(600)),
            make_history_message("msg-2", "run-prev", "assistant", &"y".repeat(600)),
        ];
        let history = convert_history_messages(&history_messages, &[], &primary_model.model);
        let estimated_tokens = crate::core::context_compression::estimate_total_tokens(&history);
        let run_summary = make_run_summary_with_cache(
            "primary-model",
            estimated_tokens as u64 / 2,
            estimated_tokens as u64 * 3 / 2,
        );

        let calibration = build_initial_context_token_calibration(
            Some(&run_summary),
            &history_messages,
            &[],
            &primary_model,
            "",
        );

        assert_eq!(calibration.ratio_basis_points(), 20_000);
        assert_eq!(
            calibration.apply_to_estimate(estimated_tokens),
            estimated_tokens * 2
        );
    }

    #[test]
    fn build_initial_context_token_calibration_ignores_mismatched_models_and_zero_usage() {
        let primary_model = sample_resolved_model_role("primary-model");
        let history_messages = vec![make_history_message(
            "msg-1",
            "run-prev",
            "user",
            &"x".repeat(400),
        )];

        let mismatched = build_initial_context_token_calibration(
            Some(&make_run_summary("other-model", 4_096)),
            &history_messages,
            &[],
            &primary_model,
            "",
        );
        let zero_usage = build_initial_context_token_calibration(
            Some(&make_run_summary("primary-model", 0)),
            &history_messages,
            &[],
            &primary_model,
            "",
        );

        assert_eq!(mismatched.ratio_basis_points(), 10_000);
        assert_eq!(zero_usage.ratio_basis_points(), 10_000);
    }

    #[test]
    fn turn_retrying_event_emits_runtime_retry_notice() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        let current_turn_index = StdMutex::new(None::<usize>);

        handle_agent_event(
            "run-retry",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &current_turn_index,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::TurnRetrying {
                attempt: 1,
                max_attempts: 3,
                delay_ms: 1_000,
                reason: "Incomplete anthropic stream: missing message_stop".to_string(),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        assert!(matches!(
            events.as_slice(),
            [ThreadStreamEvent::RunRetrying {
                run_id,
                attempt: 1,
                max_attempts: 3,
                delay_ms: 1_000,
                reason,
            }] if run_id == "run-retry" && reason.contains("Incomplete anthropic stream")
        ));
    }

    #[test]
    fn message_end_empty_content_no_tool_calls_skips_message_completed() {
        // When a provider error interrupts the stream before any text is
        // generated, MessageEnd arrives with empty text_content() and no
        // tool calls.  The handler must NOT emit MessageCompleted —
        // otherwise an empty plain_message record poisons the DB and
        // causes DeepSeek 400 errors on the next run.
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        let current_turn_index = StdMutex::new(None::<usize>);

        // Empty assistant: no content blocks, no tool calls.
        let empty_assistant = sample_partial_assistant_message();

        handle_agent_event(
            "run-empty",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &current_turn_index,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageEnd {
                turn_index: 0,
                response_id: None,
                message: AgentMessage::Assistant(empty_assistant),
            },
        );

        let events: Vec<_> = std::iter::from_fn(|| event_rx.try_recv().ok()).collect();
        // No MessageCompleted should have been emitted.
        let has_message_completed = events
            .iter()
            .any(|e| matches!(e, ThreadStreamEvent::MessageCompleted { .. }));
        assert!(
            !has_message_completed,
            "MessageCompleted should NOT be emitted for empty assistant without tool calls, got: {:?}",
            events
        );
    }

    #[test]
    fn message_discarded_reuses_last_completed_assistant_message_id() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        let current_turn_index = StdMutex::new(None::<usize>);
        // Build an assistant message with actual text content so that
        // MessageEnd emits MessageCompleted (empty content is now skipped).
        let assistant = AssistantMessage::builder()
            .content(vec![ContentBlock::Text(TextContent::new(
                "Here is the answer.",
            ))])
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .build()
            .expect("partial assistant message with content");

        handle_agent_event(
            "run-discard",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &current_turn_index,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageEnd {
                turn_index: 0,
                response_id: None,
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_agent_event(
            "run-discard",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &current_turn_index,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageDiscarded {
                turn_index: 0,
                message: AgentMessage::Assistant(assistant),
                reason: "Incomplete anthropic stream: missing message_stop".to_string(),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        let completed_message_id = events.iter().find_map(|event| match event {
            ThreadStreamEvent::MessageCompleted { message_id, .. } => Some(message_id.clone()),
            _ => None,
        });
        let discarded_message_id = events.iter().find_map(|event| match event {
            ThreadStreamEvent::MessageDiscarded { message_id, .. } => Some(message_id.clone()),
            _ => None,
        });

        assert!(completed_message_id.is_some());
        assert_eq!(completed_message_id, discarded_message_id);
    }

    #[test]
    fn helper_profiles_match_explore_and_review_tools() {
        assert_eq!(
            resolve_helper_profile(RuntimeOrchestrationTool::Explore),
            SubagentProfile::Explore,
        );
        assert_eq!(
            resolve_helper_profile(RuntimeOrchestrationTool::Review),
            SubagentProfile::Review,
        );
    }

    #[test]
    fn update_plan_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"update_plan"));
        }
    }

    #[test]
    fn clarify_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"clarify"));
        }
    }

    #[test]
    fn query_task_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"query_task"));
        }
    }

    #[test]
    fn query_task_tool_schema_defaults_to_active_and_supports_all_scope() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let query_task = tools
            .iter()
            .find(|tool| tool.name == "query_task")
            .expect("query_task tool should exist");
        let scope = &query_task.parameters["properties"]["scope"];
        let scope_enum = scope["enum"]
            .as_array()
            .expect("query_task scope enum should be present");
        let description = scope["description"]
            .as_str()
            .expect("query_task scope description should be present");

        assert_eq!(scope_enum.len(), 2);
        assert_eq!(scope_enum[0], "active");
        assert_eq!(scope_enum[1], "all");
        assert!(description.contains("Defaults to `active`"));
    }

    #[test]
    fn runtime_tools_merge_extension_tools_without_overriding_builtin_names() {
        let tools = runtime_tools_for_profile_with_extensions(
            DEFAULT_FULL_TOOL_PROFILE,
            vec![
                AgentTool::new(
                    "__mcp_context7_resolve-library-id",
                    "resolve-library-id",
                    "Context7 MCP tool",
                    serde_json::json!({ "type": "object" }),
                ),
                AgentTool::new(
                    "read",
                    "Read",
                    "should not override builtin tool",
                    serde_json::json!({ "type": "object" }),
                ),
            ],
        );
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"__mcp_context7_resolve-library-id"));
        assert_eq!(tool_names.iter().filter(|name| **name == "read").count(), 1);
    }

    #[test]
    fn update_plan_tool_schema_no_longer_exposes_open_questions() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");
        let properties = update_plan.parameters["properties"]
            .as_object()
            .expect("update_plan properties should be object");

        assert!(!properties.contains_key("openQuestions"));
    }

    #[test]
    fn update_plan_tool_schema_exposes_structured_plan_sections() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");
        let properties = update_plan.parameters["properties"]
            .as_object()
            .expect("update_plan properties should be object");

        assert!(properties.contains_key("context"));
        assert!(properties.contains_key("design"));
        assert!(properties.contains_key("keyImplementation"));
        assert!(properties.contains_key("verification"));
        assert!(properties.contains_key("assumptions"));

        let nested_plan_properties = update_plan.parameters["properties"]["plan"]["properties"]
            .as_object()
            .expect("nested plan properties should be object");
        assert!(nested_plan_properties.contains_key("context"));
        assert!(nested_plan_properties.contains_key("design"));
        assert!(nested_plan_properties.contains_key("keyImplementation"));
        assert!(nested_plan_properties.contains_key("verification"));
        assert!(nested_plan_properties.contains_key("assumptions"));
    }

    #[test]
    fn update_plan_tool_description_contains_workflow_and_quality_contract() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");

        // Workflow phases
        assert!(update_plan.description.contains("Phase 1"));
        assert!(update_plan.description.contains("Explore and understand"));
        assert!(update_plan.description.contains("Phase 2"));
        assert!(update_plan.description.contains("Clarify ambiguities"));
        assert!(update_plan.description.contains("Phase 3"));
        assert!(update_plan
            .description
            .contains("Converge on a recommendation"));
        assert!(update_plan.description.contains("Phase 4"));
        // Quality contract
        assert!(update_plan.description.contains("Quality contract"));
        assert!(update_plan.description.contains("keyImplementation"));
        assert!(update_plan.description.contains("verification"));
        assert!(update_plan.description.contains("Prohibited"));
        assert!(update_plan
            .description
            .contains("incrementally refine the plan"));
    }

    #[test]
    fn explore_and_review_use_auxiliary_model_when_available() {
        let model_plan =
            sample_resolved_runtime_model_plan(Some(sample_resolved_model_role("assistant-model")));

        let explore_role =
            resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Explore);
        let review_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Review);

        assert_eq!(explore_role.model_id, "assistant-model");
        assert_eq!(review_role.model_id, "assistant-model");
    }

    #[test]
    fn explore_and_review_fallback_to_primary_without_auxiliary_model() {
        let model_plan = sample_resolved_runtime_model_plan(None);

        let explore_role =
            resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Explore);
        let review_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Review);

        assert_eq!(explore_role.model_id, "primary-model");
        assert_eq!(review_role.model_id, "primary-model");
    }

    #[tokio::test]
    async fn runtime_model_role_uses_declared_reasoning_capability() {
        let temp_dir = tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");
        provider_repo::insert(&pool, &sample_provider_record("provider-reasoning"))
            .await
            .expect("provider insert");

        let capable = resolve_runtime_model_role(
            &pool,
            sample_runtime_model_role(
                "provider-reasoning",
                "record-capable",
                "capable",
                Some(true),
            ),
        )
        .await
        .expect("capable role");
        let incapable = resolve_runtime_model_role(
            &pool,
            sample_runtime_model_role(
                "provider-reasoning",
                "record-incapable",
                "incapable",
                Some(false),
            ),
        )
        .await
        .expect("incapable role");
        let unspecified = resolve_runtime_model_role(
            &pool,
            sample_runtime_model_role(
                "provider-reasoning",
                "record-unspecified",
                "unspecified",
                None,
            ),
        )
        .await
        .expect("unspecified role");

        assert!(capable.model.reasoning);
        assert!(!incapable.model.reasoning);
        assert!(!unspecified.model.reasoning);
    }

    #[tokio::test]
    async fn reasoning_content_constrained_sets_compat_flag() {
        let temp_dir = tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");

        let mut compat_provider = sample_provider_record("provider-compat");
        compat_provider.provider_type = "openai-compatible".to_string();
        provider_repo::insert(&pool, &compat_provider)
            .await
            .expect("provider insert");

        let make_role = |record_id: &str, model_id: &str, constrained: Option<bool>| {
            super::super::RuntimeModelRole {
                provider_id: "provider-compat".to_string(),
                model_record_id: record_id.to_string(),
                provider: None,
                provider_key: Some("openai-compatible".to_string()),
                provider_type: "openai-compatible".to_string(),
                provider_name: Some("Custom Gateway".to_string()),
                model: model_id.to_string(),
                model_id: model_id.to_string(),
                model_display_name: Some(model_id.to_string()),
                base_url: "https://api.example.com/v1".to_string(),
                context_window: Some(TEST_CONTEXT_WINDOW.to_string()),
                max_output_tokens: Some("32000".to_string()),
                supports_image_input: Some(false),
                supports_reasoning: Some(true),
                reasoning_content_constrained: constrained,
                custom_headers: None,
                provider_options: None,
            }
        };

        let constrained = resolve_runtime_model_role(
            &pool,
            make_role("rec-constrained", "constrained-model", Some(true)),
        )
        .await
        .expect("constrained role");

        let unconstrained = resolve_runtime_model_role(
            &pool,
            make_role("rec-unconstrained", "unconstrained-model", Some(false)),
        )
        .await
        .expect("unconstrained role");

        let unspecified = resolve_runtime_model_role(
            &pool,
            make_role("rec-unspecified", "unspecified-model", None),
        )
        .await
        .expect("unspecified role");

        // When reasoning_content_constrained is Some(true), the compat flag must be set.
        let constrained_compat = constrained.model.compat.as_ref().expect("compat present");
        assert!(constrained_compat.reasoning_content_constrained);

        // When Some(false) or None, the compat flag must remain at its default (false).
        let unconstrained_compat = unconstrained.model.compat.as_ref().expect("compat present");
        assert!(!unconstrained_compat.reasoning_content_constrained);

        let unspecified_compat = unspecified.model.compat.as_ref().expect("compat present");
        assert!(!unspecified_compat.reasoning_content_constrained);
    }

    #[tokio::test]
    async fn reasoning_content_constrained_sets_compat_flag_for_non_openai_compatible_providers() {
        // Regression: Zenmux (provider_type != "openai-compatible") must still
        // propagate reasoning_content_constrained so that tiycore's
        // normalize_reasoning_content safety net activates for DeepSeek models
        // routed through Zenmux.
        let temp_dir = tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");

        let mut zenmux_provider = sample_provider_record("provider-zenmux");
        zenmux_provider.provider_type = "zenmux".to_string();
        provider_repo::insert(&pool, &zenmux_provider)
            .await
            .expect("provider insert");

        let make_role =
            |record_id: &str, constrained: Option<bool>| super::super::RuntimeModelRole {
                provider_id: "provider-zenmux".to_string(),
                model_record_id: record_id.to_string(),
                provider: None,
                provider_key: Some("zenmux".to_string()),
                provider_type: "zenmux".to_string(),
                provider_name: Some("ZenMux".to_string()),
                model: "deepseek/deepseek-v4-pro:deepseek".to_string(),
                model_id: "deepseek/deepseek-v4-pro:deepseek".to_string(),
                model_display_name: Some("DeepSeek V4 Pro".to_string()),
                base_url: "https://zenmux.ai/api/v1".to_string(),
                context_window: Some(TEST_CONTEXT_WINDOW.to_string()),
                max_output_tokens: Some("32000".to_string()),
                supports_image_input: Some(false),
                supports_reasoning: Some(true),
                reasoning_content_constrained: constrained,
                custom_headers: None,
                provider_options: None,
            };

        let constrained =
            resolve_runtime_model_role(&pool, make_role("rec-zenmux-constrained", Some(true)))
                .await
                .expect("constrained role");

        let unconstrained =
            resolve_runtime_model_role(&pool, make_role("rec-zenmux-unconstrained", Some(false)))
                .await
                .expect("unconstrained role");

        let unspecified =
            resolve_runtime_model_role(&pool, make_role("rec-zenmux-unspecified", None))
                .await
                .expect("unspecified role");

        // Zenmux + reasoning_content_constrained=true must produce a compat with the flag set.
        let constrained_compat = constrained
            .model
            .compat
            .as_ref()
            .expect("compat must be set");
        assert!(
            constrained_compat.reasoning_content_constrained,
            "reasoning_content_constrained must be true for Zenmux+DeepSeek"
        );

        // When false or None, compat should be None (no override needed).
        assert!(
            unconstrained.model.compat.is_none(),
            "compat should be None when reasoning_content_constrained is false"
        );
        assert!(
            unspecified.model.compat.is_none(),
            "compat should be None when reasoning_content_constrained is unspecified"
        );
    }

    #[tokio::test]
    async fn model_plan_applies_thinking_level_to_all_reasoning_capable_roles_only() {
        let temp_dir = tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");
        provider_repo::insert(&pool, &sample_provider_record("provider-plan"))
            .await
            .expect("provider insert");

        let plan = RuntimeModelPlan {
            primary: Some(sample_runtime_model_role(
                "provider-plan",
                "record-primary",
                "primary",
                Some(true),
            )),
            auxiliary: Some(sample_runtime_model_role(
                "provider-plan",
                "record-auxiliary",
                "auxiliary",
                Some(true),
            )),
            lightweight: Some(sample_runtime_model_role(
                "provider-plan",
                "record-lightweight",
                "lightweight",
                Some(false),
            )),
            thinking_level: Some("medium".to_string()),
            ..RuntimeModelPlan::default()
        };

        let resolved = resolve_model_plan(&pool, plan).await.expect("model plan");

        assert!(resolved.primary.model.reasoning);
        assert!(resolved.auxiliary.as_ref().unwrap().model.reasoning);
        assert!(!resolved.lightweight.as_ref().unwrap().model.reasoning);
    }

    #[tokio::test]
    async fn model_plan_keeps_reasoning_disabled_when_thinking_level_is_off() {
        let temp_dir = tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");
        provider_repo::insert(&pool, &sample_provider_record("provider-off"))
            .await
            .expect("provider insert");

        let plan = RuntimeModelPlan {
            primary: Some(sample_runtime_model_role(
                "provider-off",
                "record-primary",
                "primary",
                Some(true),
            )),
            auxiliary: Some(sample_runtime_model_role(
                "provider-off",
                "record-auxiliary",
                "auxiliary",
                Some(true),
            )),
            thinking_level: Some("off".to_string()),
            ..RuntimeModelPlan::default()
        };

        let resolved = resolve_model_plan(&pool, plan).await.expect("model plan");

        assert!(!resolved.primary.model.reasoning);
        assert!(!resolved.auxiliary.as_ref().unwrap().model.reasoning);
    }

    #[test]
    fn helper_model_role_preserves_auxiliary_reasoning_capability() {
        let mut auxiliary = sample_resolved_model_role("assistant-model");
        auxiliary.model.reasoning = true;
        let mut model_plan = sample_resolved_runtime_model_plan(Some(auxiliary));
        model_plan.thinking_level = ThinkingLevel::Medium;

        let helper_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Explore);

        assert_eq!(helper_role.model_id, "assistant-model");
        assert!(helper_role.model.reasoning);
    }

    #[test]
    fn helper_model_role_keeps_non_reasoning_auxiliary_disabled() {
        let mut auxiliary = sample_resolved_model_role("assistant-model");
        auxiliary.model.reasoning = false;
        let mut model_plan = sample_resolved_runtime_model_plan(Some(auxiliary));
        model_plan.thinking_level = ThinkingLevel::Medium;

        let helper_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Review);

        assert_eq!(helper_role.model_id, "assistant-model");
        assert!(!helper_role.model.reasoning);
    }

    #[test]
    fn plan_mode_prompt_mentions_waiting_for_approval_after_update_plan() {
        let prompt = run_mode_prompt_body("plan");

        assert!(prompt.contains("clarify"));
        assert!(prompt.contains("does NOT complete the run"));
        assert!(prompt.contains("must call update_plan"));
        assert!(prompt.contains("Unresolved core ambiguities pushed to the approval step"));
        assert!(prompt.contains("Once published, the run pauses for user approval"));
        assert!(prompt.contains("`design`: Write a detailed prose description"));
        assert!(prompt.contains("`verification`: Write a thorough description"));
        assert!(prompt.contains("pause"));
        // Verify phased workflow is present
        assert!(prompt.contains("Phase 1: Explore and understand"));
        assert!(prompt.contains("Phase 2: Clarify ambiguities"));
        assert!(prompt.contains("Phase 3: Converge on a recommendation"));
        assert!(prompt.contains("Phase 4: Publish the plan"));
        // Verify quality contract is present
        assert!(prompt.contains("Plan quality contract"));
    }

    #[test]
    fn default_mode_prompt_mentions_clarify_for_missing_information() {
        let prompt = run_mode_prompt_body("default");

        assert!(prompt.contains("Use clarify instead of guessing"));
        assert!(prompt.contains("multiple reasonable approaches"));
        assert!(prompt.contains("approve a risky action"));
    }

    #[test]
    fn default_mode_prompt_references_update_plan_quality_contract() {
        let prompt = run_mode_prompt_body("default");

        assert!(prompt.contains("follow the quality contract"));
        assert!(prompt.contains("update_plan tool description"));
        assert!(prompt.contains("Explore the codebase first"));
    }

    #[test]
    fn plan_mode_requires_checkpoint_before_successful_completion() {
        assert_eq!(
            plan_mode_missing_checkpoint_error("plan", false),
            Some(PLAN_MODE_MISSING_CHECKPOINT_ERROR)
        );
        assert_eq!(plan_mode_missing_checkpoint_error("plan", true), None);
        assert_eq!(plan_mode_missing_checkpoint_error("default", false), None);
    }

    #[test]
    fn build_plan_artifact_extracts_numbered_steps() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Implementation Plan",
                "summary": "Produce the implementation plan.",
                "steps": [
                    "Update runtime-thread-surface.",
                    { "title": "Validate typecheck." }
                ]
            }),
            3,
        );

        assert_eq!(artifact.title, "Implementation Plan");
        assert_eq!(artifact.plan_revision, 3);
        assert_eq!(artifact.steps[0].title, "Update runtime-thread-surface.");
        assert_eq!(artifact.steps[1].title, "Validate typecheck.");
    }

    #[test]
    fn convert_history_messages_keeps_plan_checkpoints_but_skips_approval_prompts() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Plan title",
                "summary": "Carry the previous plan forward.",
                "steps": ["Keep the plan in follow-up context."]
            }),
            2,
        );
        let plan_metadata = build_plan_message_metadata(artifact, "run-plan", "plan");
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Please refine the plan.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-plan".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-plan".to_string()),
                role: "assistant".to_string(),
                content_markdown: "stale plan body".to_string(),
                message_type: "plan".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::to_string(&plan_metadata).expect("serialize plan metadata"),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-approval".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-plan".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Review and approve the plan.".to_string(),
                message_type: "approval_prompt".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 2);
        assert_eq!(message_text(&history[0]), "Please refine the plan.");
        assert!(message_text(&history[1])
            .contains("Implementation plan checkpoint (revision 2, approval state: pending):"));
        assert!(message_text(&history[1]).contains("# Plan title"));
        assert!(message_text(&history[1]).contains("Keep the plan in follow-up context."));
    }

    #[test]
    fn convert_history_messages_uses_effective_prompt_for_command_messages() {
        let messages = vec![MessageRecord {
            id: "msg-command".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "/init".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(
                serde_json::json!({
                    "composer": {
                        "kind": "command",
                        "displayText": "/init",
                        "effectivePrompt": "Generate or update a file named AGENTS.md."
                    }
                })
                .to_string(),
            ),
            attachments_json: None,
            created_at: String::new(),
        }];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        assert_eq!(
            message_text(&history[0]),
            "Generate or update a file named AGENTS.md."
        );
    }

    #[test]
    fn convert_history_messages_includes_image_and_text_attachments() {
        let messages = vec![MessageRecord {
            id: "msg-attachment".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "Please inspect these files.".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: Some(
                serde_json::json!([
                    {
                        "id": "image-1",
                        "name": "diagram.png",
                        "mediaType": "image/png",
                        "url": "data:image/png;base64,aGVsbG8="
                    },
                    {
                        "id": "text-1",
                        "name": "notes.md",
                        "mediaType": "text/markdown",
                        "url": "data:text/markdown;base64,IyBIZWFkZXIKCkJvZHkgbGluZS4="
                    }
                ])
                .to_string(),
            ),
            created_at: String::new(),
        }];

        let history = convert_history_messages(
            &messages,
            &[],
            &sample_resolved_model_role_with_inputs(
                "vision-model",
                vec![
                    tiycore::types::InputType::Text,
                    tiycore::types::InputType::Image,
                ],
            )
            .model,
        );

        assert_eq!(history.len(), 1);
        let blocks = user_blocks(&history[0]);
        assert_eq!(blocks.len(), 3);

        match &blocks[0] {
            ContentBlock::Text(text) => assert_eq!(text.text, "Please inspect these files."),
            _ => panic!("expected prompt text block"),
        }

        match &blocks[1] {
            ContentBlock::Image(image) => {
                assert_eq!(image.mime_type, "image/png");
                assert_eq!(image.data, "aGVsbG8=");
            }
            _ => panic!("expected image block"),
        }

        match &blocks[2] {
            ContentBlock::Text(text) => {
                assert!(text.text.contains("[Text attachment: notes.md]"));
                assert!(text.text.contains("~~~markdown"));
                assert!(text.text.contains("# Header"));
                assert!(text.text.contains("Body line."));
            }
            _ => panic!("expected text attachment block"),
        }
    }

    #[test]
    fn convert_history_messages_falls_back_to_text_for_unsupported_image_models() {
        let messages = vec![MessageRecord {
            id: "msg-image".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "Describe this image.".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: Some(
                serde_json::json!([
                    {
                        "id": "image-1",
                        "name": "photo.png",
                        "mediaType": "image/png",
                        "url": "data:image/png;base64,aGVsbG8="
                    }
                ])
                .to_string(),
            ),
            created_at: String::new(),
        }];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        let blocks = user_blocks(&history[0]);
        assert_eq!(blocks.len(), 2);

        match &blocks[1] {
            ContentBlock::Text(text) => {
                assert_eq!(text.text, "[Image attachment: photo.png]");
            }
            _ => panic!("expected text fallback block"),
        }
    }

    #[test]
    fn trim_history_to_current_context_keeps_only_messages_after_latest_reset() {
        let messages = vec![
            MessageRecord {
                id: "msg-before".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-before".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Old assistant reply".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-reset".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "Context is now reset".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_reset",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-summary".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>\nCarry this forward.\n</context_summary>"
                    .to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_summary",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-after".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "New request".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let trimmed = trim_history_to_current_context(&messages);

        assert_eq!(trimmed.len(), 2);
        assert_eq!(trimmed[0].id, "msg-summary");
        assert_eq!(trimmed[1].id, "msg-after");
    }

    #[test]
    fn convert_history_messages_keeps_context_summary_but_skips_reset_markers() {
        let messages = vec![
            MessageRecord {
                id: "msg-reset".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "Context is now reset".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_reset",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-summary".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>\nCarry this forward.\n</context_summary>"
                    .to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_summary",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        assert_eq!(
            message_text(&history[0]),
            "<context_summary>\nCarry this forward.\n</context_summary>"
        );
    }

    #[test]
    fn convert_history_messages_merges_reasoning_into_assistant() {
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Hello".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me think about this.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Here is the answer.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        // Should produce: User + Assistant (with Thinking + Text blocks)
        assert_eq!(history.len(), 2);
        match &history[0] {
            AgentMessage::User(_) => {}
            other => panic!("expected User, got {:?}", other),
        }
        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    2,
                    "assistant should have Thinking + Text blocks"
                );
                assert!(assistant.content[0].is_thinking());
                assert!(assistant.content[1].is_text());
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Let me think about this.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content")
                );
                let text = assistant.content[1].as_text().unwrap();
                assert_eq!(text.text, "Here is the answer.");
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_reasoning_without_signature_still_merges() {
        let messages = vec![
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Thinking...".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: None, // no signature (old data)
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Result.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        assert_eq!(history.len(), 1);
        match &history[0] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 2);
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Thinking...");
                assert!(thinking.thinking_signature.is_none());
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_orphan_reasoning_at_end_is_dropped() {
        // A reasoning message at the end with no following assistant text
        // (e.g. interrupted run) should be silently dropped.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Hello".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Orphan reasoning".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        // Only the user message should appear; the orphan reasoning is dropped.
        assert_eq!(history.len(), 1);
        assert!(matches!(&history[0], AgentMessage::User(_)));
    }

    #[test]
    fn convert_history_messages_attaches_reasoning_to_tool_call() {
        // Scenario: user → reasoning → tool_call (no intermediate text).
        // No text message to merge into, so tool call gets its own standalone
        // assistant message.  The reasoning message's position IS the
        // insert_pos for the tool call.  With SortKey::after_position the
        // standalone sorts after the reasoning, so Phase 4 attaches the
        // PendingThinking correctly.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Run a command".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // Reasoning arrives at 00:00:02 — AFTER the tool call's started_at
            // so that insert_pos points here (the first message in run-1 with
            // created_at >= tc.started_at).
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me think about the command.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
            // A plain_message from a LATER response (after tool result).
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Done.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:05.000Z".to_string(),
            },
        ];

        let tool_calls = vec![ToolCallDto {
            id: "tc-1".to_string(),
            storage_id: "st-1".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            helper_id: None,
            tool_name: "shell".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
            tool_output: Some(serde_json::json!("file.txt")),
            status: "completed".to_string(),
            approval_status: None,
            started_at: "2026-01-01T00:00:01.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:03.000Z".to_string()),
        }];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // insert_pos = reasoning (index 1), no preceding text → standalone.
        // Phase 4: reasoning PendingThinking at (1,2) → standalone at (1,3)
        // gets Thinking attached.
        // Expected: User → Assistant[Thinking, ToolCall] → ToolResult → Assistant[Text]
        assert_eq!(
            history.len(),
            4,
            "should have User + TC-Assistant + ToolResult + Text-Assistant"
        );

        // 1. User message
        assert!(matches!(&history[0], AgentMessage::User(_)));

        // 2. Tool-call assistant with reasoning prepended
        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    2,
                    "tool-call assistant should have Thinking + ToolCall blocks, got: {:?}",
                    assistant.content
                );
                assert!(
                    assistant.content[0].is_thinking(),
                    "first block should be Thinking"
                );
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Let me think about the command.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content")
                );
                assert!(
                    assistant.content[1].is_tool_call(),
                    "second block should be ToolCall"
                );
            }
            other => panic!("expected Assistant for tool call, got {:?}", other),
        }

        // 3. Tool result
        assert!(matches!(&history[2], AgentMessage::ToolResult(_)));

        // 4. Final text assistant (no thinking blocks — they were consumed by the tool call)
        match &history[3] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    1,
                    "text assistant should have only Text"
                );
                assert!(assistant.content[0].is_text());
            }
            other => panic!("expected text Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_merges_multiple_standalone_tool_calls_at_same_position() {
        // Scenario: one DeepSeek response emits reasoning + two tool calls and no text.
        // Both tool calls insert at the same position (reasoning message), so they
        // must be reconstructed as a single assistant message sharing reasoning_content.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Run two commands".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // Reasoning arrives at 00:00:02 — AFTER both tool calls' started_at
            // so that insert_pos points here for both tool calls.
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "I need to run two independent commands.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Both commands finished.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:10.000Z".to_string(),
            },
        ];

        let tool_calls = vec![
            ToolCallDto {
                id: "tc-1".to_string(),
                storage_id: "st-1".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "pwd"}),
                tool_output: Some(serde_json::json!("/tmp/project")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:01.000Z".to_string(),
                finished_at: Some("2026-01-01T00:00:03.000Z".to_string()),
            },
            ToolCallDto {
                id: "tc-2".to_string(),
                storage_id: "st-2".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_output: Some(serde_json::json!("file.txt")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:01.500Z".to_string(),
                finished_at: Some("2026-01-01T00:00:05.000Z".to_string()),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // Expected: User → Assistant[Thinking, ToolCall1, ToolCall2]
        // → ToolResult1 → ToolResult2 → Assistant[Text]
        assert_eq!(history.len(), 5, "unexpected history: {history:?}");
        assert!(matches!(&history[0], AgentMessage::User(_)));

        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    3,
                    "assistant should contain Thinking + both ToolCalls"
                );
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "I need to run two independent commands.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content")
                );
                assert!(assistant.content[1].is_tool_call());
                assert!(assistant.content[2].is_tool_call());
                assert_eq!(assistant.stop_reason, StopReason::ToolUse);
            }
            other => panic!("expected merged standalone Assistant, got {:?}", other),
        }

        match &history[2] {
            AgentMessage::ToolResult(result) => assert_eq!(result.tool_call_id, "tc-1"),
            other => panic!("expected first ToolResult, got {:?}", other),
        }
        match &history[3] {
            AgentMessage::ToolResult(result) => assert_eq!(result.tool_call_id, "tc-2"),
            other => panic!("expected second ToolResult, got {:?}", other),
        }

        match &history[4] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 1);
                assert!(assistant.content[0].is_text());
                assert_eq!(
                    assistant.content[0].as_text().unwrap().text,
                    "Both commands finished."
                );
            }
            other => panic!("expected final text Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_merges_tool_call_into_preceding_text() {
        // Scenario mimicking real DeepSeek flow: a single API response produces
        // reasoning + text + tool_call.  The text is saved first, then the tool
        // is executed (started_at > text.created_at).  The tool call must be
        // merged into the text assistant message so they share the same
        // reasoning_content.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Do something".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // Reasoning from the first response
            MessageRecord {
                id: "msg-r1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me run a command.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // Text from the first response (saved during streaming)
            MessageRecord {
                id: "msg-text1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Running the command now.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
            // Reasoning from the SECOND response (after tool result)
            MessageRecord {
                id: "msg-r2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "The command succeeded.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:06.000Z".to_string(),
            },
            // Text from the second response
            MessageRecord {
                id: "msg-text2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "All done!".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:07.000Z".to_string(),
            },
        ];

        // TC1 from the first response — started AFTER the text was saved.
        let tool_calls = vec![ToolCallDto {
            id: "tc-1".to_string(),
            storage_id: "st-1".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "t1".to_string(),
            helper_id: None,
            tool_name: "shell".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
            tool_output: Some(serde_json::json!("file.txt")),
            status: "completed".to_string(),
            approval_status: None,
            // started_at > text1.created_at but < r2.created_at
            started_at: "2026-01-01T00:00:03.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:04.000Z".to_string()),
        }];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // TC1 should merge into text1 (same run, no reasoning between them).
        // Expected: User → Assistant[Thinking(R1), Text, ToolCall] → ToolResult → Assistant[Thinking(R2), Text]
        assert_eq!(
            history.len(),
            4,
            "should have User + merged-Assistant + ToolResult + final-Assistant, got {:?}",
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );

        // 1. User
        assert!(matches!(&history[0], AgentMessage::User(_)));

        // 2. Merged assistant with Thinking + Text + ToolCall
        match &history[1] {
            AgentMessage::Assistant(a) => {
                assert_eq!(
                    a.content.len(),
                    3,
                    "merged assistant should have Thinking + Text + ToolCall, got: {:?}",
                    a.content
                );
                assert!(a.content[0].is_thinking(), "block 0 should be Thinking");
                assert_eq!(
                    a.content[0].as_thinking().unwrap().thinking,
                    "Let me run a command."
                );
                assert!(a.content[1].is_text(), "block 1 should be Text");
                assert_eq!(
                    a.content[1].as_text().unwrap().text,
                    "Running the command now."
                );
                assert!(a.content[2].is_tool_call(), "block 2 should be ToolCall");
                assert_eq!(a.stop_reason, StopReason::ToolUse);
            }
            other => panic!("expected merged Assistant, got {:?}", other),
        }

        // 3. Tool result
        assert!(matches!(&history[2], AgentMessage::ToolResult(_)));

        // 4. Final text with R2 reasoning
        match &history[3] {
            AgentMessage::Assistant(a) => {
                assert_eq!(a.content.len(), 2, "final: Thinking(R2) + Text");
                assert!(a.content[0].is_thinking());
                assert_eq!(
                    a.content[0].as_thinking().unwrap().thinking,
                    "The command succeeded."
                );
                assert!(a.content[1].is_text());
                assert_eq!(a.content[1].as_text().unwrap().text, "All done!");
            }
            other => panic!("expected final Assistant, got {:?}", other),
        }
    }

    #[test]
    fn sortkey_ordering() {
        // Same position: before (sub=0) < positional (sub=2) < after (sub=3)
        let before = SortKey::before_position(5, 1);
        let positional = SortKey::positional(5);
        let after = SortKey::after_position(5, 2);
        assert!(
            before < positional,
            "before_position should sort before positional"
        );
        assert!(
            positional < after,
            "positional should sort before after_position"
        );
        assert!(
            before < after,
            "before_position should sort before after_position"
        );

        // Seq tiebreaker for same (position, sub)
        let a = SortKey::before_position(5, 10);
        let b = SortKey::before_position(5, 20);
        assert!(
            a < b,
            "lower seq should sort before higher seq at same (position, sub)"
        );

        // Different positions should still respect sub ordering when positions differ
        let later_pos_before = SortKey::before_position(10, 0);
        let earlier_pos_after = SortKey::after_position(5, 0);
        assert!(
            earlier_pos_after < later_pos_before,
            "position should be primary sort key"
        );
    }

    #[test]
    fn convert_history_messages_multiple_reasoning_tool_call_cycles() {
        // Scenario: user → R1 → text1 → R2 → TC2 → R3 → text2
        // TC1 merges into text1 (no intervening reasoning).
        // TC2 cannot merge because R2 sits between text1 and its insert_pos.
        // R1 attaches to text1, R2 attaches to TC2 standalone, R3 to text2.
        let messages = vec![
            MessageRecord {
                id: "msg-01-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Go".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // R1
            MessageRecord {
                id: "msg-02-r1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "R1 thinking".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"thinking_signature": "sig"}).to_string()),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // text1 — TC1 merges into this (no reasoning between text1 and TC1)
            MessageRecord {
                id: "msg-03-text1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Text 1".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:05.000Z".to_string(),
            },
            // R2 — blocks TC2 from merging into text1
            MessageRecord {
                id: "msg-04-r2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "R2 thinking".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"thinking_signature": "sig"}).to_string()),
                attachments_json: None,
                created_at: "2026-01-01T00:00:06.000Z".to_string(),
            },
            // R3
            MessageRecord {
                id: "msg-05-r3".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "R3 thinking".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"thinking_signature": "sig"}).to_string()),
                attachments_json: None,
                created_at: "2026-01-01T00:00:09.000Z".to_string(),
            },
            // text2 — final reply
            MessageRecord {
                id: "msg-06-text2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Text 2".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:12.000Z".to_string(),
            },
        ];

        let tool_calls = vec![
            // TC1: started AFTER text1, no reasoning between text1 and TC1's
            // insert_pos → merges into text1.
            ToolCallDto {
                id: "tc-1".to_string(),
                storage_id: "st-1".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "t1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_output: Some(serde_json::json!("out1")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:05.500Z".to_string(),
                finished_at: Some("2026-01-01T00:00:05.800Z".to_string()),
            },
            // TC2: started after R2 but before R3.  insert_pos points to R3
            // (first msg in run-1 with created_at >= 00:00:07).
            // R2 sits between text1 and R3 → merge blocked → standalone.
            // after_position: standalone sorts AFTER R3's PendingThinking.
            ToolCallDto {
                id: "tc-2".to_string(),
                storage_id: "st-2".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "t1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "pwd"}),
                tool_output: Some(serde_json::json!("out2")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:07.000Z".to_string(),
                finished_at: Some("2026-01-01T00:00:08.000Z".to_string()),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // Timeline after sort:
        //   (0,2) User
        //   (1,2) PendingThinking(R1)
        //   (2,0,0) TC1-result (merged into text1, result before pos 3)
        //   (2,2) text1 merged with TC1 → Assistant[Thinking(R1), Text, ToolCall]
        //   (3,2) PendingThinking(R2)
        //   (4,2) PendingThinking(R3)
        //   (4,3,2) TC2-standalone → gets R2+R3
        //   (4,3,3) TC2-result
        //   (5,2) text2 → Assistant[Text]
        //
        // Wait, TC1 merge: text1 is at index 2. TC1 insert_pos: first msg in
        // run-1 with created_at >= 00:00:05.5 → msg-04-r2 at 00:06 (index 3).
        // merge_target: search backwards from index 3 for plain_message in run-1
        // → msg-03-text1 at index 2. Check no reasoning between index 2+1=3 and
        // insert_pos 3 → range [3..3) is empty → merge allowed!
        // TC1 merges into text1 at index 2.
        //
        // TC2: insert_pos → msg-05-r3 at 00:09 (index 4).
        // merge_target: search backwards from index 4 → msg-03-text1 at index 2.
        // Check reasoning between 3..4 → msg-04-r2 at index 3 is reasoning → blocked!
        // Standalone at (4, 3, ...).
        //
        // Phase 4:
        //   R1(PendingThinking) → accumulated
        //   text1+TC1 at (2,2) → consumes R1, becomes [Thinking(R1), Text, ToolCall]
        //   TC1-result at (3,0) → pass through
        //   R2(PendingThinking at 3,2) → accumulated
        //   R3(PendingThinking at 4,2) → accumulated
        //   TC2-standalone at (4,3) → consumes R2+R3
        //   TC2-result → pass through
        //   text2 at (5,2) → no pending thinking → just Text
        //
        // Result: User, Asst[T(R1),Text1,TC1], TR1, Asst[T(R2),T(R3),TC2], TR2, Asst[Text2]
        assert_eq!(
            history.len(),
            6,
            "expected 6 messages: {:?}",
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );

        // 1. User
        assert!(matches!(&history[0], AgentMessage::User(_)));

        // 2. text1 merged with TC1, R1 thinking prepended
        match &history[1] {
            AgentMessage::Assistant(a) => {
                assert_eq!(
                    a.content.len(),
                    3,
                    "text1+TC1: Thinking(R1) + Text + ToolCall, got: {:?}",
                    a.content
                );
                assert!(a.content[0].is_thinking());
                assert_eq!(a.content[0].as_thinking().unwrap().thinking, "R1 thinking");
                assert!(a.content[1].is_text());
                assert!(a.content[2].is_tool_call());
            }
            other => panic!("expected text1+TC1 assistant, got {:?}", other),
        }

        // 3. TC1 result
        assert!(matches!(&history[2], AgentMessage::ToolResult(_)));

        // 4. TC2 standalone with R2+R3 thinking
        match &history[3] {
            AgentMessage::Assistant(a) => {
                // R2 and R3 both accumulated as PendingThinking before TC2
                assert_eq!(
                    a.content.len(),
                    3,
                    "TC2 should have Thinking(R2) + Thinking(R3) + ToolCall"
                );
                assert!(a.content[0].is_thinking());
                assert_eq!(a.content[0].as_thinking().unwrap().thinking, "R2 thinking");
                assert!(a.content[1].is_thinking());
                assert_eq!(a.content[1].as_thinking().unwrap().thinking, "R3 thinking");
                assert!(a.content[2].is_tool_call());
            }
            other => panic!("expected TC2 standalone assistant, got {:?}", other),
        }

        // 5. TC2 result
        assert!(matches!(&history[4], AgentMessage::ToolResult(_)));

        // 6. text2 — no pending thinking, just text
        match &history[5] {
            AgentMessage::Assistant(a) => {
                assert_eq!(
                    a.content.len(),
                    1,
                    "text2 should have only Text, no thinking"
                );
                assert!(a.content[0].is_text());
                assert_eq!(a.content[0].as_text().unwrap().text, "Text 2");
            }
            other => panic!("expected text2 assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_skips_empty_assistant_plain_message() {
        // Scenario: a provider error left an empty assistant plain_message
        // in the DB.  The reasoning before it should be treated as orphan
        // and dropped; the empty assistant should be skipped entirely.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Do something".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // Reasoning from a run that later failed
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Thinking about the task.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // Empty assistant plain_message left by the failed run
            MessageRecord {
                id: "msg-empty".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: String::new(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
            // Next user message (new run)
            MessageRecord {
                id: "msg-user-2".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Continue".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:03.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        // Empty assistant is skipped; reasoning before it becomes orphan
        // (no following assistant to attach to before the next user message)
        // and is also dropped.  Result: User, User.
        assert_eq!(
            history.len(),
            2,
            "expected 2 messages (both users), got {}: {:?}",
            history.len(),
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );
        assert!(matches!(&history[0], AgentMessage::User(_)));
        assert!(matches!(&history[1], AgentMessage::User(_)));
    }

    #[test]
    fn convert_history_messages_skips_whitespace_only_assistant_plain_message() {
        // Same as above but with whitespace-only content (trimmed to empty).
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Hello".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-ws".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "   \n  ".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        assert_eq!(history.len(), 1);
        assert!(matches!(&history[0], AgentMessage::User(_)));
    }

    #[test]
    fn convert_history_messages_standalone_tool_call_gets_reasoning_when_at_same_position() {
        // Scenario that triggered DeepSeek 400:
        //   user → reasoning-1 → reasoning-2 → text (later)
        //   tool_call starts between reasoning-1 and reasoning-2
        //
        // No preceding plain_message in run-1 → standalone.
        // insert_pos points to reasoning-2 (first msg >= tc.started_at).
        // With SortKey::after_position, the standalone sorts AFTER
        // reasoning-2's PendingThinking at the same position, so Phase 4
        // attaches reasoning-1 + reasoning-2 to the standalone.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Do something".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning-1".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me plan this.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // reasoning-2 created AFTER tc.started_at → this is the insert_pos
            MessageRecord {
                id: "msg-reasoning-2".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Checking output now.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:04.000Z".to_string(),
            },
            // A later text response after the tool result
            MessageRecord {
                id: "msg-text".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "All done.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:08.000Z".to_string(),
            },
        ];

        // TC started_at=00:00:03 < reasoning-2 created_at=00:00:04
        // → insert_pos = reasoning-2 (index 2)
        // No preceding plain_message in run-1 → standalone
        let tool_calls = vec![ToolCallDto {
            id: "tc-1".to_string(),
            storage_id: "st-1".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            helper_id: None,
            tool_name: "shell".to_string(),
            tool_input: serde_json::json!({"command": "cargo test"}),
            tool_output: Some(serde_json::json!("ok")),
            status: "completed".to_string(),
            approval_status: None,
            started_at: "2026-01-01T00:00:03.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:03.500Z".to_string()),
        }];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // Timeline after sort:
        //   (0,2) User
        //   (1,2) PendingThinking(reasoning-1)
        //   (2,2) PendingThinking(reasoning-2)
        //   (2,3) Standalone assistant → gets reasoning-1 + reasoning-2
        //   (2,3) ToolResult
        //   (3,2) Assistant[Text("All done.")]
        //
        // Expected: User → Assistant[Thinking×2, ToolCall] → ToolResult → Assistant[Text]
        assert_eq!(
            history.len(),
            4,
            "should have User + standalone-TC + ToolResult + text-assistant, got {}: {:?}",
            history.len(),
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );

        // The standalone tool-call assistant MUST have Thinking blocks
        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                // reasoning-1 and reasoning-2 both attached
                assert!(
                    assistant.content.len() >= 2,
                    "standalone should have Thinking(s) + ToolCall, got: {:?}",
                    assistant.content
                );
                assert!(
                    assistant.content[0].is_thinking(),
                    "first block should be Thinking, got: {:?}",
                    assistant.content[0]
                );
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Let me plan this.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content"),
                    "thinking_signature must be preserved for DeepSeek"
                );
                // Last block must be ToolCall
                assert!(
                    assistant.content.last().unwrap().is_tool_call(),
                    "last block should be ToolCall"
                );
            }
            other => panic!("expected standalone Assistant at index 1, got {:?}", other),
        }

        // Tool result at index 2
        assert!(
            matches!(&history[2], AgentMessage::ToolResult(_)),
            "index 2 should be ToolResult"
        );

        // Final text assistant has no orphan thinking
        match &history[3] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 1);
                assert!(assistant.content[0].is_text());
            }
            other => panic!("expected text Assistant at index 3, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // convert_history_messages boundary regression tests
    // -----------------------------------------------------------------------

    #[test]
    fn convert_history_messages_empty_reasoning_skipped() {
        let model = sample_resolved_model_role("gpt-test").model;
        let messages = vec![
            MessageRecord {
                id: "msg-reasoning-empty".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "  ".to_string(), // empty/whitespace-only
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Hello".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
        ];

        let history = convert_history_messages(&messages, &[], &model);

        // The empty reasoning should be skipped, so only the assistant text remains.
        assert_eq!(history.len(), 1);
        match &history[0] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 1);
                assert!(assistant.content[0].is_text());
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_tool_result_clears_pending_thinking() {
        // Scenario: user → reasoning → [tool_call → tool_result] → final
        // assistant.  The reasoning should attach to the tool-call assistant,
        // the ToolResult should clear any remaining pending thinking, and the
        // final assistant should have NO thinking blocks.
        let model = sample_resolved_model_role("gpt-test").model;
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "user".to_string(),
                content_markdown: "Start".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Orphan thinking".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
            // The assistant text that follows the tool result must NOT carry
            // the orphan reasoning from before the tool call.
            MessageRecord {
                id: "msg-assistant-after".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Final answer".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:05.000Z".to_string(),
            },
        ];
        // Use started_at matching the reasoning's created_at so the tool call
        // is placed at the reasoning's position in the timeline (before the
        // final assistant).
        let tool_calls = vec![ToolCallDto {
            id: "tc-1".to_string(),
            storage_id: "tc-storage-1".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            helper_id: None,
            tool_name: "read".to_string(),
            tool_input: serde_json::json!({"path": "foo.rs"}),
            tool_output: Some(serde_json::json!("file content")),
            status: "completed".to_string(),
            approval_status: None,
            started_at: "2026-01-01T00:00:02.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:04.000Z".to_string()),
        }];

        let history = convert_history_messages(&messages, &tool_calls, &model);

        // Verify there is a ToolResult in the history.
        let tool_result_idx = history
            .iter()
            .position(|m| matches!(m, AgentMessage::ToolResult(_)));
        assert!(
            tool_result_idx.is_some(),
            "should have a ToolResult in history"
        );

        // The "Final answer" assistant message comes after the ToolResult.
        // It must NOT contain any thinking blocks from the orphan reasoning.
        let final_assistant =
            history
                .iter()
                .skip(tool_result_idx.unwrap() + 1)
                .find_map(|m| match m {
                    AgentMessage::Assistant(a) => Some(a),
                    _ => None,
                });
        assert!(
            final_assistant.is_some(),
            "should have an assistant after ToolResult"
        );
        let final_assistant = final_assistant.unwrap();
        for block in &final_assistant.content {
            assert!(
                !block.is_thinking(),
                "orphan thinking leaked past tool result boundary into final assistant"
            );
        }
        assert_eq!(final_assistant.content.len(), 1);
        assert!(final_assistant.content[0].is_text());
    }

    /// Regression test documenting the Phase 4 known limitation that caused
    /// DeepSeek 400 errors.  When a tool call's `insert_pos` coincides with
    /// a reasoning message's position, Phase 4 attaches that reasoning to
    /// the standalone tool-call assistant instead of the subsequent text
    /// assistant.  The `normalize_reasoning_content` safety net in tiycore
    /// backfills `reasoning_content` on the final JSON to prevent the 400.
    #[test]
    fn convert_history_messages_final_text_loses_thinking_when_tool_at_same_pos() {
        // Timeline that mirrors the real bug:
        //   pos 0: user message
        //   pos 1: reasoning-A  (thinking for tool-call iteration)
        //   pos 2: reasoning-B  (thinking for final text — but tc insert_pos = 2)
        //   pos 3: text assistant ("Final answer")
        //
        // Tool call started_at lands between reasoning-A and reasoning-B,
        // so insert_pos = index of reasoning-B = 2.
        //
        // Phase 4 after sort:
        //   (0,2) User
        //   (1,2) PendingThinking(A)
        //   (2,2) PendingThinking(B)        ← enters buffer
        //   (2,3) Standalone[tc]            ← drains buffer: gets A + B
        //   (2,3) ToolResult                ← clears buffer
        //   (3,2) Assistant["Final answer"] ← buffer empty, no thinking
        let model = sample_resolved_model_role("gpt-test").model;
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Analyze the code".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning-a".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me search for it.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // reasoning-B: thinking for final text, but tc.started_at < this
            MessageRecord {
                id: "msg-reasoning-b".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Now I can write the final answer.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:05.000Z".to_string(),
            },
            // Final text response
            MessageRecord {
                id: "msg-final-text".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Here is my complete analysis.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:10.000Z".to_string(),
            },
        ];

        // Tool call started between reasoning-A and reasoning-B
        let tool_calls = vec![ToolCallDto {
            id: "tc-search".to_string(),
            storage_id: "st-search".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "t1".to_string(),
            helper_id: None,
            tool_name: "search".to_string(),
            tool_input: serde_json::json!({"query": "analysis"}),
            tool_output: Some(serde_json::json!("search results")),
            status: "completed".to_string(),
            approval_status: None,
            started_at: "2026-01-01T00:00:03.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:04.000Z".to_string()),
        }];

        let history = convert_history_messages(&messages, &tool_calls, &model);

        // Expected structure: User → Assistant[Thinking+ToolCall] → ToolResult → Assistant[Text]
        assert_eq!(
            history.len(),
            4,
            "should be User + TC-Assistant + ToolResult + Text-Assistant, got {:?}",
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );

        // The standalone tool-call assistant got reasoning-A + reasoning-B.
        match &history[1] {
            AgentMessage::Assistant(a) => {
                let thinking_count = a.content.iter().filter(|b| b.is_thinking()).count();
                assert!(
                    thinking_count >= 1,
                    "standalone should have at least one Thinking block"
                );
                assert!(
                    a.content.last().unwrap().is_tool_call(),
                    "last block should be ToolCall"
                );
            }
            other => panic!("expected Assistant at index 1, got {:?}", other),
        }

        // KEY ASSERTION: The final text assistant has NO thinking blocks.
        // This documents the Phase 4 known limitation — the normalizer
        // safety net in normalize_reasoning_content is required to
        // backfill reasoning_content on the serialized JSON before sending.
        match &history[3] {
            AgentMessage::Assistant(a) => {
                let has_thinking = a.content.iter().any(|b| b.is_thinking());
                assert!(
                    !has_thinking,
                    "Phase 4 known limitation: final text assistant should NOT have thinking \
                     (it was consumed by the standalone at same insert_pos); \
                     normalize_reasoning_content provides the safety net"
                );
                assert_eq!(a.content.len(), 1);
                assert!(a.content[0].is_text());
            }
            other => panic!("expected text Assistant at index 3, got {:?}", other),
        }
    }
}
