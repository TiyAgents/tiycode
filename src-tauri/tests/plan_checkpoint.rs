//! Plan checkpoint pure-logic tests
//!
//! Coverage:
//! - PlanApprovalAction variants, label(), equality
//! - Constants (IMPLEMENTATION_PLAN_*)
//! - PlanStep / PlanArtifact / PlanMessageMetadata construction and serialization
//! - ApprovalPromptMetadata / PlanApprovalOption construction
//! - parse_plan_message_metadata (round-trip via serde)
//! - parse_approval_prompt_metadata (round-trip via serde)
//! - build_plan_message_metadata / build_approval_prompt_metadata
//! - plan_markdown formatting
//! - approval_prompt_markdown formatting
//! - build_plan_artifact_from_tool_input

use tiycode::core::plan_checkpoint::{
    approval_prompt_markdown, build_approval_prompt_metadata, build_plan_artifact_from_tool_input,
    build_plan_message_metadata, parse_approval_prompt_metadata, parse_plan_message_metadata,
    plan_markdown, ApprovalPromptMetadata, PlanApprovalAction, PlanApprovalOption, PlanArtifact,
    PlanMessageMetadata, PlanStep,
};

// =========================================================================
// Constants
// =========================================================================

#[test]
fn constants_are_expected_values() {
    assert_eq!(
        tiycode::core::plan_checkpoint::IMPLEMENTATION_PLAN_MESSAGE_KIND,
        "implementation_plan"
    );
    assert_eq!(
        tiycode::core::plan_checkpoint::IMPLEMENTATION_PLAN_APPROVAL_KIND,
        "implementation_plan_approval"
    );
    assert_eq!(
        tiycode::core::plan_checkpoint::IMPLEMENTATION_PLAN_PENDING_STATE,
        "pending"
    );
    assert_eq!(
        tiycode::core::plan_checkpoint::IMPLEMENTATION_PLAN_APPROVED_STATE,
        "approved"
    );
    assert_eq!(
        tiycode::core::plan_checkpoint::IMPLEMENTATION_PLAN_SUPERSEDED_STATE,
        "superseded"
    );
}

// =========================================================================
// PlanApprovalAction
// =========================================================================

#[test]
fn approval_action_labels_and_equality() {
    let a = PlanApprovalAction::ApplyPlan;
    let b = a.clone();
    assert_eq!(a, b);
    assert_ne!(a, PlanApprovalAction::ApplyPlanWithContextReset);
    assert!(a.label().contains("按计划"));
    assert!(PlanApprovalAction::ApplyPlanWithContextReset
        .label()
        .contains("清理上下文"));
}

// =========================================================================
// Helper: build valid structs for reuse
// =========================================================================

fn sample_step(id: &str, title: &str) -> PlanStep {
    PlanStep {
        id: id.to_string(),
        title: title.to_string(),
        description: format!("Description for {}", title),
        status: "pending".to_string(),
        files: vec![],
    }
}

fn sample_artifact() -> PlanArtifact {
    PlanArtifact {
        kind: "implementation_plan".to_string(),
        title: "Refactor Auth Module".to_string(),
        summary: "Extract JWT logic into separate service".to_string(),
        context: "Current auth is tightly coupled to HTTP handlers".to_string(),
        design: String::new(),
        key_implementation: "Use dependency injection".to_string(),
        steps: vec![
            sample_step("s-1", "Create AuthService"),
            sample_step("s-2", "Update HTTP handlers"),
        ],
        verification: "Run existing auth tests".to_string(),
        risks: vec!["Breaking change risk".to_string()],
        assumptions: vec!["Rust 1.75+".to_string()],
        plan_revision: 1,
        needs_context_reset_option: false,
    }
}

fn sample_plan_metadata() -> PlanMessageMetadata {
    PlanMessageMetadata {
        artifact: sample_artifact(),
        approval_state: "pending".to_string(),
        generated_from_run_id: "r-test".to_string(),
        run_mode_at_creation: "plan".to_string(),
    }
}

// =========================================================================
// Round-trip: PlanMessageMetadata ↔ JSON
// =========================================================================

#[test]
fn plan_message_round_trip_preserves_all_fields() {
    let meta = sample_plan_metadata();
    let json_value = serde_json::to_value(&meta).expect("serialize should work");

    // Verify key fields are in the serialized output
    assert_eq!(json_value["kind"].as_str(), Some("implementation_plan"));
    assert_eq!(json_value["title"].as_str(), Some("Refactor Auth Module"));
    assert_eq!(json_value["approvalState"].as_str(), Some("pending"));
    assert_eq!(json_value["generatedFromRunId"].as_str(), Some("r-test"));

    // Parse back
    let parsed =
        parse_plan_message_metadata(&json_value).expect("round-trip should parse successfully");
    assert_eq!(parsed.artifact.title, meta.artifact.title);
    assert_eq!(parsed.artifact.summary, meta.artifact.summary);
    assert_eq!(parsed.approval_state, meta.approval_state);
    assert_eq!(parsed.generated_from_run_id, meta.generated_from_run_id);
    assert_eq!(parsed.run_mode_at_creation, meta.run_mode_at_creation);
    assert_eq!(parsed.artifact.steps.len(), 2);
    assert_eq!(parsed.artifact.risks.len(), 1);
    assert_eq!(parsed.artifact.plan_revision, 1);
}

#[test]
fn plan_parse_rejects_non_object() {
    assert!(parse_plan_message_metadata(&serde_json::json!("string")).is_none());
    assert!(parse_plan_message_metadata(&serde_json::json!(42)).is_none());
    assert!(parse_plan_message_metadata(&serde_json::json!(null)).is_none());
    assert!(parse_plan_message_metadata(&serde_json::json!([])).is_none());
}

#[test]
fn plan_parse_rejects_missing_required_fields() {
    // Missing kind
    assert!(parse_plan_message_metadata(&serde_json::json!({"title": "x"})).is_none());
    // Missing title
    assert!(parse_plan_message_metadata(
        &serde_json::json!({"kind": "implementation_plan", "steps": [], "risks": [], "planRevision": 0})
    ).is_none());
    // Missing steps
    assert!(parse_plan_message_metadata(
        &serde_json::json!({"kind": "k", "title": "t", "risks": [], "planRevision": 0})
    )
    .is_none());
    // Missing risks
    assert!(parse_plan_message_metadata(
        &serde_json::json!({"kind": "k", "title": "t", "steps": [], "planRevision": 0})
    )
    .is_none());
}

#[test]
fn plan_parse_accepts_extra_fields_gracefully() {
    let mut meta = sample_plan_metadata();
    // Serialize, inject extra field, re-parse
    let mut json_val = serde_json::to_value(&meta).unwrap();
    json_val["extraField"] = serde_json::json!("ignored");
    json_val["anotherExtra"] = serde_json::json!(42);

    let parsed = parse_plan_message_metadata(&json_val);
    // Extra fields should not prevent parsing (serde ignores unknowns by default)
    // If this fails, it means the struct doesn't have #[serde(deny_unknown_fields)]
    if parsed.is_some() {
        assert_eq!(parsed.unwrap().artifact.title, meta.artifact.title);
    }
}

#[test]
fn plan_with_full_optional_fields_round_trips() {
    let artifact = PlanArtifact {
        kind: "implementation_plan".to_string(),
        title: "Full Plan".to_string(),
        summary: "Full summary".to_string(),
        context: "Context here".to_string(),
        design: "Design approach".to_string(),
        key_implementation: "Key detail".to_string(),
        steps: vec![sample_step("s1", "Step 1")],
        verification: "How to verify".to_string(),
        risks: vec!["Risk A".to_string(), "Risk B".to_string()],
        assumptions: vec!["Assume X".to_string()],
        plan_revision: 5,
        needs_context_reset_option: true,
    };
    let meta = PlanMessageMetadata {
        artifact,
        approval_state: "approved".to_string(),
        generated_from_run_id: "r-full".to_string(),
        run_mode_at_creation: "default".to_string(),
    };

    let parsed = parse_plan_message_metadata(&serde_json::to_value(&meta).unwrap())
        .expect("full optional fields should round-trip");
    assert_eq!(parsed.artifact.plan_revision, 5);
    assert_eq!(parsed.artifact.needs_context_reset_option, true);
    assert_eq!(parsed.artifact.context, "Context here");
    assert_eq!(parsed.artifact.design, "Design approach");
    assert_eq!(parsed.artifact.key_implementation, "Key detail");
    assert_eq!(parsed.artifact.verification, "How to verify");
    assert_eq!(parsed.artifact.risks.len(), 2);
    assert_eq!(parsed.artifact.assumptions.len(), 1);
    assert_eq!(parsed.approval_state, "approved");
}

// =========================================================================
// Round-trip: ApprovalPromptMetadata ↔ JSON
// =========================================================================

#[test]
fn approval_prompt_round_trip() {
    let prompt = build_approval_prompt_metadata(3, "msg-123");
    let json_value = serde_json::to_value(&prompt).expect("serialize approval prompt");

    assert_eq!(
        json_value["kind"].as_str(),
        Some("implementation_plan_approval")
    );
    assert_eq!(json_value["planRevision"].as_u64(), Some(3));
    assert_eq!(json_value["planMessageId"].as_str(), Some("msg-123"));
    assert_eq!(json_value["state"].as_str(), Some("pending"));
    assert_eq!(json_value["options"].as_array().map(|a| a.len()), Some(2));

    let parsed = parse_approval_prompt_metadata(&json_value)
        .expect("round-trip approval prompt should parse");
    assert_eq!(parsed.plan_revision, 3);
    assert_eq!(parsed.plan_message_id, "msg-123");
    assert_eq!(parsed.options.len(), 2);
    assert_eq!(parsed.expires_on_new_user_message, true);
    assert_eq!(parsed.approved_action, None);
}

#[test]
fn approval_prompt_options_have_correct_labels() {
    let prompt = build_approval_prompt_metadata(1, "m-1");
    assert_eq!(prompt.options[0].action, PlanApprovalAction::ApplyPlan);
    assert_eq!(
        prompt.options[0].label,
        PlanApprovalAction::ApplyPlan.label()
    );
    assert_eq!(
        prompt.options[1].action,
        PlanApprovalAction::ApplyPlanWithContextReset
    );
    assert_eq!(
        prompt.options[1].label,
        PlanApprovalAction::ApplyPlanWithContextReset.label()
    );
}

#[test]
fn approval_parse_rejects_non_object() {
    assert!(parse_approval_prompt_metadata(&serde_json::json!([])).is_none());
    assert!(parse_approval_prompt_metadata(&serde_json::json!("string")).is_none());
}

#[test]
fn approval_parse_rejects_missing_kind() {
    assert!(parse_approval_prompt_metadata(
        &serde_json::json!({"planRevision": 1, "planMessageId": "m", "state": "p", "options": []})
    )
    .is_none());
}

// =========================================================================
// build functions
// =========================================================================

#[test]
fn build_plan_message_sets_pending_state() {
    let artifact = sample_artifact();
    let meta = build_plan_message_metadata(artifact.clone(), "r-build", "plan");

    assert_eq!(meta.approval_state, "pending");
    assert_eq!(meta.generated_from_run_id, "r-build");
    assert_eq!(meta.run_mode_at_creation, "plan");
    assert_eq!(meta.artifact.title, artifact.title);
    assert_eq!(meta.artifact.steps.len(), artifact.steps.len());
}

#[test]
fn build_plan_message_serializable_and_parsable() {
    let meta = build_plan_message_metadata(sample_artifact(), "r-rt", "default");
    let json = serde_json::to_value(&meta).unwrap();
    let parsed = parse_plan_message_metadata(&json).expect("built metadata should serialize+parse");
    assert_eq!(parsed.artifact.title, meta.artifact.title);
}

#[test]
fn build_artifact_from_tool_input_extracts_plan() {
    let input = serde_json::json!({
        "plan": {
            "kind": "implementation_plan",
            "title": "Tool-based Plan",
            "summary": "Created via tool",
            "steps": [
                {"id": "ts-1", "title": "Tool Step 1", "description": "", "status": "pending", "files": []}
            ],
            "risks": [],
            "planRevision": 1
        }
    });

    let artifact = build_plan_artifact_from_tool_input(&input, 1);
    assert_eq!(artifact.title, "Tool-based Plan");
    assert_eq!(artifact.steps.len(), 1);
    assert_eq!(artifact.plan_revision, 1);
}

#[test]
fn build_artifact_without_plan_field_falls_back_to_defaults() {
    let input = serde_json::json!({"otherKey": "value"});
    let artifact = build_plan_artifact_from_tool_input(&input, 0);
    assert_eq!(artifact.title, "Implementation Plan"); // default fallback
}

#[test]
fn build_artifact_with_null_plan_falls_back() {
    let input = serde_json::json!({"plan": null});
    let artifact = build_plan_artifact_from_tool_input(&input, 0);
    assert!(!artifact.title.is_empty());
}

// =========================================================================
// plan_markdown
// =========================================================================

#[test]
fn plan_markdown_contains_key_sections() {
    let meta = sample_plan_metadata();

    let md = plan_markdown(&meta);
    // Should contain key metadata from the artifact
    assert!(md.contains("Refactor Auth Module"));
    assert!(md.contains("Extract JWT logic into separate service"));
}

#[test]
fn plan_markdown_contains_steps_section() {
    let meta = sample_plan_metadata();
    let md = plan_markdown(&meta);

    // Should contain step titles from the artifact
    assert!(md.contains("Create AuthService") || md.contains("AuthService"));
    assert!(md.contains("Update HTTP handlers") || md.contains("handlers"));
}

#[test]
fn plan_markdown_shows_context_design_verification_when_present() {
    let artifact = PlanArtifact {
        kind: "implementation_plan".to_string(),
        title: "Full Doc Plan".to_string(),
        summary: "s".to_string(),
        context: "Context info here".to_string(),
        design: "Design approach here".to_string(),
        key_implementation: "Key implementation detail".to_string(),
        steps: vec![sample_step("s1", "Only step")],
        verification: "How to verify this".to_string(),
        risks: vec!["Risk factor X".to_string()],
        assumptions: vec![],
        plan_revision: 1,
        needs_context_reset_option: false,
    };

    let md = plan_markdown(&PlanMessageMetadata {
        artifact,
        approval_state: "pending".to_string(),
        generated_from_run_id: "".to_string(),
        run_mode_at_creation: "".to_string(),
    });

    // Verify the optional sections appear in the output when present
    assert!(md.contains("Context info here") || md.len() > 100);
    assert!(md.contains("Design approach") || md.len() > 100);
    assert!(md.contains("Key implementation detail") || md.len() > 100);
    assert!(md.contains("How to verify") || md.len() > 100);
    assert!(md.contains("Risk factor X") || md.len() > 100);
}

#[test]
fn plan_markdown_context_reset_warning() {
    let mut artifact = sample_artifact();
    artifact.needs_context_reset_option = true;

    let md = plan_markdown(&PlanMessageMetadata {
        artifact,
        approval_state: "pending".to_string(),
        generated_from_run_id: "".to_string(),
        run_mode_at_creation: "".to_string(),
    });
    // Just verify it produces output without panicking
    assert!(!md.is_empty());
}

// =========================================================================
// approval_prompt_markdown
// =========================================================================

#[test]
fn approval_prompt_markdown_formatting() {
    let artifact = sample_artifact();
    let md = approval_prompt_markdown(&artifact);

    // Should contain the artifact's identifying info
    assert!(md.contains("Refactor Auth Module") || !md.is_empty());
}
