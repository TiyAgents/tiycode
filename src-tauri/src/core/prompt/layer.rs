/// Cache-aware prompt layers ordered by stability.
/// Sections are grouped into these layers for LLM prefix-cache optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PromptLayer {
    /// Cross-session stable. No thread/run/timestamp data allowed.
    /// Determines LLM provider prefix-cache hit rate.
    StablePrefix,
    /// Thread-level stable. Same thread, not reset until context reset.
    /// Example: Project Context, Profile Instructions, Run Mode, Skills snapshot.
    SessionStable,
    /// May change between builds. Runtime data without dates.
    /// Example: Sandbox Policy, Workspace Path.
    RuntimeOverlay,
    /// One-shot, state-dependent transient data.
    /// Example: Active Goal, Active Plan, Active Task Board hints.
    Ephemeral,
}

/// Resolves which PromptLayer a section belongs to, either statically or per-surface.
pub enum LayerResolver {
    /// Same layer for all surfaces
    Fixed(PromptLayer),
    /// Layer depends on which surface is being built
    PerSurface(fn(&super::surface::PromptSurface) -> PromptLayer),
}

impl LayerResolver {
    pub fn resolve(&self, surface: &super::surface::PromptSurface) -> PromptLayer {
        match self {
            LayerResolver::Fixed(layer) => *layer,
            LayerResolver::PerSurface(f) => f(surface),
        }
    }
}

/// Semantic ordering hint for sections within the same layer.
/// Replaces the old bare `u16` with anchor-relative or absolute positioning.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionOrder {
    /// Anchored at the very beginning of the layer
    First,
    /// Positioned relative to another section (before/after)
    Anchored(SectionAnchor),
    /// Default middle slot
    Default,
    /// Anchored at the very end of the layer
    Last,
}

/// Relative anchor positioning for SectionOrder::Anchored.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionAnchor {
    Before(super::section_id::SectionId),
    After(super::section_id::SectionId),
}

/// Warning produced during section construction or layer resolution.
#[derive(Debug, Clone)]
pub enum SectionWarning {
    /// A per-section budget truncation occurred
    Truncated {
        section_id: super::section_id::SectionId,
        original_chars: usize,
        truncated_to: usize,
    },
    /// An anchor target was missing (section not in current surface / soft-failed)
    AnchorMissing {
        section_id: super::section_id::SectionId,
        anchor: SectionAnchor,
    },
    /// A section was evicted due to total budget overflow
    Evicted {
        section_id: super::section_id::SectionId,
        layer: PromptLayer,
    },
    /// Generic soft warning with code
    SoftWarning {
        section_id: super::section_id::SectionId,
        code: &'static str,
        detail: String,
    },
    /// Emergency fallback was injected because all sections failed/skipped
    EmergencyFallback,
}

/// Audit trail for a single section in a composed prompt.
#[derive(Debug, Clone)]
pub struct SectionAudit {
    pub id: super::section_id::SectionId,
    pub layer: PromptLayer,
    pub version: u32,
    pub bytes: usize,
    pub estimated_tokens: usize,
    pub source_kind: &'static str,
    pub elapsed: std::time::Duration,
    pub fallback_used: bool,
    pub truncated: bool,
    /// Template version from front-matter (if template-backed)
    pub template_version: Option<u32>,
    /// Renderer name used
    pub renderer: &'static str,
    /// Tokenizer name used
    pub tokenizer: &'static str,
}

#[cfg(test)]
mod tests {
    use super::super::run_mode::RunMode;
    use super::super::surface::PromptSurface;
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn collect_anchors() -> Vec<(super::super::section_id::SectionId, SectionAnchor)> {
        use super::super::registry::default_registry;
        let registry = default_registry();
        registry
            .iter()
            .filter_map(|spec| {
                if let SectionOrder::Anchored(anchor) = &spec.order_hint {
                    Some((spec.id.clone(), anchor.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn anchors_are_well_formed() {
        let registry = super::super::registry::default_registry();
        let valid_ids: HashSet<super::super::section_id::SectionId> =
            registry.iter().map(|s| s.id.clone()).collect();

        for (section_id, anchor) in collect_anchors() {
            let target = match &anchor {
                SectionAnchor::Before(id) | SectionAnchor::After(id) => id,
            };
            assert!(
                valid_ids.contains(target),
                "Section {:?} anchors to {:?} which is not in the registry",
                section_id,
                target
            );
        }
    }

    #[test]
    fn anchors_do_not_form_cycles() {
        let anchors = collect_anchors();
        let mut after_edges: HashSet<(
            super::super::section_id::SectionId,
            super::super::section_id::SectionId,
        )> = HashSet::new();

        for (src, anchor) in &anchors {
            if let SectionAnchor::After(target) = anchor {
                after_edges.insert((src.clone(), target.clone()));
            }
        }

        for (a, b) in &after_edges {
            assert!(
                !after_edges.contains(&(b.clone(), a.clone())),
                "Cycle: {:?}.After({:?}) and {:?}.After({:?})",
                a,
                b,
                b,
                a
            );
        }
    }

    #[test]
    fn anchors_target_same_layer() {
        use super::super::registry::default_registry;
        let registry = default_registry();
        let surface = PromptSurface::MainAgent {
            run_mode: RunMode::Default,
        };
        let layer_map: HashMap<super::super::section_id::SectionId, PromptLayer> = registry
            .iter()
            .map(|spec| (spec.id.clone(), spec.layer.resolve(&surface)))
            .collect();

        for (src, anchor) in collect_anchors() {
            let target = match &anchor {
                SectionAnchor::Before(id) | SectionAnchor::After(id) => id,
            };
            let src_layer = layer_map.get(&src);
            let tgt_layer = layer_map.get(target);
            if let (Some(sl), Some(tl)) = (src_layer, tgt_layer) {
                assert_eq!(
                    sl, tl,
                    "Section {:?} (layer {:?}) anchors to {:?} (layer {:?}) — must be same layer",
                    src, sl, target, tl
                );
            }
        }
    }
}
