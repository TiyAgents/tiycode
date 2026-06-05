use async_trait::async_trait;

use crate::core::subagent::SubagentProfile;

use super::build_context::BuildCx;
use super::context::PromptBuildContext;
use super::providers::{BaseProvider, ProfileProvider, SkillsProvider};
use super::section::PromptSectionProvider;
use super::section_source::{FatalError, SectionBody, SectionOutcome, SectionSource};

// ---------------------------------------------------------------------------
// Legacy adapter wrappers retained for sections that still depend on dynamic
// PromptSectionProvider logic (Profile / Skills / Base sections used by
// `Composer::build_main_agent_legacy_compat`). The static-content sections
// (SystemEnvironment, SandboxPermissions, RunMode, WorkspaceLocation,
// ProjectContext) have moved to `template_sources.rs` and no longer go
// through this adapter.
// ---------------------------------------------------------------------------

#[allow(dead_code)] // retained for legacy_compat unit tests
pub struct LegacyRoleSource(pub BaseProvider);
#[allow(dead_code)]
pub struct LegacyBehavioralGuidelinesSource(pub BaseProvider);
#[allow(dead_code)]
pub struct LegacyFinalResponseStructureSource(pub BaseProvider);
#[allow(dead_code)]
pub struct LegacyShellToolingGuideSource(pub super::providers::EnvironmentProvider);
pub struct LegacySkillsSource(pub SkillsProvider);
pub struct LegacyProfileInstructionsSource(pub ProfileProvider);

// ---------------------------------------------------------------------------
// SectionSource implementations via macro
// ---------------------------------------------------------------------------

macro_rules! impl_legacy_source {
    ($wrapper:ty, $section_key:literal) => {
        #[async_trait]
        impl SectionSource for $wrapper {
            async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
                // If raw_plan is None, this source cannot produce output (needs plan context)
                let raw_plan = match cx.raw_plan {
                    Some(plan) => plan,
                    None => return Ok(SectionOutcome::Skip),
                };
                let old_ctx = PromptBuildContext::new(
                    cx.pool,
                    raw_plan,
                    cx.workspace_path,
                    cx.run_mode.as_str(),
                );

                let sections = self
                    .0
                    .collect(&old_ctx)
                    .await
                    .map_err(|e| FatalError::new("legacy.provider", e.to_string()))?;

                match sections.into_iter().find(|s| s.key == $section_key) {
                    Some(section) if !section.body.trim().is_empty() => Ok(
                        SectionOutcome::Produced(SectionBody::markdown(section.body)),
                    ),
                    _ => Ok(SectionOutcome::Skip),
                }
            }
        }
    };
}

impl_legacy_source!(LegacyRoleSource, "role");
impl_legacy_source!(LegacyBehavioralGuidelinesSource, "behavioral_guidelines");
impl_legacy_source!(
    LegacyFinalResponseStructureSource,
    "final_response_structure"
);
impl_legacy_source!(LegacyShellToolingGuideSource, "shell_tooling_guide");
impl_legacy_source!(LegacySkillsSource, "skills");
impl_legacy_source!(LegacyProfileInstructionsSource, "profile_instructions");

// ---------------------------------------------------------------------------
// Subagent-specific sources (direct SectionSource impl, not macro-based)
// ---------------------------------------------------------------------------

pub struct LegacySubagentOutputContractSource;

#[async_trait]
impl SectionSource for LegacySubagentOutputContractSource {
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let body = match cx.helper_profile {
            Some(SubagentProfile::Explore) => {
                "Your output will be consumed by the parent agent, not the user. Follow any response language and response style instructions inherited above unless the parent explicitly overrides them. If the inherited prompt specifies a response language, write your entire output in that language. Produce a concise, structured summary. Lead with the key conclusion, then supporting details. Reference specific file paths and code locations where relevant. Skip preamble."
            }
            Some(SubagentProfile::Review) => {
                "Your output will be consumed by the parent agent, not the user. Follow any response language instructions inherited above unless the parent explicitly overrides them. If the inherited prompt specifies a response language, use that language in all natural-language JSON fields. Follow the review helper's JSON contract exactly. Do not add markdown fences, headings, or prose outside the JSON object."
            }
            Some(SubagentProfile::Custom { .. }) => {
                "Your output will be consumed by the parent agent, not the user. Produce a concise, structured summary. Lead with the key conclusion, then supporting details. Reference specific file paths and code locations where relevant. Skip preamble."
            }
            None => return Ok(SectionOutcome::Skip),
        };
        Ok(SectionOutcome::Produced(SectionBody::markdown(body)))
    }
}

pub struct LegacyCustomSubagentBodySource;

#[async_trait]
impl SectionSource for LegacyCustomSubagentBodySource {
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let system_prompt = match cx.helper_profile {
            Some(SubagentProfile::Custom { system_prompt, .. }) => system_prompt.as_str(),
            _ => return Ok(SectionOutcome::Skip),
        };
        if system_prompt.trim().is_empty() {
            return Ok(SectionOutcome::Skip);
        }
        Ok(SectionOutcome::Produced(SectionBody::markdown(
            system_prompt,
        )))
    }
}

// ── Title contract source ─────────────────────────────────────────

pub struct LegacyTitleContractSource;

#[async_trait]
impl SectionSource for LegacyTitleContractSource {
    async fn build(&self, _cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        Ok(SectionOutcome::Produced(SectionBody::markdown(
            "You write concise conversation titles. Return only the title text.",
        )))
    }
}

// ── Compaction contract source ────────────────────────────────────

pub struct LegacyCompactionContractSource;

#[async_trait]
impl SectionSource for LegacyCompactionContractSource {
    fn source_kind(&self) -> &'static str {
        "compaction_contract"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        // Mirror agent_run_summary::build_compact_summary_system_prompt exactly so
        // that switching the call site to Composer produces byte-equal output.
        let kind = match cx_compaction_kind(cx) {
            Some(k) => k,
            None => return Ok(SectionOutcome::Skip),
        };

        let body = match kind {
            super::surface::CompactionKind::Compact => render_compact_body(cx.response_language),
            super::surface::CompactionKind::Merge => render_merge_body(cx.response_language),
        };

        Ok(SectionOutcome::Produced(SectionBody::markdown(body)))
    }
}

/// Probe BuildCx to find the active compaction kind.
/// Currently we encode it via response_language presence + a dedicated marker
/// in custom_subagent_slug — but a cleaner path is to read it from a future
/// BuildCx field. For now, callers must wrap their build with a BuildCx that
/// has helper_profile=None and custom_subagent_slug carrying "compact"/"merge".
fn cx_compaction_kind(cx: &BuildCx<'_>) -> Option<super::surface::CompactionKind> {
    match cx.custom_subagent_slug {
        Some("__compact__") => Some(super::surface::CompactionKind::Compact),
        Some("__merge__") => Some(super::surface::CompactionKind::Merge),
        _ => None,
    }
}

fn render_compact_body(response_language: Option<&str>) -> String {
    let mut lines = vec![
        "You compress conversation state so another model can continue after context reset.".to_string(),
        "Return only one compact summary block using the exact XML-style wrapper below.".to_string(),
        String::new(),
        "Requirements:".to_string(),
        "- Preserve the user's current goal and latest requested outcome.".to_string(),
        "- Preserve important constraints, preferences, and decisions.".to_string(),
        "- List work already completed and important findings.".to_string(),
        "- List the most relevant remaining tasks, open questions, or risks.".to_string(),
        "- Mention key files, components, commands, tools, or errors only when they matter for continuation.".to_string(),
        "- Be factual and concise. Do not invent details.".to_string(),
        "- Do not address the user directly. Do not include greetings or commentary.".to_string(),
        "- Prefer short bullet lists under clear section labels.".to_string(),
        "- Keep the summary self-contained and suitable for direct insertion into future model context.".to_string(),
    ];

    if let Some(language) =
        crate::core::agent_session::normalize_profile_response_language(response_language)
    {
        lines.push(format!(
            "- Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    lines.extend([
        String::new(),
        "Output rules:".to_string(),
        "- Start with <context_summary> on its own line.".to_string(),
        "- End with </context_summary> on its own line.".to_string(),
        "- Do not output any text before or after the wrapper.".to_string(),
        String::new(),
        "Example output:".to_string(),
        "<context_summary>".to_string(),
        "- User goal: Stabilize /compact summary formatting.".to_string(),
        "- Completed: Checked current local summarization flow and wrapper handling.".to_string(),
        "- Remaining: Move compact rules into system prompt and keep output parsing robust."
            .to_string(),
        "</context_summary>".to_string(),
    ]);

    lines.join("\n")
}

fn render_merge_body(response_language: Option<&str>) -> String {
    let mut lines = vec![
        "You maintain a rolling context summary for another model to continue after context reset."
            .to_string(),
        "You will be given the PRIOR summary (already in <context_summary> form) and a DELTA of conversation"
            .to_string(),
        "that happened after that summary was last produced. Produce a SINGLE updated <context_summary>"
            .to_string(),
        "that merges both — keeping still-relevant facts from the prior summary and folding in new information"
            .to_string(),
        "from the delta. Treat the prior summary as authoritative for anything it covers and do not drop"
            .to_string(),
        "details that remain pertinent.".to_string(),
        String::new(),
        "Requirements:".to_string(),
        "- Preserve the user's current goal and most recent requested outcome.".to_string(),
        "- Retain important constraints, preferences, and decisions from the prior summary unless the delta"
            .to_string(),
        "  explicitly supersedes them.".to_string(),
        "- Fold newly completed work, findings, key files/commands, and remaining tasks from the delta in."
            .to_string(),
        "- Drop items the delta marks resolved; add items the delta newly raises.".to_string(),
        "- Be factual and concise. Do not invent details. Do not address the user.".to_string(),
        "- Prefer short bullet lists under clear section labels.".to_string(),
    ];

    if let Some(language) =
        crate::core::agent_session::normalize_profile_response_language(response_language)
    {
        lines.push(format!(
            "- Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    lines.extend([
        String::new(),
        "Output rules:".to_string(),
        "- Start with <context_summary> on its own line.".to_string(),
        "- End with </context_summary> on its own line.".to_string(),
        "- Do not output any text before or after the wrapper.".to_string(),
    ]);

    lines.join("\n")
}
