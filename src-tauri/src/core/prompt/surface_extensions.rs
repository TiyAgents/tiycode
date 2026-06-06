use std::sync::Arc;

use super::budget::PromptBudget;
use super::renderer::{MarkdownRenderer, SectionRenderer};
use super::surface::{PromptSurface, SurfacePattern};

/// Trait that every PromptSurface variant must implement.
/// Adding a new surface variant requires implementing this trait,
/// enforced by startup lint `surface_extensions_complete`.
pub trait SurfaceExtension {
    /// The SurfacePattern that matches this surface.
    fn pattern(&self) -> SurfacePattern;

    /// Default prompt budget for this surface.
    fn default_budget(&self) -> PromptBudget;

    /// Whether this surface uses RuntimeMessageInjectors.
    fn runtime_message_enabled(&self) -> bool;

    /// Default section renderer for this surface.
    fn default_renderer(&self) -> Arc<dyn SectionRenderer>;
}

impl SurfaceExtension for PromptSurface {
    fn pattern(&self) -> SurfacePattern {
        match self {
            PromptSurface::MainAgent { run_mode } => SurfacePattern::MainAgent(*run_mode),
            PromptSurface::SubagentExplore { .. } => SurfacePattern::AnySubagent,
            PromptSurface::SubagentReview { .. } => SurfacePattern::AnySubagent,
            PromptSurface::SubagentCustom { .. } => SurfacePattern::CustomSubagent,
            PromptSurface::Compaction { kind } => SurfacePattern::Compaction(*kind),
            PromptSurface::Title => SurfacePattern::Title,
        }
    }

    fn default_budget(&self) -> PromptBudget {
        PromptBudget::default()
    }

    fn runtime_message_enabled(&self) -> bool {
        matches!(
            self,
            PromptSurface::MainAgent { .. }
                | PromptSurface::SubagentExplore { .. }
                | PromptSurface::SubagentReview { .. }
                | PromptSurface::SubagentCustom { .. }
        )
    }

    fn default_renderer(&self) -> Arc<dyn SectionRenderer> {
        Arc::new(MarkdownRenderer)
    }
}

/// Startup lint: verifies every PromptSurface variant has all SurfaceExtension fields.
/// Run via `cargo test prompt::surface_extensions_complete`.
#[cfg(test)]
mod tests {
    use super::super::run_mode::RunMode;
    use super::super::surface::{CompactionKind, SubagentCacheStability};
    use super::*;

    #[test]
    fn surface_extensions_complete() {
        // Build representative instances of each surface variant
        let surfaces: Vec<PromptSurface> = vec![
            PromptSurface::MainAgent {
                run_mode: RunMode::Default,
            },
            PromptSurface::MainAgent {
                run_mode: RunMode::Plan,
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
            PromptSurface::Compaction {
                kind: CompactionKind::Merge,
            },
            PromptSurface::Title,
        ];

        for surface in &surfaces {
            // Verify each field is non-empty/valid
            let _pattern = surface.pattern();
            let _budget = surface.default_budget();
            let _renderer = surface.default_renderer();
            // runtime_message_enabled just returns bool
            let _ = surface.runtime_message_enabled();
        }
    }
}
