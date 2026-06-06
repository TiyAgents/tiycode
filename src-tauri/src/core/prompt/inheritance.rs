use super::section_id::SectionId;
use super::surface::PromptSurface;

/// Kind of subagent surface for inheritance lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubagentSurfaceKind {
    Explore,
    Review,
    Custom,
}

impl SubagentSurfaceKind {
    pub fn from_surface(surface: &PromptSurface) -> Option<Self> {
        match surface {
            PromptSurface::SubagentExplore { .. } => Some(SubagentSurfaceKind::Explore),
            PromptSurface::SubagentReview { .. } => Some(SubagentSurfaceKind::Review),
            PromptSurface::SubagentCustom { .. } => Some(SubagentSurfaceKind::Custom),
            _ => None,
        }
    }
}

/// Single source of truth: which Section IDs must appear on each subagent surface.
/// When adding/removing sections or adjusting SurfaceMatcher, sync this list.
pub const SUBAGENT_INHERITED_SECTIONS: &[(SubagentSurfaceKind, &[SectionId])] = &[
    (
        SubagentSurfaceKind::Explore,
        &[
            SectionId::Role,
            SectionId::SystemEnvironment,
            SectionId::ProjectContext,
            SectionId::ProfileInstructions,
            SectionId::WorkspaceLocation,
            SectionId::ShellToolingGuide,
            SectionId::SubagentOutputContract,
        ],
    ),
    (
        SubagentSurfaceKind::Review,
        &[
            SectionId::Role,
            SectionId::SystemEnvironment,
            SectionId::ProjectContext,
            SectionId::ProfileInstructions,
            SectionId::WorkspaceLocation,
            SectionId::ShellToolingGuide,
            SectionId::SubagentOutputContract,
        ],
    ),
    (
        SubagentSurfaceKind::Custom,
        &[
            SectionId::Role,
            SectionId::SystemEnvironment,
            SectionId::ProjectContext,
            SectionId::ProfileInstructions,
            SectionId::WorkspaceLocation,
            SectionId::CustomSubagentBody,
            SectionId::SubagentOutputContract,
        ],
    ),
];

/// Sections that must NOT appear on subagent surfaces.
pub const SUBAGENT_FORBIDDEN_SECTIONS: &[SectionId] = &[
    SectionId::BehavioralGuidelines,
    SectionId::FinalResponseStructure,
];

#[cfg(test)]
mod tests {
    use super::super::registry::default_registry;
    use super::super::run_mode::RunMode;
    use super::super::surface::{PromptSurface, SubagentCacheStability};
    use super::*;
    use std::collections::HashSet;

    fn surface_for(kind: SubagentSurfaceKind) -> PromptSurface {
        match kind {
            SubagentSurfaceKind::Explore => PromptSurface::SubagentExplore {
                inherited_run_mode: RunMode::Default,
            },
            SubagentSurfaceKind::Review => PromptSurface::SubagentReview {
                inherited_run_mode: RunMode::Default,
            },
            SubagentSurfaceKind::Custom => PromptSurface::SubagentCustom {
                slug: "lint".into(),
                inherited_run_mode: RunMode::Default,
                cache_stability: SubagentCacheStability::Volatile,
            },
        }
    }

    #[test]
    fn subagent_inheritance_complete() {
        let covered: HashSet<_> = SUBAGENT_INHERITED_SECTIONS
            .iter()
            .map(|(kind, _)| *kind)
            .collect();
        assert!(covered.contains(&SubagentSurfaceKind::Explore));
        assert!(covered.contains(&SubagentSurfaceKind::Review));
        assert!(covered.contains(&SubagentSurfaceKind::Custom));

        let forbidden: HashSet<_> = SUBAGENT_FORBIDDEN_SECTIONS.iter().cloned().collect();
        for (_kind, sections) in SUBAGENT_INHERITED_SECTIONS {
            assert!(
                !sections.is_empty(),
                "SUBAGENT_INHERITED_SECTIONS entry for {:?} is empty",
                _kind
            );
            for section in *sections {
                assert!(
                    !forbidden.contains(section),
                    "Forbidden section {:?} found in SUBAGENT_INHERITED_SECTIONS for {:?}",
                    section,
                    _kind
                );
            }
        }
    }

    /// Lint per § 3.22 step 2-3: declared inheritance ⊆ registry filter result.
    #[test]
    fn subagent_inheritance_matches_registry() {
        let registry = default_registry();
        for (kind, declared) in SUBAGENT_INHERITED_SECTIONS {
            let surface = surface_for(*kind);
            let actual: HashSet<SectionId> = registry
                .filter_for_surface(&surface)
                .into_iter()
                .map(|spec| spec.id.clone())
                .collect();

            for required in *declared {
                assert!(
                    actual.contains(required),
                    "Subagent {:?} is missing required section {:?} (declared in SUBAGENT_INHERITED_SECTIONS but not registered for surface)",
                    kind,
                    required
                );
            }

            // § 3.22 step 4: forbidden sections must NOT appear in subagent surface
            for forbidden in SUBAGENT_FORBIDDEN_SECTIONS {
                assert!(
                    !actual.contains(forbidden),
                    "Subagent {:?} contains forbidden section {:?} (must be main-agent only)",
                    kind,
                    forbidden
                );
            }
        }
    }
}
