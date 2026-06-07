use super::run_mode::RunMode;

/// Prompts are built for one of these surfaces.
/// Each surface determines which sections are included and how they are rendered.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PromptSurface {
    /// Main agent system prompt
    MainAgent { run_mode: RunMode },
    /// Built-in explore subagent
    SubagentExplore { inherited_run_mode: RunMode },
    /// Built-in review subagent
    SubagentReview { inherited_run_mode: RunMode },
    /// Built-in goal acceptance Judge subagent
    SubagentJudge { inherited_run_mode: RunMode },
    /// User-defined custom subagent
    SubagentCustom {
        slug: String,
        inherited_run_mode: RunMode,
        /// Whether the user has declared the custom prompt to be cache-stable
        cache_stability: SubagentCacheStability,
    },
    /// Context compaction for long-running threads
    Compaction { kind: CompactionKind },
    /// Session title generation
    Title,
}

/// Compaction variants: incremental compact vs merge-of-summaries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompactionKind {
    Compact,
    Merge,
}

/// Cache stability declaration for custom subagent prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubagentCacheStability {
    /// Default; user prompt may contain transient content
    Volatile,
    /// User explicitly declares the prompt is cross-session stable
    Stable,
}

/// Pattern for matching surfaces when declaring section applicability.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SurfacePattern {
    /// Matches any MainAgent surface regardless of run_mode
    AnyMainAgent,
    /// Matches a specific MainAgent run_mode
    MainAgent(RunMode),
    /// Matches any subagent surface (explore, review, judge, custom)
    AnySubagent,
    /// Matches built-in explore + review + judge subagents only
    BuiltinSubagent,
    /// Matches any custom subagent regardless of slug
    CustomSubagent,
    /// Matches a specific compaction kind
    Compaction(CompactionKind),
    /// Matches any compaction surface
    AnyCompaction,
    /// Matches Title surface
    Title,
}

impl SurfacePattern {
    /// Check whether this pattern matches a given surface.
    pub fn matches(&self, surface: &PromptSurface) -> bool {
        match (self, surface) {
            (SurfacePattern::AnyMainAgent, PromptSurface::MainAgent { .. }) => true,
            (SurfacePattern::MainAgent(rm), PromptSurface::MainAgent { run_mode }) => {
                rm == run_mode
            }
            (SurfacePattern::AnySubagent, PromptSurface::SubagentExplore { .. }) => true,
            (SurfacePattern::AnySubagent, PromptSurface::SubagentReview { .. }) => true,
            (SurfacePattern::AnySubagent, PromptSurface::SubagentJudge { .. }) => true,
            (SurfacePattern::AnySubagent, PromptSurface::SubagentCustom { .. }) => true,
            (SurfacePattern::BuiltinSubagent, PromptSurface::SubagentExplore { .. }) => true,
            (SurfacePattern::BuiltinSubagent, PromptSurface::SubagentReview { .. }) => true,
            (SurfacePattern::BuiltinSubagent, PromptSurface::SubagentJudge { .. }) => true,
            (SurfacePattern::CustomSubagent, PromptSurface::SubagentCustom { .. }) => true,
            (SurfacePattern::Compaction(k), PromptSurface::Compaction { kind }) => k == kind,
            (SurfacePattern::AnyCompaction, PromptSurface::Compaction { .. }) => true,
            (SurfacePattern::Title, PromptSurface::Title) => true,
            _ => false,
        }
    }
}

/// Declares which surfaces a section applies to.
#[derive(Debug, Clone)]
pub enum SurfaceMatcher {
    /// Applies to all surfaces
    All,
    /// Applies to any of the listed patterns
    Any(Vec<SurfacePattern>),
    /// Applies to all surfaces except the listed patterns
    Excluding(Vec<SurfacePattern>),
    /// Custom predicate (rare; prefer the above)
    Predicate(fn(&PromptSurface) -> bool),
}

impl SurfaceMatcher {
    pub fn matches(&self, surface: &PromptSurface) -> bool {
        match self {
            SurfaceMatcher::All => true,
            SurfaceMatcher::Any(patterns) => patterns.iter().any(|p| p.matches(surface)),
            SurfaceMatcher::Excluding(patterns) => !patterns.iter().any(|p| p.matches(surface)),
            SurfaceMatcher::Predicate(f) => f(surface),
        }
    }
}
