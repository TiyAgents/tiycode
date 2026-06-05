use std::borrow::Cow;

use super::active_goal_source::ActiveGoalSource;
use super::layer::{LayerResolver, PromptLayer, SectionAnchor, SectionOrder};
use super::legacy_adapter::{
    LegacyCompactionContractSource,
    LegacyCustomSubagentBodySource,
    LegacyProfileInstructionsSource,
    LegacySkillsSource, LegacySubagentOutputContractSource,
    LegacyTitleContractSource,
};
use super::providers::{ProfileProvider, SkillsProvider};
use super::section_id::SectionId;
use super::section_source::SectionSpec;
use super::surface::{PromptSurface, SurfaceMatcher, SurfacePattern};
use super::template_sources::{
    ProjectContextSource, RunModeSource, SandboxPermissionsSource,
    SystemEnvironmentSource, WorkspaceLocationSource,
};
use super::templates::{TemplateSource, TemplateVars};

/// PerSurface layer resolver for ProfileInstructions:
/// MainAgent / Subagent → SessionStable
/// Compaction / Title → StablePrefix (no thread state, fully stable)
fn profile_instructions_layer(surface: &PromptSurface) -> PromptLayer {
    match surface {
        PromptSurface::Compaction { .. } | PromptSurface::Title => PromptLayer::StablePrefix,
        _ => PromptLayer::SessionStable,
    }
}

/// Registry of all prompt sections.
/// Sections are registered once at startup and never change.
pub struct SectionRegistry {
    sections: Vec<SectionSpec>,
    /// Monotonic schema version; bump according to § 3.19 rules.
    schema_version: u32,
}

impl SectionRegistry {
    pub fn new(schema_version: u32) -> Self {
        Self {
            sections: Vec::new(),
            schema_version,
        }
    }

    /// Register a section spec.
    pub fn register(&mut self, spec: SectionSpec) {
        self.sections.push(spec);
    }

    /// Iterate over all registered sections.
    pub fn iter(&self) -> impl Iterator<Item = &SectionSpec> {
        self.sections.iter()
    }

    /// Get the current schema version.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Find all sections matching a given surface.
    pub fn filter_for_surface<'a>(
        &'a self,
        surface: &'a super::surface::PromptSurface,
    ) -> Vec<&'a SectionSpec> {
        self.sections
            .iter()
            .filter(|spec| spec.surfaces.matches(surface))
            .collect()
    }
}

/// Build the default section registry with all 11 built-in legacy sections.
/// Byte-equal layer mapping: Core→StablePrefix, Capability+WorkspacePreference→SessionStable,
/// RuntimeContext→RuntimeOverlay. This preserves the old (phase, order_in_phase) ordering.
pub fn default_registry() -> SectionRegistry {
    let mut registry = SectionRegistry::new(1);

    // ── StablePrefix (was Core) ──────────────────────────────────────
    registry.register(SectionSpec {
        id: SectionId::Role,
        title: Cow::Borrowed("Role"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![
            SurfacePattern::AnyMainAgent,
            SurfacePattern::AnySubagent,
        ]),
        version: 1,
        max_chars: None,
        source: Box::new(TemplateSource::new(
            "role.md",
            include_str!("templates/role.md"),
            &[],
            |_cx| Ok(TemplateVars::new()),
        )),
    });

    registry.register(SectionSpec {
        id: SectionId::BehavioralGuidelines,
        title: Cow::Borrowed("Behavioral Guidelines"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::Role)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        // Behavioral guidelines is the largest static section (~7.5 KB).
        // Cap at 20 KB to leave headroom for future additions while still
        // bounding worst-case growth.
        max_chars: Some(20_000),
        source: Box::new(TemplateSource::new(
            "behavioral_guidelines.md",
            include_str!("templates/behavioral_guidelines.md"),
            &[],
            |_cx| Ok(TemplateVars::new()),
        )),
    });

    registry.register(SectionSpec {
        id: SectionId::FinalResponseStructure,
        title: Cow::Borrowed("Final Response Structure"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::BehavioralGuidelines)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        max_chars: None,
        source: Box::new(TemplateSource::new(
            "final_response_structure.md",
            include_str!("templates/final_response_structure.md"),
            &[],
            |_cx| Ok(TemplateVars::new()),
        )),
    });

    // ── SessionStable (was Capability + WorkspacePreference) ─────────
    // NOTE (Stage 5 follow-up, see docs/prompt-injection-refactor.md § 4):
    //   Skills, ProjectContext, SystemEnvironment, SandboxPermissions, RunMode,
    //   ProfileInstructions, WorkspaceLocation still use Legacy*Source adapters.
    //   The .md templates exist (skills_usage.md, sandbox_permissions.tpl.md,
    //   run_mode.{plan,default}.md, etc.) but are NOT byte-equal to legacy output —
    //   migrating each requires careful template-vs-legacy diff and explicit
    //   approval. Tracking issue: byte-equal alignment per § 4 阶段 1 + 5.
    registry.register(SectionSpec {
        id: SectionId::ShellToolingGuide,
        title: Cow::Borrowed("Shell Tooling Guide"),
        layer: LayerResolver::Fixed(PromptLayer::SessionStable),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![
            SurfacePattern::AnyMainAgent,
            SurfacePattern::AnySubagent,
        ]),
        version: 1,
        max_chars: None,
        source: Box::new(TemplateSource::new(
            "shell_tooling_guide.md",
            include_str!("templates/shell_tooling_guide.md"),
            &["shell"],
            |_cx| {
                let shell = crate::core::shell_runtime::current_shell();
                Ok(TemplateVars::new().insert("shell", shell))
            },
        )),
    });

    registry.register(SectionSpec {
        id: SectionId::Skills,
        title: Cow::Borrowed("Skills"),
        layer: LayerResolver::Fixed(PromptLayer::SessionStable),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::ShellToolingGuide)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        // Skills body is dynamic and can be large for users with many installed
        // skills (~200 chars per skill × N skills + ~2.5 KB usage guide).
        // Without an explicit cap the per_section_default_chars (6 KB) would
        // truncate the trailing "How to use skills" guidance.
        max_chars: Some(40_000),
        source: Box::new(LegacySkillsSource(SkillsProvider)),
    });

    registry.register(SectionSpec {
        id: SectionId::ProjectContext,
        title: Cow::Borrowed("Project Context (workspace instructions)"),
        layer: LayerResolver::Fixed(PromptLayer::SessionStable),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::Skills)),
        surfaces: SurfaceMatcher::Any(vec![
            SurfacePattern::AnyMainAgent,
            SurfacePattern::AnySubagent,
        ]),
        version: 1,
        max_chars: None,
        source: Box::new(ProjectContextSource),
    });

    registry.register(SectionSpec {
        id: SectionId::ProfileInstructions,
        title: Cow::Borrowed("Profile Instructions"),
        layer: LayerResolver::PerSurface(profile_instructions_layer),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::ProjectContext)),
        surfaces: SurfaceMatcher::Any(vec![
            SurfacePattern::AnyMainAgent,
            SurfacePattern::AnySubagent,
            SurfacePattern::AnyCompaction,
            SurfacePattern::Title,
        ]),
        version: 1,
        max_chars: None,
        source: Box::new(LegacyProfileInstructionsSource(ProfileProvider)),
    });

    // ── RuntimeOverlay (was RuntimeContext) ──────────────────────────
    registry.register(SectionSpec {
        id: SectionId::SystemEnvironment,
        title: Cow::Borrowed("System Environment"),
        layer: LayerResolver::Fixed(PromptLayer::RuntimeOverlay),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![
            SurfacePattern::AnyMainAgent,
            SurfacePattern::AnySubagent,
        ]),
        version: 1,
        max_chars: None,
        source: Box::new(SystemEnvironmentSource),
    });

    registry.register(SectionSpec {
        id: SectionId::SandboxPermissions,
        title: Cow::Borrowed("Sandbox & Permissions"),
        layer: LayerResolver::Fixed(PromptLayer::RuntimeOverlay),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::SystemEnvironment)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        max_chars: None,
        source: Box::new(SandboxPermissionsSource),
    });

    registry.register(SectionSpec {
        id: SectionId::RunMode,
        title: Cow::Borrowed("Run Mode"),
        layer: LayerResolver::Fixed(PromptLayer::RuntimeOverlay),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::SandboxPermissions)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        max_chars: None,
        source: Box::new(RunModeSource),
    });

    registry.register(SectionSpec {
        id: SectionId::WorkspaceLocation,
        title: Cow::Borrowed("Runtime Context"),
        layer: LayerResolver::Fixed(PromptLayer::RuntimeOverlay),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::RunMode)),
        surfaces: SurfaceMatcher::Any(vec![
            SurfacePattern::AnyMainAgent,
            SurfacePattern::AnySubagent,
        ]),
        version: 1,
        max_chars: None,
        source: Box::new(WorkspaceLocationSource),
    });

    // ── Subagent sections ────────────────────────────────────────────
    registry.register(SectionSpec {
        id: SectionId::SubagentOutputContract,
        title: Cow::Borrowed("Subagent Output Contract"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::FinalResponseStructure)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnySubagent]),
        version: 1,
        max_chars: None,
        source: Box::new(LegacySubagentOutputContractSource),
    });

    registry.register(SectionSpec {
        id: SectionId::CustomSubagentBody,
        title: Cow::Borrowed("Custom Subagent Body"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::SubagentOutputContract)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::CustomSubagent]),
        version: 1,
        max_chars: None,
        source: Box::new(LegacyCustomSubagentBodySource),
    });

    // ── Ephemeral ────────────────────────────────────────────────────
    registry.register(SectionSpec {
        id: SectionId::ActiveGoal,
        title: Cow::Borrowed("Active Goal"),
        layer: LayerResolver::Fixed(PromptLayer::Ephemeral),
        order_hint: SectionOrder::Default,
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        max_chars: None,
        source: Box::new(ActiveGoalSource),
    });

    // ── Compaction + Title sections ──────────────────────────────────
    registry.register(SectionSpec {
        id: SectionId::CompactionContract,
        title: Cow::Borrowed("Compaction Contract"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyCompaction]),
        version: 1,
        max_chars: None,
        source: Box::new(LegacyCompactionContractSource),
    });

    registry.register(SectionSpec {
        id: SectionId::TitleContract,
        title: Cow::Borrowed("Title Contract"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::Title]),
        version: 1,
        max_chars: None,
        source: Box::new(LegacyTitleContractSource),
    });

    registry
}

#[cfg(test)]
mod tests {
    use super::super::layer::LayerResolver;
    use super::super::section_id::SectionId;
    use super::super::section_source::SectionSpec;
    use super::super::surface::SurfaceMatcher;
    use super::*;

    #[test]
    fn registry_has_all_16_sections() {
        let reg = default_registry();
        assert_eq!(reg.sections.len(), 16);
        assert_eq!(reg.schema_version(), 1);
    }

    #[test]
    fn registry_register_and_iterate() {
        let mut reg = SectionRegistry::new(1);
        reg.register(SectionSpec {
            id: SectionId::ActiveGoal,
            title: Cow::Borrowed("Active Goal"),
            layer: LayerResolver::Fixed(super::super::layer::PromptLayer::Ephemeral),
            order_hint: super::super::layer::SectionOrder::Default,
            surfaces: SurfaceMatcher::All,
            version: 1,
            max_chars: None,
            source: Box::new(DummySource),
        });
        assert_eq!(reg.iter().count(), 1);
    }

    struct DummySource;
    #[async_trait::async_trait]
    impl super::super::section_source::SectionSource for DummySource {
        async fn build(
            &self,
            _cx: &super::super::build_context::BuildCx<'_>,
        ) -> Result<
            super::super::section_source::SectionOutcome,
            super::super::section_source::FatalError,
        > {
            Ok(super::super::section_source::SectionOutcome::Skip)
        }
    }

    #[test]
    fn schema_version_monotonic() {
        // L1 hard-floor: schema_version must never go below the recorded baseline.
        // Bump BASELINE_SCHEMA_VERSION every time you bump default_registry().schema_version
        // per the rules in docs/prompt-injection-refactor.md § 3.19.
        const BASELINE_SCHEMA_VERSION: u32 = 1;

        let reg = default_registry();
        assert!(
            reg.schema_version() >= BASELINE_SCHEMA_VERSION,
            "schema_version {} is below baseline {} — regression in registry version (see § 3.19)",
            reg.schema_version(),
            BASELINE_SCHEMA_VERSION
        );

        // L2 hint: every Section must declare a version ≥ 1
        for spec in reg.iter() {
            assert!(
                spec.version >= 1,
                "Section {:?} has invalid version {} (must be ≥ 1)",
                spec.id,
                spec.version
            );
        }
    }
}
