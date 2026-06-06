use std::borrow::Cow;

use async_trait::async_trait;

use super::build_context::BuildCx;
use super::layer::{LayerResolver, SectionWarning};
use super::section_id::SectionId;
use super::signals::BuildSignal;
use super::surface::{PromptSurface, SurfaceMatcher};

/// A fatal error that causes the entire prompt build to fail.
/// Rare; reserved for truly unrecoverable errors (template load failure, SQLite fatal disconnect).
#[derive(Debug)]
pub struct FatalError {
    pub message: String,
    pub code: &'static str,
}

impl FatalError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FatalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for FatalError {}

/// The result of building a single section.
/// Four-state enum replacing the confusing Result<Option<...>, SoftError> pattern.
pub enum SectionOutcome {
    /// Not applicable for this build (e.g., ActiveGoal when no thread)
    Skip,
    /// Normal successful output
    Produced(SectionBody),
    /// Partially degraded but still usable (e.g., Skills partially loaded)
    Degraded {
        body: SectionBody,
        warning: SectionWarning,
    },
    /// Skipped with a warning (e.g., ProjectContext IO failure)
    SoftFailed {
        code: &'static str,
        error: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Rendered body of a section.
#[derive(Debug, Clone)]
pub struct SectionBody {
    /// Rendered Markdown body (excluding H2 title; Renderer wraps it)
    pub markdown: String,
    /// Optional metadata
    pub meta: SectionMeta,
}

impl SectionBody {
    pub fn markdown(body: impl Into<String>) -> Self {
        Self {
            markdown: body.into(),
            meta: SectionMeta::default(),
        }
    }

    pub fn with_meta(body: impl Into<String>, meta: SectionMeta) -> Self {
        Self {
            markdown: body.into(),
            meta,
        }
    }
}

/// Metadata for a section body.
#[derive(Debug, Clone, Default)]
pub struct SectionMeta {
    /// Estimated token count
    pub estimated_tokens: Option<usize>,
    /// Source template file path (for debugging)
    pub template_path: Option<&'static str>,
}

/// Specification for a registered section.
pub struct SectionSpec {
    /// Unique identifier
    pub id: SectionId,
    /// Display title (used in rendered heading; v1 no runtime i18n)
    pub title: Cow<'static, str>,
    /// Which layer this section belongs to (static or per-surface)
    pub layer: LayerResolver,
    /// Ordering hint within the layer
    pub order_hint: super::layer::SectionOrder,
    /// Which surfaces this section appears in
    pub surfaces: SurfaceMatcher,
    /// Content/structural version; bump when template or logic changes
    pub version: u32,
    /// Per-section character limit; None uses budget's per_section_default_chars
    pub max_chars: Option<usize>,
    /// The source that produces this section's body
    pub source: Box<dyn SectionSource>,
    /// Whether failure of this section should escalate to overall build failure.
    /// Default: Critical — only override to NonCritical for optional sections.
    pub criticality: SectionCriticality,
}

/// Criticality level for a section. Controls whether SoftFailed escalates to FatalError.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionCriticality {
    /// Failure of this section causes the entire prompt build to fail
    Critical,
    /// Failure is tolerated; the build continues without this section
    NonCritical,
}

/// The core trait for producing a section's body.
/// Replaces the old PromptSectionProvider.
#[async_trait]
pub trait SectionSource: Send + Sync {
    /// Whether this source is enabled for the given surface and context.
    /// Default: checks SectionSpec.surfaces.
    fn enabled_for(&self, _surface: &PromptSurface, _cx: &BuildCx<'_>) -> bool {
        true // Default: always enabled; overridden by registry-level filtering
    }

    /// Which signals this source depends on. Composer uses this for concurrency scheduling.
    fn required_signals(&self) -> &'static [BuildSignal] {
        &[]
    }

    /// A short, stable name describing the source kind (e.g. "template:role.md").
    /// Written into SectionAudit.source_kind; defaults to the type name.
    fn source_kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Build the section body. Catastrophic errors go in Result::Err;
    /// all other states use SectionOutcome variants.
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError>;
}
