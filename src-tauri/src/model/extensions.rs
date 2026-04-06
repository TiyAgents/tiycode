use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionKind {
    Plugin,
    Mcp,
    Skill,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionInstallState {
    Discovered,
    Installed,
    Enabled,
    Disabled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionHealth {
    Unknown,
    Healthy,
    Degraded,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ExtensionSourceDto {
    Builtin,
    LocalDir { path: String },
    Marketplace { listing_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionSummaryDto {
    pub id: String,
    pub kind: ExtensionKind,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub source: ExtensionSourceDto,
    pub install_state: ExtensionInstallState,
    pub health: ExtensionHealth,
    pub permissions: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolDto {
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_ms: Option<u64>,
    pub required_permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandDto {
    pub name: String,
    pub description: String,
    pub prompt_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookGroupDto {
    pub event: String,
    pub handlers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetailDto {
    pub id: String,
    pub path: String,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub default_enabled: bool,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub permissions: Vec<String>,
    pub hooks: Vec<PluginHookGroupDto>,
    pub tools: Vec<PluginToolDto>,
    pub commands: Vec<PluginCommandDto>,
    pub bundled_skills: Vec<String>,
    pub bundled_mcp_servers: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub skills_dir: Option<String>,
    pub config_schema_path: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigInput {
    pub id: String,
    pub label: String,
    pub transport: String,
    pub enabled: bool,
    pub auto_start: bool,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigDto {
    pub id: String,
    pub label: String,
    pub transport: String,
    pub enabled: bool,
    pub auto_start: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub headers: std::collections::HashMap<String, String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolSummaryDto {
    pub name: String,
    pub qualified_name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceSummaryDto {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStateDto {
    pub id: String,
    pub label: String,
    pub scope: String,
    pub status: String,
    pub phase: String,
    pub tools: Vec<McpToolSummaryDto>,
    pub resources: Vec<McpResourceSummaryDto>,
    pub stale_snapshot: bool,
    pub last_error: Option<String>,
    pub updated_at: String,
    pub config: McpServerConfigDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecordDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub triggers: Vec<String>,
    pub tools: Vec<String>,
    pub priority: Option<String>,
    pub source: String,
    pub path: String,
    pub enabled: bool,
    pub pinned: bool,
    pub scope: String,
    pub content_preview: String,
    pub prompt_budget_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPreviewDto {
    pub record: SkillRecordDto,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionDetailDto {
    pub summary: ExtensionSummaryDto,
    pub plugin: Option<PluginDetailDto>,
    pub mcp: Option<McpServerStateDto>,
    pub skill: Option<SkillRecordDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandDto {
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub prompt_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionActivityEventDto {
    pub id: String,
    pub source: String,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourceDto {
    pub id: String,
    pub name: String,
    pub url: String,
    pub builtin: bool,
    pub kind: String,
    pub status: String,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub plugin_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourceInputDto {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourcePluginRefDto {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceRemoveSourcePlanDto {
    pub source: MarketplaceSourceDto,
    pub can_remove: bool,
    pub blocking_plugins: Vec<MarketplaceSourcePluginRefDto>,
    pub removable_installed_plugins: Vec<MarketplaceSourcePluginRefDto>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceItemDto {
    pub id: String,
    pub source_id: String,
    pub source_name: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub description: String,
    pub publisher: String,
    pub tags: Vec<String>,
    pub hooks: Vec<PluginHookGroupDto>,
    pub command_names: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub skill_names: Vec<String>,
    pub path: String,
    pub installable: bool,
    pub installed: bool,
    pub enabled: bool,
}
