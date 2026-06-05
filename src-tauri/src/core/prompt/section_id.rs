/// Typed section identifiers replacing the old `&'static str` key pattern.
/// Each variant identifies exactly one prompt section.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SectionId {
    /// Agent identity statement
    Role,
    /// Tool usage, delegation, and communication rules
    BehavioralGuidelines,
    /// How to structure the final response
    FinalResponseStructure,
    /// Shell command selection and boundary guide
    ShellToolingGuide,
    /// Available skills listing
    Skills,
    /// OS / architecture / shell (no date)
    SystemEnvironment,
    /// Sandbox policy and writable roots
    SandboxPermissions,
    /// Workspace-level AGENTS.md instructions
    ProjectContext,
    /// User profile custom instructions and response style
    ProfileInstructions,
    /// Plan vs default execution mode instructions
    RunMode,
    /// Workspace path literal
    WorkspaceLocation,
    /// Active goal block (Ephemeral)
    ActiveGoal,
    /// Active implementation plan (Ephemeral)
    ActivePlan,
    /// Output contract for subagent surfaces
    SubagentOutputContract,
    /// User-provided custom subagent system prompt body
    CustomSubagentBody,
    /// Compaction instructions for summary generation
    CompactionContract,
    /// Title generation instructions
    TitleContract,
    /// Third-party extension point (static string key)
    Extension(&'static str),
}
