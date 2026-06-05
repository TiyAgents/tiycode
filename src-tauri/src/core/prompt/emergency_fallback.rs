use super::section_id::SectionId;
use super::surface::PromptSurface;

/// Emergency fallback text for each surface, compiled inline via include_str!.
/// These are used when ALL sections fail / skip / soft-fail.
/// Must be ≤ 1 KB each, contain NO placeholders, and have zero runtime dependencies.

/// Per-surface fallback: returns the embedded static text.
pub fn emergency_fallback_text(surface: &PromptSurface) -> &'static str {
    match surface {
        PromptSurface::MainAgent { .. } => {
            include_str!("templates/emergency_fallback/main_agent.md")
        }
        PromptSurface::SubagentExplore { .. } => {
            include_str!("templates/emergency_fallback/subagent_explore.md")
        }
        PromptSurface::SubagentReview { .. } => {
            include_str!("templates/emergency_fallback/subagent_review.md")
        }
        PromptSurface::SubagentCustom { .. } => {
            include_str!("templates/emergency_fallback/subagent_custom.md")
        }
        PromptSurface::Compaction { .. } => {
            include_str!("templates/emergency_fallback/compaction.md")
        }
        PromptSurface::Title => {
            include_str!("templates/emergency_fallback/title.md")
        }
    }
}

/// Critical sections that, if soft-failed, escalate to FatalError.
/// These are the minimum set of sections needed for each surface to function.
pub fn critical_sections(surface: &PromptSurface) -> &'static [SectionId] {
    match surface {
        PromptSurface::MainAgent { .. } => &[
            SectionId::Role,
            SectionId::BehavioralGuidelines,
            SectionId::FinalResponseStructure,
        ],
        PromptSurface::SubagentExplore { .. } | PromptSurface::SubagentReview { .. } => {
            &[SectionId::Role, SectionId::SubagentOutputContract]
        }
        PromptSurface::SubagentCustom { .. } => &[
            SectionId::Role,
            SectionId::SubagentBody,
            SectionId::SubagentOutputContract,
        ],
        PromptSurface::Compaction { .. } => &[SectionId::Role, SectionId::CompactionContract],
        PromptSurface::Title => &[SectionId::Role, SectionId::TitleContract],
    }
}

#[cfg(test)]
mod tests {
    use super::super::run_mode::RunMode;
    use super::super::surface::{CompactionKind, SubagentCacheStability};
    use super::*;

    #[test]
    fn emergency_fallback_purity() {
        let surfaces = vec![
            PromptSurface::MainAgent {
                run_mode: RunMode::Default,
            },
            PromptSurface::SubagentExplore {
                inherited_run_mode: RunMode::Default,
            },
            PromptSurface::SubagentReview {
                inherited_run_mode: RunMode::Default,
            },
            PromptSurface::SubagentCustom {
                slug: "test".into(),
                inherited_run_mode: RunMode::Default,
                cache_stability: SubagentCacheStability::Volatile,
            },
            PromptSurface::Compaction {
                kind: CompactionKind::Compact,
            },
            PromptSurface::Title,
        ];

        for surface in &surfaces {
            let text = emergency_fallback_text(surface);
            assert!(
                !text.trim().is_empty(),
                "emergency_fallback_text returned empty for {:?}",
                surface
            );
        }
    }
}
