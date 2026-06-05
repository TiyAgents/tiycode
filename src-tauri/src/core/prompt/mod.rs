// Legacy modules (kept for backward compat during migration)
pub mod active_goal_source;
pub mod active_plan_source;
pub mod assembler;
pub mod compaction_contract_source;
pub mod context;
pub mod providers;
pub mod section;
pub mod skills_source;
pub mod subagent_output_contract_source;
pub mod title_contract_source;

// New modules (Phase 0+)
pub mod budget;
pub mod build_context;
pub mod cache_marker;
pub mod clock;
pub mod composer;
pub mod error_codes;
pub mod exec_policy;
pub mod inheritance;
pub mod layer;
pub mod legacy_adapter;
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
pub mod template_sources;
pub mod templates;

// Legacy re-exports
pub use assembler::build_system_prompt;
pub use context::PromptBuildContext;
pub use section::{PromptPhase, PromptSection, PromptSectionProvider};

// New re-exports (additive)
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
