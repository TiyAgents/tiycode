// ── Individual SectionSource implementations ──────────────────────
// Each file contains exactly one SectionSource implementation.

pub mod active_goal;
pub mod active_plan;
pub mod behavioral_guidelines;
pub mod compaction_contract;
pub mod custom_subagent_body;
pub mod final_response_structure;
pub mod profile_instructions;
pub mod project_context;
pub mod role;
pub mod run_mode;
pub mod sandbox_permissions;
pub mod shell_tooling_guide;
pub mod skills;
pub mod source_tests;
pub mod subagent_output_contract;
pub mod system_environment;
pub mod title_contract;
pub mod workspace_location;

// Re-export all public types
pub use active_goal::ActiveGoalSource;
pub use active_plan::ActivePlanSource;
pub use behavioral_guidelines::BehavioralGuidelinesSource;
pub use compaction_contract::CompactionContractSource;
pub use custom_subagent_body::SubagentBodySource;
pub use final_response_structure::FinalResponseStructureSource;
pub use profile_instructions::ProfileInstructionsSource;
pub use project_context::ProjectContextSource;
pub use role::RoleSource;
pub use run_mode::RunModeSource;
pub use sandbox_permissions::SandboxPermissionsSource;
pub use shell_tooling_guide::ShellToolingGuideSource;
pub use skills::SkillsSource;
pub use subagent_output_contract::SubagentOutputContractSource;
pub use system_environment::SystemEnvironmentSource;
pub use title_contract::TitleContractSource;
pub use workspace_location::WorkspaceLocationSource;
