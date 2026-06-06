use std::collections::BTreeMap;

use super::build_context::ModelTarget;
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
    pub fn for_model(model: &ModelTarget, surface: &PromptSurface) -> Self {
        let context_window = match model {
            ModelTarget::AnthropicClaude { context_window, .. } => *context_window,
            ModelTarget::OpenAiCompat { context_window } => *context_window,
            ModelTarget::Local { context_window } => *context_window,
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::prompt::run_mode::RunMode;

    fn model_200k() -> ModelTarget {
        ModelTarget::AnthropicClaude {
            context_window: 200_000,
            supports_cache_control: true,
        }
    }

    #[test]
    fn default_budget_has_sane_limits() {
        let budget = PromptBudget::default();
        assert_eq!(budget.total_chars, 60_000);
        assert_eq!(budget.per_section_default_chars, 6_000);
        assert!(
            budget.per_section_overrides.is_empty(),
            "default budget should have no overrides"
        );
    }

    #[test]
    fn default_eviction_order_is_least_stable_first() {
        let budget = PromptBudget::default();
        assert_eq!(budget.eviction_order.len(), 4);
        assert_eq!(budget.eviction_order[0], PromptLayer::Ephemeral);
        assert_eq!(budget.eviction_order[1], PromptLayer::RuntimeOverlay);
        assert_eq!(budget.eviction_order[2], PromptLayer::SessionStable);
        assert_eq!(budget.eviction_order[3], PromptLayer::StablePrefix);
    }

    #[test]
    fn for_model_scales_with_context_window() {
        let model = ModelTarget::AnthropicClaude {
            context_window: 200_000,
            supports_cache_control: true,
        };
        let budget = PromptBudget::for_model(
            &model,
            &PromptSurface::MainAgent {
                run_mode: RunMode::Default,
            },
        );
        // 200_000 × 4.0 × 0.30 = 240_000 chars
        assert_eq!(budget.total_chars, 240_000);
        // per_section_default_chars = 240_000 × 0.10 = 24_000
        assert_eq!(budget.per_section_default_chars, 24_000);
    }

    #[test]
    fn for_model_sets_per_section_overrides() {
        let model = model_200k();
        let budget = PromptBudget::for_model(
            &model,
            &PromptSurface::MainAgent {
                run_mode: RunMode::Default,
            },
        );
        assert_eq!(
            budget
                .per_section_overrides
                .get(&SectionId::BehavioralGuidelines),
            Some(&120_000) // total_chars / 2
        );
        assert_eq!(
            budget
                .per_section_overrides
                .get(&SectionId::FinalResponseStructure),
            Some(&60_000) // total_chars / 4
        );
        assert_eq!(
            budget.per_section_overrides.get(&SectionId::ProjectContext),
            Some(&30_000) // total_chars / 8
        );
        assert_eq!(
            budget
                .per_section_overrides
                .get(&SectionId::CustomSubagentBody),
            Some(&60_000) // total_chars / 4
        );
    }

    #[test]
    fn compaction_surface_halves_total_chars() {
        let model = model_200k();
        let main_budget = PromptBudget::for_model(
            &model,
            &PromptSurface::MainAgent {
                run_mode: RunMode::Default,
            },
        );
        let compact_budget = PromptBudget::for_model(
            &model,
            &PromptSurface::Compaction {
                kind: crate::core::prompt::surface::CompactionKind::Compact,
            },
        );
        let merge_budget = PromptBudget::for_model(
            &model,
            &PromptSurface::Compaction {
                kind: crate::core::prompt::surface::CompactionKind::Merge,
            },
        );
        let title_budget = PromptBudget::for_model(&model, &PromptSurface::Title);

        assert_eq!(main_budget.total_chars, 240_000);
        assert_eq!(compact_budget.total_chars, 120_000);
        assert_eq!(merge_budget.total_chars, 120_000);
        assert_eq!(title_budget.total_chars, 120_000);
    }

    #[test]
    fn small_context_window_produces_proportional_budget() {
        let model = ModelTarget::AnthropicClaude {
            context_window: 32_000,
            supports_cache_control: true,
        };
        let budget = PromptBudget::for_model(
            &model,
            &PromptSurface::MainAgent {
                run_mode: RunMode::Default,
            },
        );
        // 32_000 × 4.0 × 0.30 = 38_400
        assert_eq!(budget.total_chars, 38_400);
        assert_eq!(budget.per_section_default_chars, 3_840);
    }

    #[test]
    fn budget_eviction_order_preserves_stable_prefix_last() {
        let budget = PromptBudget::default();
        let last = budget.eviction_order.last().copied().unwrap();
        assert_eq!(
            last,
            PromptLayer::StablePrefix,
            "StablePrefix must be evicted last to preserve LLM cache"
        );
    }
}
