use std::collections::BTreeMap;

use super::layer::PromptLayer;
use super::section_id::SectionId;
use super::surface::PromptSurface;

/// Length budget for prompt composition.
/// Prevents system prompt from consuming the LLM's entire context window.
#[derive(Debug, Clone)]
pub struct PromptBudget {
    /// Global character limit (derived from model context window × 0.30 × ~4 chars/token).
    pub total_chars: usize,

    /// Default per-section character limit.
    pub per_section_default_chars: usize,

    /// Per-section override limits.
    pub per_section_overrides: BTreeMap<SectionId, usize>,

    /// Eviction order: layers are removed in this order when total budget is exceeded.
    /// Default: [Ephemeral, RuntimeOverlay, SessionStable, StablePrefix]
    pub eviction_order: Vec<PromptLayer>,
}

impl Default for PromptBudget {
    fn default() -> Self {
        // Conservative default: ~200K context → ~60K chars for system prompt
        Self {
            total_chars: 60_000,
            per_section_default_chars: 6_000,
            per_section_overrides: BTreeMap::new(),
            eviction_order: vec![
                PromptLayer::Ephemeral,
                PromptLayer::RuntimeOverlay,
                PromptLayer::SessionStable,
                PromptLayer::StablePrefix,
            ],
        }
    }
}

impl PromptBudget {
    /// Create a budget tuned for a specific model's context window.
    pub fn for_model(context_window: usize, surface: &PromptSurface) -> Self {
        let total_chars = ((context_window as f32) * 4.0 * 0.30) as usize;
        let per_section_default_chars = (total_chars as f32 * 0.10) as usize;

        let mut per_section_overrides = BTreeMap::new();
        // Large static sections get more room
        per_section_overrides.insert(SectionId::BehavioralGuidelines, total_chars / 2);
        per_section_overrides.insert(SectionId::FinalResponseStructure, total_chars / 4);
        // User-provided sections get tighter limits
        per_section_overrides.insert(SectionId::ProjectContext, total_chars / 8);
        per_section_overrides.insert(SectionId::CustomSubagentBody, total_chars / 4);

        // Compaction / Title surfaces use tighter budgets
        let total_chars = match surface {
            PromptSurface::Compaction { .. } | PromptSurface::Title => total_chars / 2,
            _ => total_chars,
        };

        Self {
            total_chars,
            per_section_default_chars,
            per_section_overrides,
            eviction_order: vec![
                PromptLayer::Ephemeral,
                PromptLayer::RuntimeOverlay,
                PromptLayer::SessionStable,
                PromptLayer::StablePrefix,
            ],
        }
    }
}
