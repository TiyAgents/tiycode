use std::sync::Arc;

use sqlx::SqlitePool;

use crate::core::agent_session::RuntimeModelPlan;
use crate::core::subagent::SubagentProfile;

use super::clock::Clock;
use super::feature_set::PromptFeatureSet;
use super::renderer::SectionRenderer;
use super::run_mode::RunMode;
use super::signals::SignalCache;

/// Target LLM model information for budget and renderer selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModelTarget {
    AnthropicClaude {
        context_window: usize,
        supports_cache_control: bool,
    },
    OpenAiCompat {
        context_window: usize,
    },
    Local {
        context_window: usize,
    },
}

impl ModelTarget {
    pub fn context_window(&self) -> usize {
        match self {
            ModelTarget::AnthropicClaude { context_window, .. } => *context_window,
            ModelTarget::OpenAiCompat { context_window } => *context_window,
            ModelTarget::Local { context_window } => *context_window,
        }
    }

    pub fn supports_cache_control(&self) -> bool {
        match self {
            ModelTarget::AnthropicClaude {
                supports_cache_control,
                ..
            } => *supports_cache_control,
            _ => false,
        }
    }
}

/// Aggregated context passed to every SectionSource::build() call.
/// This is the single source of truth for all data a source may need.
pub struct BuildCx<'a> {
    /// SQLite connection pool
    pub pool: &'a SqlitePool,
    /// Current workspace path
    pub workspace_path: &'a str,
    /// Thread ID (None for non-threaded contexts like title generation)
    pub thread_id: Option<&'a str>,
    /// Run ID (None if no active run)
    pub run_id: Option<&'a str>,
    /// Runtime model plan (None for surfaces that don't need it)
    pub raw_plan: Option<&'a RuntimeModelPlan>,
    /// Current run mode
    pub run_mode: RunMode,
    /// Helper profile for subagent surfaces (None for main agent)
    pub helper_profile: Option<&'a SubagentProfile>,
    /// Custom subagent slug for CustomSubagentBody source
    pub custom_subagent_slug: Option<&'a str>,
    /// Override response language for surfaces that don't carry raw_plan
    /// (Compaction / Title). Falls back to raw_plan.response_language when None.
    pub response_language: Option<&'a str>,
    /// Target LLM model info
    pub target_model: ModelTarget,
    /// Time source (must use this, not Utc::now())
    pub clock: Arc<dyn Clock>,
    /// Memoized signal cache for this build
    pub signals: Arc<SignalCache>,
    /// Feature flags for A/B experiments
    pub features: Arc<PromptFeatureSet>,
    /// Section renderer (Markdown/XML) chosen by caller
    pub renderer: Arc<dyn SectionRenderer>,
}

impl<'a> BuildCx<'a> {
    /// Create a build context for the main agent surface.
    pub fn for_main_agent(
        pool: &'a SqlitePool,
        raw_plan: Option<&'a RuntimeModelPlan>,
        workspace_path: &'a str,
        thread_id: Option<&'a str>,
        run_id: Option<&'a str>,
        run_mode: RunMode,
        target_model: ModelTarget,
        clock: Arc<dyn Clock>,
        features: Arc<PromptFeatureSet>,
        renderer: Arc<dyn SectionRenderer>,
    ) -> Self {
        Self {
            pool,
            workspace_path,
            thread_id,
            run_id,
            raw_plan,
            run_mode,
            helper_profile: None,
            custom_subagent_slug: None,
            response_language: None,
            target_model,
            clock,
            signals: Arc::new(SignalCache::new()),
            features,
            renderer,
        }
    }

    /// Derive a helper subagent build context from the parent.
    /// Key differences: new SignalCache (isolation), helper_profile set,
    /// inherited_run_mode from the surface.
    pub fn derive_for_helper(
        parent: &BuildCx<'a>,
        helper_profile: &'a SubagentProfile,
        inherited_run_mode: RunMode,
        renderer: Arc<dyn SectionRenderer>,
    ) -> Self {
        Self {
            pool: parent.pool,
            workspace_path: parent.workspace_path,
            thread_id: parent.thread_id,
            run_id: None, // helper gets its own run_id
            raw_plan: parent.raw_plan,
            run_mode: inherited_run_mode,
            helper_profile: Some(helper_profile),
            custom_subagent_slug: None,
            response_language: parent.response_language,
            target_model: parent.target_model.clone(),
            clock: parent.clock.clone(),
            signals: Arc::new(SignalCache::new()), // isolated cache
            features: parent.features.clone(),
            renderer,
        }
    }

    /// Create an isolated context for render_section_only().
    pub fn for_section_only(parent: &BuildCx<'a>) -> Self {
        Self {
            pool: parent.pool,
            workspace_path: parent.workspace_path,
            thread_id: parent.thread_id,
            run_id: parent.run_id,
            raw_plan: parent.raw_plan,
            run_mode: parent.run_mode,
            helper_profile: parent.helper_profile,
            custom_subagent_slug: parent.custom_subagent_slug,
            response_language: parent.response_language,
            target_model: parent.target_model.clone(),
            clock: parent.clock.clone(),
            signals: Arc::new(SignalCache::standalone()),
            features: parent.features.clone(),
            renderer: parent.renderer.clone(),
        }
    }
}
