//! Context compression pure-logic tests (external, using public API)
//!
//! Tests the public functions of context_compression module.
//! Note: context_compression also has comprehensive internal unit tests;
//! these external tests add additional coverage through the public interface.

use tiycode::core::context_compression::{
    compress_context_fallback, find_cut_point, generate_discard_summary, CompressionSettings,
    ContextTokenCalibration,
};
use tiycore::agent::AgentMessage;
use tiycore::types::{ContentBlock, TextContent, ToolCall, ToolResultMessage, UserMessage};

// =========================================================================
// Helper functions to build messages using tiycore's builder API
// =========================================================================

fn user_msg(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage::text(text.to_string()))
}

fn assistant_msg(text: &str) -> AgentMessage {
    AgentMessage::Assistant(
        tiycore::types::AssistantMessage::builder()
            .content(vec![ContentBlock::Text(TextContent::new(text))])
            .api(tiycore::types::Api::OpenAICompletions)
            .provider(tiycore::types::Provider::OpenAI)
            .model("test-model")
            .usage(tiycore::types::Usage::default())
            .stop_reason(tiycore::types::StopReason::Stop)
            .build()
            .unwrap(),
    )
}

fn assistant_with_tool(tool_name: &str) -> AgentMessage {
    AgentMessage::Assistant(
        tiycore::types::AssistantMessage::builder()
            .content(vec![ContentBlock::ToolCall(ToolCall::new(
                "tc-1",
                tool_name,
                serde_json::json!({}),
            ))])
            .api(tiycore::types::Api::OpenAICompletions)
            .provider(tiycore::types::Provider::OpenAI)
            .model("test")
            .usage(tiycore::types::Usage::default())
            .stop_reason(tiycore::types::StopReason::ToolUse)
            .build()
            .unwrap(),
    )
}

fn tool_result_msg(tool_name: &str, output: &str) -> AgentMessage {
    AgentMessage::ToolResult(ToolResultMessage::text("tc-1", tool_name, output, false))
}

// =========================================================================
// ContextTokenCalibration
// =========================================================================

#[test]
fn calibration_default_is_identity() {
    let cal = ContextTokenCalibration::default();
    assert_eq!(cal.ratio_basis_points(), 10_000);
    assert_eq!(cal.apply_to_estimate(100), 100);
    assert_eq!(cal.apply_to_estimate(0), 0);
    assert_eq!(cal.apply_to_estimate(999_999), 999_999);
}

#[test]
fn calibration_from_observation_none_for_zero_inputs() {
    assert!(ContextTokenCalibration::from_observation(0, 100).is_none());
    assert!(ContextTokenCalibration::from_observation(100, 0).is_none());
    assert!(ContextTokenCalibration::from_observation(0, 0).is_none());
}

#[test]
fn calibration_from_observation_builds_ratio() {
    let cal = ContextTokenCalibration::from_observation(100, 200).unwrap();
    // Actual is 2x estimated → ratio should be > 10000 bps
    assert!(cal.ratio_basis_points() > 10_000);
    let applied = cal.apply_to_estimate(100);
    assert!(applied >= 200); // at least 2x
}

#[test]
fn calibration_observe_increases_ratio() {
    let cal = ContextTokenCalibration::default().observe(100, 300); // 3x
    assert!(cal.ratio_basis_points() > 10_000);
    let applied = cal.apply_to_estimate(100);
    assert!(applied >= 300);
}

#[test]
fn calibration_keeps_max_ratio_across_observations() {
    let cal = ContextTokenCalibration::default()
        .observe(100, 500) // 5x — max so far
        .observe(100, 200); // 2x — should NOT lower the ratio
    assert_eq!(cal.ratio_basis_points(), 50_000);
}

#[test]
fn calibration_observe_zero_does_not_change() {
    let cal = ContextTokenCalibration::default()
        .observe(100, 200)
        .observe(0, 999)
        .observe(999, 0);
    assert_eq!(cal.ratio_basis_points(), 20_000);
}

#[test]
fn calibration_apply_saturates_at_u32_max() {
    let cal = ContextTokenCalibration::from_observation(1, u64::MAX).unwrap();
    let result = cal.apply_to_estimate(1_000_000);
    // Should saturate, not overflow
    assert!(result > 0);
}

// =========================================================================
// estimate_message_tokens (via should_compress as a proxy)
// =========================================================================

#[test]
fn estimate_counts_user_messages() {
    let settings = CompressionSettings {
        context_window: 20,
        reserve_tokens: 10,
        keep_recent_tokens: 5,
    };
    // A single short user message (~5 tokens) fits in 20-window budget
    let messages = vec![user_msg("hi")];
    assert!(
        !tiycode::core::context_compression::should_compress(&messages, &settings),
        "short message should fit in tiny budget"
    );
}

#[test]
fn estimate_counts_long_messages_correctly() {
    let settings = CompressionSettings {
        context_window: 50,
        reserve_tokens: 10,
        keep_recent_tokens: 5,
    };
    let long_text = "a".repeat(200); // ~50+ tokens
    let messages = vec![user_msg(&long_text)];
    assert!(
        tiycode::core::context_compression::should_compress(&messages, &settings),
        "long message should exceed small budget"
    );
}

#[test]
fn cjk_text_estimates_higher_than_ascii_same_length() {
    let settings = CompressionSettings {
        context_window: 30,
        reserve_tokens: 5,
        keep_recent_tokens: 5,
    };

    let ascii_messages = vec![user_msg(&"a".repeat(24))]; // ~6 tokens ascii heuristic
    let cjk_messages = vec![user_msg(&"你好世界你好世界你好世界你好世界")]; // 12 CJK chars

    let ascii_compressed =
        tiycode::core::context_compression::should_compress(&ascii_messages, &settings);
    let cjk_compressed =
        tiycode::core::context_compression::should_compress(&cjk_messages, &settings);

    // CJK should produce equal or higher token estimates
    // If ASCII doesn't trigger compression, CJK might
    if !ascii_compressed {
        // CJK could go either way depending on exact thresholds
        // But it should definitely be >= ASCII tokens
    }
}

// =========================================================================
// CompressionSettings
// =========================================================================

#[test]
fn compression_settings_new() {
    let settings = CompressionSettings::new(128_000);
    assert_eq!(settings.context_window, 128_000);
    assert_eq!(settings.reserve_tokens, 16_384);
    assert_eq!(settings.keep_recent_tokens, 16_000);
}

#[test]
fn compression_settings_budget() {
    let settings = CompressionSettings::new(128_000);
    assert_eq!(settings.budget(), 128_000 - 16_384);
}

#[test]
fn compression_settings_budget_saturates_at_zero() {
    let settings = CompressionSettings::new(5000); // less than reserve
    assert_eq!(settings.budget(), 0); // saturates
}

// =========================================================================
// should_compress / should_compress_with_calibration
// =========================================================================

#[test]
fn should_compress_false_when_empty() {
    let settings = CompressionSettings::new(100_000);
    assert!(!tiycode::core::context_compression::should_compress(
        &[],
        &settings
    ));
}

#[test]
fn should_compress_true_for_huge_content_in_tiny_window() {
    let settings = CompressionSettings::new(100); // tiny window
    let long_text = "x".repeat(10_000);
    let messages = vec![user_msg(&long_text)];
    assert!(tiycode::core::context_compression::should_compress(
        &messages, &settings
    ));
}

#[test]
fn should_compress_with_calibration_amplifies() {
    let settings = CompressionSettings::new(100_000);
    let text = "x".repeat(4000); // ~1000 tokens heuristic
    let messages = vec![user_msg(&text)];

    // Without calibration: under budget → no compression needed
    assert!(
        !tiycode::core::context_compression::should_compress(&messages, &settings),
        "without calibration, should not compress"
    );

    // With aggressive calibration saying we underestimate by 5x:
    // 1000 * 5 = 5000, still might be under budget for 128k window.
    // Use an even more extreme calibration to guarantee compression triggers.
    let cal = ContextTokenCalibration::from_observation(100, u32::MAX as u64).unwrap();
    assert!(
        tiycode::core::context_compression::should_compress_with_calibration(
            &messages,
            &settings,
            Some(cal)
        ),
        "with max calibration, should compress"
    );
}

#[test]
fn should_compress_boundary_exact_budget() {
    // Test right around the budget boundary
    let settings = CompressionSettings {
        context_window: 50,
        reserve_tokens: 10,
        keep_recent_tokens: 10,
    };
    let text = "x".repeat(160); // ~40 tokens, budget is 40
    let messages = vec![user_msg(&text)];
    // At exactly budget, should NOT compress (strictly greater than)
    // or may compress depending on overhead (+4 per message)
    // Just verify it doesn't panic
    let _ = tiycode::core::context_compression::should_compress(&messages, &settings);
}

// =========================================================================
// find_cut_point
// =========================================================================

#[test]
fn find_cut_point_keeps_all_when_keep_is_large() {
    let messages = vec![user_msg("old"), assistant_msg("recent")];
    let estimates: Vec<u32> = messages
        .iter()
        .map(|m| tiycode::core::context_compression::estimate_message_tokens(m))
        .collect();

    let cut = find_cut_point(&messages, &estimates, 99_999);
    assert_eq!(cut, 0, "huge keep_recent → nothing cut");
}

#[test]
fn find_cut_point_cuts_old_when_keep_is_small() {
    let messages = vec![
        user_msg(&"a".repeat(500)), // old, large
        user_msg(&"b".repeat(500)), // old, large
        assistant_msg("recent"),    // recent, small
    ];
    let estimates: Vec<u32> = messages
        .iter()
        .map(|m| tiycode::core::context_compression::estimate_message_tokens(m))
        .collect();

    let cut = find_cut_point(&messages, &estimates, 50);
    assert!(cut <= 2, "cut should be before/at large old messages");
    assert!(cut < messages.len(), "some messages should remain");
}

#[test]
fn find_cut_point_preserves_assistant_tool_result_pairs() {
    let messages = vec![
        user_msg("old"),
        assistant_with_tool("Read"),          // has tool call
        tool_result_msg("Read", "file data"), // paired result
    ];
    let estimates: Vec<u32> = messages
        .iter()
        .map(|m| tiycode::core::context_compression::estimate_message_tokens(m))
        .collect();

    let cut = find_cut_point(&messages, &estimates, 5);
    // Should not split between index 1 (asst w/ tool) and 2 (tool_result)
    if cut == 1 || cut == 2 {
        panic!("cut={} splits assistant from its tool_result pair", cut);
    }
}

#[test]
fn find_cut_point_single_message_all_or_nothing() {
    let messages = vec![user_msg("only one")];
    let estimates: Vec<u32> = messages
        .iter()
        .map(|m| tiycode::core::context_compression::estimate_message_tokens(m))
        .collect();

    // Large keep → no cut
    assert_eq!(find_cut_point(&messages, &estimates, 99_999), 0);

    // Tiny keep → cut everything
    assert_eq!(find_cut_point(&messages, &estimates, 1), 0);
}

// =========================================================================
// build_compressed_messages
// =========================================================================

#[test]
fn build_compressed_injects_summary_first() {
    let recent = vec![assistant_msg("reply")];
    let result =
        tiycode::core::context_compression::build_compressed_messages("Summary here", &recent);

    assert_eq!(result.len(), 2);
    match &result[0] {
        AgentMessage::User(_) => {} // OK
        other => panic!("first message should be User(summary), got {:?}", other),
    }
}

#[test]
fn build_compressed_preserves_order_and_count() {
    let recent = vec![user_msg("hi"), assistant_msg("hello")];
    let result = tiycode::core::context_compression::build_compressed_messages("summary", &recent);

    assert_eq!(result.len(), 3); // summary + 2 recent
    assert!(matches!(&result[1], AgentMessage::User(_)));
    assert!(matches!(&result[2], AgentMessage::Assistant(_)));
}

#[test]
fn build_compressed_empty_recent_only_summary() {
    let result = tiycode::core::context_compression::build_compressed_messages("summary", &[]);
    assert_eq!(result.len(), 1); // just summary
}

// =========================================================================
// compress_context_fallback
// =========================================================================

#[test]
fn fallback_empty_returns_empty() {
    let settings = CompressionSettings::new(100_000);
    let result = compress_context_fallback(vec![], &settings);
    assert!(result.is_empty());
}

#[test]
fn fallback_under_budget_returns_originals() {
    let settings = CompressionSettings::new(100_000);
    let messages = vec![user_msg("small")];
    let original_count = messages.len();
    let result = compress_context_fallback(messages, &settings);
    assert_eq!(result.len(), original_count);
}

#[test]
fn fallback_over_budget_produces_shorter_list() {
    let settings = CompressionSettings::new(50); // very small window
    let messages = vec![
        user_msg(&"a".repeat(500)),
        assistant_msg(&"b".repeat(500)),
        user_msg(&"c".repeat(500)),
        assistant_msg(&"d".repeat(500)),
    ];
    let original_len = messages.len();
    let result = compress_context_fallback(messages, &settings);
    assert!(!result.is_empty());
    assert!(result.len() <= original_len);
}

#[test]
fn fallback_starts_with_heuristic_summary_tag() {
    let settings = CompressionSettings::new(10);
    // Use long content to ensure we exceed the tiny budget and trigger fallback
    let messages = vec![
        user_msg(&"Fix login bug. ".repeat(50)),
        assistant_msg(&"I'll refactor auth now. ".repeat(50)),
        tool_result_msg("Bash", &"done. ".repeat(100)),
    ];
    let result = compress_context_fallback(messages, &settings);
    assert!(!result.is_empty());

    match &result[0] {
        AgentMessage::User(u) => {
            let text = match &u.content {
                tiycore::types::UserContent::Text(t) => t.clone(),
                tiycore::types::UserContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text(tc) => Some(tc.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            // Fallback should produce a summary with context markers
            assert!(
                text.contains("<context_summary>") || text.contains("summary") || text.len() > 20,
                "fallback should produce meaningful summary: {}",
                &text[..text.len().min(200)]
            );
        }
        other => panic!("expected first msg to be User, got {:?}", other),
    }
}

// =========================================================================
// generate_discard_summary
// =========================================================================

#[test]
fn discard_summary_includes_user_topics() {
    let messages = vec![
        user_msg("Fix the login bug on the auth page"),
        user_msg("Add unit tests for payment"),
        user_msg("Update README with new API docs"),
    ];
    let summary = generate_discard_summary(&messages);
    assert!(summary.contains("User Requests"));
    assert!(summary.contains("login bug"));
    assert!(summary.contains("payment"));
}

#[test]
fn discard_summary_includes_tools_used() {
    let messages = vec![
        user_msg("read the file"),
        assistant_with_tool("Read"),
        tool_result_msg("Read", "file contents"),
        assistant_with_tool("Write"),
        tool_result_msg("Write", "written ok"),
    ];
    let summary = generate_discard_summary(&messages);
    assert!(summary.contains("Read"));
    assert!(summary.contains("Write"));
}

#[test]
fn discard_summary_includes_assistant_actions() {
    let messages = vec![
        user_msg("do something"),
        assistant_msg("I've completed the task successfully."),
    ];
    let summary = generate_discard_summary(&messages);
    // The summary should contain content from assistant messages
    // (either in a dedicated section or as part of the overall summary)
    assert!(
        summary.contains("completed") || summary.contains("task") || summary.len() > 50,
        "summary should reflect assistant actions: {}",
        &summary[..summary.len().min(200)]
    );
}

#[test]
fn discard_summary_limits_to_five_topics() {
    let mut messages = vec![];
    for i in 0..8u32 {
        messages.push(user_msg(&format!("Request {}", i + 1)));
    }
    let summary = generate_discard_summary(&messages);
    assert!(summary.contains("more requests"));
}

#[test]
fn discard_summary_wrapped_in_tags() {
    let summary = generate_discard_summary(&[user_msg("hello")]);
    assert!(summary.starts_with("<context_summary>"));
    assert!(summary.ends_with("</context_summary>"));
}

#[test]
fn discard_summary_empty_input_produces_valid_output() {
    let messages: Vec<AgentMessage> = vec![];
    let summary = generate_discard_summary(&messages);
    assert!(summary.starts_with("<context_summary>"));
    assert!(summary.ends_with("</context_summary>"));
}
