// ── Section sources (one per SectionId, each in sources/) ────────
pub mod sources;

// ── Backward-compat test utilities (not used in production) ─────
pub mod providers;

// ── Core architecture modules ───────────────────────────────────
pub mod budget;
pub mod build_context;
pub mod cache_marker;
pub mod clock;
pub mod composer;
pub mod error_codes;
pub mod exec_policy;
pub mod inheritance;
pub mod layer;
pub mod redactor;
pub mod registry;
pub mod renderer;
pub mod run_mode;
pub mod runtime_message;
pub mod section_id;
pub mod section_source;
pub mod signals;
pub mod surface;
pub mod surface_extensions;
pub mod templates;

// ── Core re-exports ──────────────────────────────────────────────
pub use budget::PromptBudget;
pub use build_context::{BuildCx, ModelTarget};
pub use cache_marker::{CacheMarker, CacheMarkerArbiter, CacheMarkerSlot, PromptBlock};
pub use clock::{Clock, FixedClock, SystemClock};
pub use composer::{ComposedPrompt, Composer};
pub use error_codes::codes;
pub use exec_policy::SourceExecPolicy;
pub use layer::{
    LayerResolver, PromptLayer, SectionAnchor, SectionAudit, SectionOrder, SectionWarning,
};
pub use redactor::{DefaultRedactor, NoopRedactor, Redactor};
pub use registry::SectionRegistry;
pub use renderer::{MarkdownRenderer, SectionRenderer, XmlRenderer};
pub use run_mode::RunMode;
pub use runtime_message::{
    CompactionPolicy, CurrentDateInjector, RuntimeMessage, RuntimeMessageInjector,
    RuntimeMessagePlacement,
};
pub use section_id::SectionId;
pub use section_source::{
    FatalError, SectionBody, SectionCriticality, SectionMeta, SectionOutcome, SectionSource,
    SectionSpec,
};
pub use signals::{BuildSignal, SignalCache, SignalKey};
pub use surface::{
    CompactionKind, PromptSurface, SubagentCacheStability, SurfaceMatcher, SurfacePattern,
};
pub use surface_extensions::SurfaceExtension;
pub use templates::{
    load_template, parse_front_matter, render_template_strict, HeuristicTokenizer, TemplateError,
    TemplateVars, Tokenizer,
};
