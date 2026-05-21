use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Model role selection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomSubagentModelRole {
    Primary,
    Auxiliary,
    Lightweight,
}

impl Default for CustomSubagentModelRole {
    fn default() -> Self {
        Self::Auxiliary
    }
}

impl CustomSubagentModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Auxiliary => "auxiliary",
            Self::Lightweight => "lightweight",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "primary" => Self::Primary,
            "lightweight" => Self::Lightweight,
            _ => Self::Auxiliary,
        }
    }
}

// ---------------------------------------------------------------------------
// Database row type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CustomSubagentRecord {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub system_prompt: String,
    pub invocation_description: String,
    pub allowed_tools: String, // JSON array string, e.g. '["read","list","search"]'
    pub model_role: CustomSubagentModelRole,
    pub is_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl CustomSubagentRecord {
    /// Parse the `allowed_tools` JSON string into a Vec of tool names.
    pub fn allowed_tools_vec(&self) -> Vec<String> {
        serde_json::from_str(&self.allowed_tools).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// DTO sent to the frontend (camelCase)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomSubagentDto {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub system_prompt: String,
    pub invocation_description: String,
    pub allowed_tools: Vec<String>,
    pub model_role: CustomSubagentModelRole,
    pub is_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CustomSubagentRecord> for CustomSubagentDto {
    fn from(r: CustomSubagentRecord) -> Self {
        let tools = r.allowed_tools_vec();
        Self {
            id: r.id,
            name: r.name,
            slug: r.slug,
            system_prompt: r.system_prompt,
            invocation_description: r.invocation_description,
            allowed_tools: tools,
            model_role: r.model_role,
            is_enabled: r.is_enabled,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Input from the frontend for create/update
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomSubagentInput {
    pub name: String,
    pub slug: String,
    pub system_prompt: String,
    pub invocation_description: String,
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub model_role: CustomSubagentModelRole,
    pub is_enabled: Option<bool>,
}

// ---------------------------------------------------------------------------
// Profile ↔ Subagent access record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProfileSubagentAccessRecord {
    pub profile_id: String,
    pub subagent_id: String,
}

// ---------------------------------------------------------------------------
// Reserved slugs that cannot be used for custom subagents
// ---------------------------------------------------------------------------

pub const RESERVED_SUBAGENT_SLUGS: &[&str] = &["explore", "review"];

/// Validate that a slug is well-formed and not reserved.
pub fn validate_slug(slug: &str) -> Result<(), &'static str> {
    if slug.is_empty() {
        return Err("slug cannot be empty");
    }
    if RESERVED_SUBAGENT_SLUGS.contains(&slug) {
        return Err("slug is reserved for built-in agents");
    }
    // Allow only lowercase alphanumeric + hyphens, must start with a letter
    let mut chars = slug.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return Err("slug must start with a lowercase letter"),
    }
    for c in chars {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' {
            return Err("slug may only contain lowercase letters, digits, and hyphens");
        }
    }
    Ok(())
}
