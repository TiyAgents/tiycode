use std::sync::Arc;

use async_trait::async_trait;

use super::build_context::BuildCx;
use super::surface::PromptSurface;

/// Runtime message injector: produces transient messages that are injected
/// into the conversation before each turn, keeping the system prompt stable
/// for LLM prefix-cache optimization.
#[async_trait]
pub trait RuntimeMessageInjector: Send + Sync {
    /// Whether this injector applies to the given surface.
    fn applies_to(&self, surface: &PromptSurface) -> bool;

    /// Build the runtime message, if applicable.
    async fn build_message(&self, cx: &BuildCx<'_>) -> Option<RuntimeMessage>;
}

/// A runtime message to be injected into the conversation.
#[derive(Debug, Clone)]
pub struct RuntimeMessage {
    /// Message text content
    pub text: String,
    /// Kind of runtime message (for filtering/discovery)
    pub kind: RuntimeMessageKind,
    /// How compaction should handle this message
    pub compaction_policy: CompactionPolicy,
    /// Where in the message sequence to place this message
    pub placement: RuntimeMessagePlacement,
    /// Dedup ID: same-ID messages from previous turns are replaced
    pub dedup_id: Option<&'static str>,
}

/// Categorization of runtime messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMessageKind {
    /// Current date/time context
    CurrentDate,
    /// Active PR or branch status
    ActivePr,
    /// Other transient context
    Other,
}

/// How compaction should treat this runtime message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionPolicy {
    /// Default: may be absorbed by compaction; re-injected next turn
    AbsorbAndReinject,
    /// Excluded from the compaction window (prevents double-injection in summary-of-summary)
    PinOutsideWindow,
}

/// Where in the message sequence the runtime message is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMessagePlacement {
    /// Right after the system prompt, before any user/assistant messages
    AfterSystem,
    /// Before the latest user message (default; after cache marker)
    BeforeLatestUser,
}

/// Injects the current date as a runtime message each turn.
pub struct CurrentDateInjector {
    pub clock: Arc<dyn super::clock::Clock>,
}

impl CurrentDateInjector {
    pub fn new(clock: Arc<dyn super::clock::Clock>) -> Self {
        Self { clock }
    }
}

#[async_trait]
impl RuntimeMessageInjector for CurrentDateInjector {
    fn applies_to(&self, surface: &PromptSurface) -> bool {
        // Applies to main agent and all subagent surfaces
        matches!(
            surface,
            PromptSurface::MainAgent { .. }
                | PromptSurface::SubagentExplore { .. }
                | PromptSurface::SubagentReview { .. }
                | PromptSurface::SubagentCustom { .. }
        )
    }

    async fn build_message(&self, _cx: &BuildCx<'_>) -> Option<RuntimeMessage> {
        let now = self.clock.now_utc();
        let date_str = now.format("%Y-%m-%d").to_string();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        Some(RuntimeMessage {
            text: format!(
                "<runtime_context turn_started_at=\"{}\">\nCurrent date: {}\n</runtime_context>",
                timestamp, date_str
            ),
            kind: RuntimeMessageKind::CurrentDate,
            compaction_policy: CompactionPolicy::PinOutsideWindow,
            placement: RuntimeMessagePlacement::BeforeLatestUser,
            dedup_id: Some("current_date"),
        })
    }
}
