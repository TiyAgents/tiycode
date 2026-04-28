use std::collections::HashMap;

use tiycore::agent::AgentTool;
use tiycore::thinking::ThinkingLevel;
use tiycore::types::{Model, OpenAICompletionsCompat, Transport};

use crate::core::context_compression::ContextTokenCalibration;
use crate::model::provider::AgentProfileRecord;
use crate::model::thread::{MessageRecord, ToolCallDto};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileResponseStyle {
    Balanced,
    Concise,
    Guide,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelRole {
    pub provider_id: String,
    pub model_record_id: String,
    pub provider: Option<String>,
    pub provider_key: Option<String>,
    pub provider_type: String,
    pub provider_name: Option<String>,
    pub model: String,
    pub model_id: String,
    pub model_display_name: Option<String>,
    pub base_url: String,
    pub context_window: Option<String>,
    pub max_output_tokens: Option<String>,
    pub supports_image_input: Option<bool>,
    pub supports_reasoning: Option<bool>,
    pub custom_headers: Option<HashMap<String, String>>,
    pub provider_options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelPlan {
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub custom_instructions: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub primary: Option<RuntimeModelRole>,
    pub auxiliary: Option<RuntimeModelRole>,
    pub lightweight: Option<RuntimeModelRole>,
    pub thinking_level: Option<String>,
    pub transport: Option<String>,
    pub tool_profile_by_mode: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ResolvedModelRole {
    pub provider_id: String,
    pub model_record_id: String,
    pub model_id: String,
    pub model_name: String,
    pub provider_type: String,
    pub provider_name: String,
    pub api_key: Option<String>,
    pub provider_options: Option<serde_json::Value>,
    pub model: Model,
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntimeModelPlan {
    pub raw: RuntimeModelPlan,
    pub primary: ResolvedModelRole,
    pub auxiliary: Option<ResolvedModelRole>,
    pub lightweight: Option<ResolvedModelRole>,
    pub thinking_level: ThinkingLevel,
    pub transport: Transport,
}

#[derive(Debug, Clone)]
pub struct AgentSessionSpec {
    pub run_id: String,
    pub thread_id: String,
    pub workspace_path: String,
    pub run_mode: String,
    pub tool_profile_name: String,
    pub runtime_tools: Vec<AgentTool>,
    pub system_prompt: String,
    pub history_messages: Vec<MessageRecord>,
    pub history_tool_calls: Vec<ToolCallDto>,
    pub model_plan: ResolvedRuntimeModelPlan,
    pub initial_prompt: Option<String>,
    pub initial_context_calibration: ContextTokenCalibration,
}

pub(crate) fn default_openai_compatible_compat(
    provider_type: &str,
) -> Option<OpenAICompletionsCompat> {
    if !provider_type.eq_ignore_ascii_case("openai-compatible") {
        return None;
    }

    let mut compat = OpenAICompletionsCompat::default();
    compat.supports_developer_role = false;
    Some(compat)
}

pub fn build_profile_response_prompt_parts(profile: &AgentProfileRecord) -> Vec<String> {
    build_profile_response_prompt_parts_from_runtime(
        profile.response_language.as_deref(),
        profile.response_style.as_deref(),
    )
}

pub(crate) fn build_profile_response_prompt_parts_from_runtime(
    response_language: Option<&str>,
    response_style: Option<&str>,
) -> Vec<String> {
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

pub fn normalize_profile_response_language(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn normalize_profile_response_style(value: Option<&str>) -> ProfileResponseStyle {
    match value.unwrap_or("balanced").trim().to_lowercase().as_str() {
        "concise" => ProfileResponseStyle::Concise,
        "guide" | "guided" => ProfileResponseStyle::Guide,
        _ => ProfileResponseStyle::Balanced,
    }
}

pub fn response_style_system_instruction(style: ProfileResponseStyle) -> &'static str {
    match style {
        ProfileResponseStyle::Balanced => {
            "Response style: balanced. Default to a compact but complete answer. Lead with the answer or outcome first. Use a short paragraph or a short flat list when that makes the reply clearer. Add explanation when it materially helps understanding, but avoid over-explaining routine details. Each point should be a complete thought expressed in a full sentence, not a bare noun phrase or keyword fragment. When multiple points share a single theme, consolidate them into one paragraph rather than scattering them across separate bullets."
        }
        ProfileResponseStyle::Concise => {
            "Response style: concise. Treat brevity as a hard default. Lead with the answer, result, or next action immediately. Keep the final response to 1-3 short sentences or a very short flat list unless the user explicitly asks for more detail. Do not include background, reasoning, summaries, or pleasantries unless they are required for correctness. Prefer code, commands, and direct facts over prose."
        }
        ProfileResponseStyle::Guide => {
            "Response style: guided. Lead with the answer, then explain the reasoning, tradeoffs, and recommended next steps clearly. Be intentionally explanatory when that helps the user learn or make a decision. Surface relevant alternatives, caveats, or examples when useful."
        }
    }
}

pub(crate) fn parse_positive_u32(value: Option<&str>, fallback: u32) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub(crate) fn parse_transport(value: Option<&str>) -> Transport {
    match value.unwrap_or("sse").trim().to_lowercase().as_str() {
        "websocket" | "ws" => Transport::WebSocket,
        "auto" => Transport::Auto,
        _ => Transport::Sse,
    }
}

pub(crate) fn normalize_provider_options(
    value: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    value.and_then(|value| match value {
        serde_json::Value::Object(map) if map.is_empty() => None,
        serde_json::Value::Object(_) => Some(value),
        _ => None,
    })
}
