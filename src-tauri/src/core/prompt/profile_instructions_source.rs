use async_trait::async_trait;

use crate::persistence::repo::profile_repo;

use super::build_context::BuildCx;
use super::section_source::{FatalError, SectionBody, SectionOutcome, SectionSource};

/// Template-backed SectionSource for Profile Instructions.
/// Replaces LegacyProfileInstructionsSource (which delegated to the old ProfileProvider).
///
/// Builds the profile instructions body from:
/// 1. Runtime custom_instructions (from raw_plan)
/// 2. Runtime response_language / response_style
/// 3. Database profile defaults (for missing fields)
pub struct ProfileInstructionsSource;

#[async_trait]
impl SectionSource for ProfileInstructionsSource {
    fn source_kind(&self) -> &'static str {
        "profile_instructions"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw_plan = match cx.raw_plan {
            Some(plan) => plan,
            None => return Ok(SectionOutcome::Skip),
        };

        let mut lines: Vec<String> = Vec::new();

        // 1. Custom instructions from runtime
        if let Some(custom_instructions) = raw_plan.custom_instructions.as_deref() {
            let trimmed = custom_instructions.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }

        // 2. Response instruction from runtime
        let mut response_parts = build_response_parts(
            raw_plan.response_language.as_deref(),
            raw_plan.response_style.as_deref(),
        );
        let runtime_has_language =
            crate::core::agent_session_types::normalize_profile_response_language(
                raw_plan.response_language.as_deref(),
            )
            .is_some();
        let runtime_has_style = raw_plan
            .response_style
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());

        // 3. Fall back to database profile for missing fields
        if let Some(profile_id) = raw_plan.profile_id.as_deref() {
            if let Ok(Some(profile)) = profile_repo::find_by_id(cx.pool, profile_id).await {
                // Fallback custom_instructions
                if lines.is_empty() {
                    if let Some(custom_instructions) = profile.custom_instructions.as_deref() {
                        let trimmed = custom_instructions.trim();
                        if !trimmed.is_empty() {
                            lines.push(trimmed.to_string());
                        }
                    }
                }

                // Fallback response_language
                if !runtime_has_language {
                    if let Some(language) =
                        crate::core::agent_session_types::normalize_profile_response_language(
                            profile.response_language.as_deref(),
                        )
                    {
                        response_parts.insert(
                            0,
                            format!("Respond in {language} unless the user explicitly asks for a different language."),
                        );
                    }
                }

                // Fallback response_style
                if !runtime_has_style {
                    response_parts = build_response_parts(
                        if runtime_has_language {
                            raw_plan.response_language.as_deref()
                        } else {
                            profile.response_language.as_deref()
                        },
                        profile.response_style.as_deref(),
                    );
                }
            }
        }

        lines.extend(response_parts);

        if lines.is_empty() {
            return Ok(SectionOutcome::Skip);
        }

        let body = lines.join("\n");

        Ok(SectionOutcome::Produced(SectionBody::markdown(body)))
    }
}

/// Build the response instruction parts from runtime values.
fn build_response_parts(
    response_language: Option<&str>,
    response_style: Option<&str>,
) -> Vec<String> {
    use crate::core::agent_session_types::{
        normalize_profile_response_language, normalize_profile_response_style,
        response_style_system_instruction,
    };

    let mut parts = Vec::new();

    if let Some(language) = normalize_profile_response_language(response_language) {
        parts.push(format!(
            "Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    parts.push(
        response_style_system_instruction(normalize_profile_response_style(response_style))
            .to_string(),
    );

    parts
}
