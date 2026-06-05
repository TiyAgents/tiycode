use std::sync::Arc;

use sqlx::SqlitePool;

use crate::core::agent_session::RuntimeModelPlan;
use crate::core::subagent::SubagentProfile;

use super::clock::Clock;
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

/// Build context passed to every SectionSource::build call.
///
/// Contains all runtime data needed for prompt construction.
/// Any field not used by a particular source is simply ignored.
#[derive(Clone)]
pub struct BuildCx<'a> {
    /// SQLite connection pool for data queries.
    pub pool: &'a SqlitePool,
    /// Absolute path to the current workspace.
    pub workspace_path: &'a str,
    /// Current thread ID, if available.
    pub thread_id: Option<&'a str>,
    /// Current run ID, if available.
    pub run_id: Option<&'a str>,
    /// The resolved runtime model plan (model + provider info).
    pub raw_plan: Option<&'a RuntimeModelPlan>,
    /// Current run mode (Default, Plan, etc.).
    pub run_mode: RunMode,
    /// Helper subagent profile, when building a subagent prompt.
    pub helper_profile: Option<&'a SubagentProfile>,
    /// Custom subagent slug (set for SubagentCustom surfaces).
    pub custom_subagent_slug: Option<&'a str>,
    /// Target model info for budget computation.
    pub target_model: ModelTarget,
    /// Clock abstraction for time-sensitive sections.
    pub clock: Arc<dyn Clock>,
    /// Signal cache shared across sections in this build.
    pub signals: Arc<SignalCache>,
    /// Section renderer to use.
    pub renderer: Arc<dyn SectionRenderer>,
    /// Response language override, if any.
    pub response_language: Option<&'a str>,
}

impl<'a> BuildCx<'a> {
    /// Create a context for the main agent surface.
    pub fn for_main_agent(
        pool: &'a SqlitePool,
        workspace_path: &'a str,
        thread_id: Option<&'a str>,
        run_id: Option<&'a str>,
        raw_plan: Option<&'a RuntimeModelPlan>,
        run_mode: RunMode,
        clock: Arc<dyn Clock>,
        renderer: Arc<dyn SectionRenderer>,
        response_language: Option<&'a str>,
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
            target_model: ModelTarget::AnthropicClaude {
                context_window: 200_000,
                supports_cache_control: true,
            },
            clock,
            signals: Arc::new(SignalCache::new()),
            renderer,
            response_language,
        }
    }

    /// Derive a child context for a helper subagent, sharing clock and renderer
    /// but with a fresh SignalCache (subagent builds are independent).
    pub fn derive_for_helper(
        &self,
        helper_profile: &'a SubagentProfile,
        custom_subagent_slug: Option<&'a str>,
    ) -> Self {
        Self {
            pool: self.pool,
            workspace_path: self.workspace_path,
            thread_id: self.thread_id,
            run_id: self.run_id,
            raw_plan: self.raw_plan,
            run_mode: self.run_mode,
            helper_profile: Some(helper_profile),
            custom_subagent_slug,
            target_model: self.target_model.clone(),
            clock: self.clock.clone(),
            signals: Arc::new(SignalCache::new()),
            renderer: self.renderer.clone(),
            response_language: self.response_language,
        }
    }

    /// Create an isolated context for render_section_only, with its own
    /// SignalCache so it does not pollute the main build path.
    pub fn for_section_only(&self) -> Self {
        Self {
            pool: self.pool,
            workspace_path: self.workspace_path,
            thread_id: self.thread_id,
            run_id: self.run_id,
            raw_plan: self.raw_plan,
            run_mode: self.run_mode,
            helper_profile: self.helper_profile,
            custom_subagent_slug: self.custom_subagent_slug,
            target_model: self.target_model.clone(),
            clock: self.clock.clone(),
            signals: Arc::new(SignalCache::new()),
            renderer: self.renderer.clone(),
            response_language: self.response_language,
        }
    }
}
