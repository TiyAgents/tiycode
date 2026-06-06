use std::borrow::Cow;

use super::layer::{LayerResolver, PromptLayer, SectionAnchor, SectionOrder};
use super::section_id::SectionId;
use super::section_source::{SectionCriticality, SectionSpec};
use super::sources::{
    ActiveGoalSource, ActivePlanSource, BehavioralGuidelinesSource, CompactionContractSource,
    FinalResponseStructureSource, ProfileInstructionsSource, ProjectContextSource, RoleSource,
    RunModeSource, SandboxPermissionsSource, ShellToolingGuideSource, SkillsSource,
    SubagentBodySource, SubagentOutputContractSource, SystemEnvironmentSource, TitleContractSource,
    WorkspaceLocationSource,
};
use super::surface::{PromptSurface, SurfaceMatcher, SurfacePattern};

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
    let mut registry = SectionRegistry::new(3);

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
        version: 2,
        max_chars: None,
        criticality: SectionCriticality::Critical,
        source: Box::new(RoleSource::new(2)),
    });

    registry.register(SectionSpec {
        id: SectionId::BehavioralGuidelines,
        title: Cow::Borrowed("Behavioral Guidelines"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::Role)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 2,
        // Behavioral guidelines is the largest static section (~7.5 KB).
        // Cap at 20 KB to leave headroom for future additions while still
        // bounding worst-case growth.
        max_chars: Some(20_000),
        criticality: SectionCriticality::Critical,
        source: Box::new(BehavioralGuidelinesSource::new(2)),
    });

    registry.register(SectionSpec {
        id: SectionId::FinalResponseStructure,
        title: Cow::Borrowed("Final Response Structure"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::BehavioralGuidelines)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 2,
        max_chars: None,
        criticality: SectionCriticality::Critical,
        source: Box::new(FinalResponseStructureSource::new(2)),
    });

    // ── SessionStable (was Capability + WorkspacePreference) ─────────
    // NOTE: Stage 5 migration is complete. All sections now use direct,
    // template-backed or self-contained sources. No Legacy adapters remain.
    // See docs/prompt-injection-refactor.md § 4.
    registry.register(SectionSpec {
        id: SectionId::ShellToolingGuide,
        title: Cow::Borrowed("Shell Tooling Guide"),
        layer: LayerResolver::Fixed(PromptLayer::SessionStable),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![
            SurfacePattern::AnyMainAgent,
            SurfacePattern::AnySubagent,
        ]),
        version: 2,
        max_chars: None,
        criticality: SectionCriticality::Critical,
        source: Box::new(ShellToolingGuideSource::new(2)),
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
        criticality: SectionCriticality::NonCritical,
        source: Box::new(SkillsSource),
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
        criticality: SectionCriticality::NonCritical,
        source: Box::new(ProjectContextSource::new(1)),
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
        criticality: SectionCriticality::Critical,
        source: Box::new(ProfileInstructionsSource),
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
        criticality: SectionCriticality::Critical,
        source: Box::new(SystemEnvironmentSource::new(1)),
    });

    registry.register(SectionSpec {
        id: SectionId::SandboxPermissions,
        title: Cow::Borrowed("Sandbox & Permissions"),
        layer: LayerResolver::Fixed(PromptLayer::RuntimeOverlay),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::SystemEnvironment)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        max_chars: None,
        criticality: SectionCriticality::Critical,
        source: Box::new(SandboxPermissionsSource::new(1)),
    });

    registry.register(SectionSpec {
        id: SectionId::RunMode,
        title: Cow::Borrowed("Run Mode"),
        layer: LayerResolver::Fixed(PromptLayer::RuntimeOverlay),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::SandboxPermissions)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 2,
        max_chars: None,
        criticality: SectionCriticality::Critical,
        source: Box::new(RunModeSource::new(2)),
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
        criticality: SectionCriticality::Critical,
        source: Box::new(WorkspaceLocationSource::new(1)),
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
        criticality: SectionCriticality::Critical,
        source: Box::new(SubagentOutputContractSource::new(1)),
    });

    registry.register(SectionSpec {
        id: SectionId::CustomSubagentBody,
        title: Cow::Borrowed("Subagent Body"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::SubagentOutputContract)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnySubagent]),
        version: 1,
        // Custom subagent prompts can be arbitrarily long; 50 KB leaves
        // generous headroom while still bounding worst-case system prompt size.
        max_chars: Some(50_000),
        criticality: SectionCriticality::Critical,
        source: Box::new(SubagentBodySource),
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
        criticality: SectionCriticality::NonCritical,
        source: Box::new(ActiveGoalSource),
    });

    registry.register(SectionSpec {
        id: SectionId::ActivePlan,
        title: Cow::Borrowed("Active Plan"),
        layer: LayerResolver::Fixed(PromptLayer::Ephemeral),
        order_hint: SectionOrder::Anchored(SectionAnchor::After(SectionId::ActiveGoal)),
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyMainAgent]),
        version: 1,
        max_chars: None,
        criticality: SectionCriticality::NonCritical,
        source: Box::new(ActivePlanSource),
    });

    // ── Compaction + Title sections ──────────────────────────────────
    registry.register(SectionSpec {
        id: SectionId::CompactionContract,
        title: Cow::Borrowed("Compaction Contract"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::AnyCompaction]),
        version: 2,
        max_chars: None,
        criticality: SectionCriticality::NonCritical,
        source: Box::new(CompactionContractSource::new(2)),
    });

    registry.register(SectionSpec {
        id: SectionId::TitleContract,
        title: Cow::Borrowed("Title Contract"),
        layer: LayerResolver::Fixed(PromptLayer::StablePrefix),
        order_hint: SectionOrder::First,
        surfaces: SurfaceMatcher::Any(vec![SurfacePattern::Title]),
        version: 1,
        max_chars: None,
        criticality: SectionCriticality::NonCritical,
        source: Box::new(TitleContractSource::new(1)),
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
    fn registry_has_all_17_sections() {
        let reg = default_registry();
        assert_eq!(reg.sections.len(), 17);
        assert_eq!(reg.schema_version(), 3);
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
            criticality: SectionCriticality::NonCritical,
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
    fn all_surfaces_have_sections() {
        // Verify every PromptSurface variant has a non-empty section list
        // in the default registry. This acts as a snapshot guard: adding a
        // new surface without declaring any sections will fail here.

        let reg = default_registry();
        let surfaces: Vec<PromptSurface> = vec![
            PromptSurface::MainAgent {
                run_mode: super::super::run_mode::RunMode::Default,
            },
            PromptSurface::MainAgent {
                run_mode: super::super::run_mode::RunMode::Plan,
            },
            PromptSurface::SubagentExplore {
                inherited_run_mode: super::super::run_mode::RunMode::Default,
            },
            PromptSurface::SubagentReview {
                inherited_run_mode: super::super::run_mode::RunMode::Default,
            },
            PromptSurface::SubagentCustom {
                slug: "test-slug".to_string(),
                inherited_run_mode: super::super::run_mode::RunMode::Default,
                cache_stability: super::super::surface::SubagentCacheStability::Volatile,
            },
            PromptSurface::Compaction {
                kind: super::super::surface::CompactionKind::Compact,
            },
            PromptSurface::Compaction {
                kind: super::super::surface::CompactionKind::Merge,
            },
            PromptSurface::Title,
        ];

        for surface in &surfaces {
            let sections = reg.filter_for_surface(surface);
            assert!(
                !sections.is_empty(),
                "surface {:?} should have at least one section",
                surface
            );
        }
    }

    #[test]
    fn main_agent_sections_are_deterministic() {
        let reg = default_registry();
        let sections = reg.filter_for_surface(&PromptSurface::MainAgent {
            run_mode: super::super::run_mode::RunMode::Default,
        });

        // Snapshot: main agent surface should include these sections
        let ids: Vec<SectionId> = sections.iter().map(|s| s.id.clone()).collect();

        // Core sections must always be present
        assert!(ids.contains(&SectionId::Role), "MainAgent must have Role");
        assert!(
            ids.contains(&SectionId::BehavioralGuidelines),
            "MainAgent must have BehavioralGuidelines"
        );
        assert!(
            ids.contains(&SectionId::FinalResponseStructure),
            "MainAgent must have FinalResponseStructure"
        );
        assert!(
            ids.contains(&SectionId::ShellToolingGuide),
            "MainAgent must have ShellToolingGuide"
        );

        // Dynamic sections
        assert!(ids.contains(&SectionId::ProjectContext));
        assert!(ids.contains(&SectionId::ProfileInstructions));
        assert!(ids.contains(&SectionId::SystemEnvironment));
        assert!(ids.contains(&SectionId::WorkspaceLocation));
        assert!(ids.contains(&SectionId::ActiveGoal));

        // Subagent-specific sections should NOT be in MainAgent
        assert!(
            !ids.contains(&SectionId::SubagentOutputContract),
            "SubagentOutputContract must not appear on MainAgent"
        );
        assert!(
            !ids.contains(&SectionId::CustomSubagentBody),
            "CustomSubagentBody must not appear on MainAgent"
        );
    }

    #[test]
    fn subagent_sections_include_body_and_output_contract() {
        let reg = default_registry();

        for surface in &[
            PromptSurface::SubagentExplore {
                inherited_run_mode: super::super::run_mode::RunMode::Default,
            },
            PromptSurface::SubagentReview {
                inherited_run_mode: super::super::run_mode::RunMode::Default,
            },
            PromptSurface::SubagentCustom {
                slug: "test-slug".to_string(),
                inherited_run_mode: super::super::run_mode::RunMode::Default,
                cache_stability: super::super::surface::SubagentCacheStability::Volatile,
            },
        ] {
            let ids: Vec<SectionId> = reg
                .filter_for_surface(surface)
                .iter()
                .map(|s| s.id.clone())
                .collect();

            assert!(
                ids.contains(&SectionId::SubagentOutputContract),
                "{:?} must have SubagentOutputContract",
                surface
            );
            assert!(
                ids.contains(&SectionId::CustomSubagentBody),
                "{:?} must have CustomSubagentBody",
                surface
            );
            assert!(
                ids.contains(&SectionId::Role),
                "{:?} must have Role for identity",
                surface
            );
        }
    }

    #[test]
    fn schema_version_monotonic() {
        // L1 hard-floor: schema_version must never go below the recorded baseline.
        // Bump BASELINE_SCHEMA_VERSION every time you bump default_registry().schema_version
        // per the rules in docs/prompt-injection-refactor.md § 3.19.
        const BASELINE_SCHEMA_VERSION: u32 = 3;

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
