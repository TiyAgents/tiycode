use std::sync::Arc;
use std::time::Instant;

use tokio::time::timeout;

use crate::model::errors::AppError;

use super::budget::PromptBudget;
use super::build_context::BuildCx;
use super::cache_marker::{CacheMarker, CacheMarkerArbiter, CacheMarkerSlot, PromptBlock};
use super::exec_policy::SourceExecPolicy;
use super::layer::{PromptLayer, SectionAudit, SectionWarning};
use super::redactor::Redactor;
use super::registry::SectionRegistry;
use super::section_id::SectionId;
use super::section_source::{SectionBody, SectionOutcome, SectionSpec};
use super::surface::PromptSurface;
use super::templates::{HeuristicTokenizer, Tokenizer};

/// The composed output of a prompt build.
#[derive(Debug)]
pub struct ComposedPrompt {
    /// Complete system prompt text (fallback for providers without block support)
    pub text: String,
    /// Content blocks aligned with LLM provider cache-control APIs
    pub blocks: Vec<PromptBlock>,
    /// Global schema version (structural changes only; § 3.19)
    pub schema_version: u32,
    /// Per-section audit trail
    pub audit: Vec<SectionAudit>,
    /// Warnings collected during composition
    pub warnings: Vec<SectionWarning>,
    /// Cache marker arbiter used during this build, if any.
    /// Available for the message layer to allocate remaining marker quota.
    pub cache_arbiter: Option<Arc<dyn CacheMarkerArbiter>>,
}

/// The prompt composer: orchestrates section building, layer assignment,
/// budget enforcement, and rendering.
pub struct Composer {
    pub registry: Arc<SectionRegistry>,
    exec_policy: SourceExecPolicy,
    redactor: Arc<dyn Redactor>,
    tokenizer: Arc<dyn Tokenizer>,
    /// Cache marker arbiter for coordinating system <-> message layer quota.
    /// When set, the Composer records marker slots after `assign_cache_markers`.
    cache_arbiter: Option<Arc<dyn CacheMarkerArbiter>>,
}

impl Composer {
    pub fn new(
        registry: Arc<SectionRegistry>,
        exec_policy: SourceExecPolicy,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self {
            registry,
            exec_policy,
            redactor,
            tokenizer: Arc::new(HeuristicTokenizer),
            cache_arbiter: None,
        }
    }

    pub fn with_tokenizer(mut self, tokenizer: Arc<dyn Tokenizer>) -> Self {
        self.tokenizer = tokenizer;
        self
    }

    /// Attach a cache marker arbiter for system ↔ message layer coordination.
    /// The arbiter records marker slots after `assign_cache_markers` and is
    /// exposed in `ComposedPrompt::cache_arbiter` for the message layer.
    pub fn with_cache_arbiter(mut self, arbiter: Arc<dyn CacheMarkerArbiter>) -> Self {
        self.cache_arbiter = Some(arbiter);
        self
    }

    // ── Main entry: 7-step build pipeline (§3.3) ──────────────────────
    pub async fn build(
        &self,
        surface: &PromptSurface,
        cx: &BuildCx<'_>,
        budget: &PromptBudget,
    ) -> Result<ComposedPrompt, AppError> {
        let start = Instant::now();

        // Step 1: Filter sections by SurfaceMatcher
        let specs: Vec<&SectionSpec> = self.registry.filter_for_surface(surface);

        // Step 2+3: Build sections + resolve layers (sequential build with per-source timeout;
        // concurrent fan-out within a layer is deferred to a future phase)
        let mut results: Vec<(
            &SectionSpec,
            PromptLayer,
            SectionOutcome,
            std::time::Duration,
        )> = Vec::new();

        for spec in &specs {
            let layer = spec.layer.resolve(surface);
            let source_start = Instant::now();

            let build_fut = spec.source.build(cx);
            let outcome = match timeout(self.exec_policy.per_source_timeout, build_fut).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(fatal)) => SectionOutcome::SoftFailed {
                    code: "source.fatal",
                    error: Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        fatal.message,
                    )),
                },
                Err(_elapsed) => {
                    tracing::warn!(
                        target = "prompt.source.timeout",
                        section = ?spec.id,
                        timeout_ms = self.exec_policy.per_source_timeout.as_millis() as u64,
                        "section source timed out"
                    );
                    SectionOutcome::SoftFailed {
                        code: super::error_codes::codes::SOURCE_TIMEOUT,
                        error: Box::new(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "section source timeout",
                        )),
                    }
                }
            };

            results.push((spec, layer, outcome, source_start.elapsed()));

            // Hard overall budget cap; if exceeded mid-pipeline, stop building further sources
            if start.elapsed() > self.exec_policy.overall_build_timeout {
                tracing::warn!(
                    target = "prompt.compose.overall_timeout",
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    sections_built = results.len(),
                    sections_total = specs.len(),
                    "overall build timeout exceeded; remaining sources skipped"
                );
                break;
            }
        }

        // Step 4: Per-section budget truncation
        let mut bodies: Vec<(
            &SectionSpec,
            PromptLayer,
            SectionBody,
            Option<SectionWarning>,
            std::time::Duration,
        )> = Vec::new();

        for (spec, layer, outcome, elapsed) in results {
            match outcome {
                SectionOutcome::Produced(body) => {
                    let (body, warning) = self.apply_per_section_budget(spec, &body, budget);
                    bodies.push((spec, layer, body, warning, elapsed));
                }
                SectionOutcome::Degraded { body, warning } => {
                    let (body, budget_warning) = self.apply_per_section_budget(spec, &body, budget);
                    let merged_warning = budget_warning.unwrap_or(warning);
                    bodies.push((spec, layer, body, Some(merged_warning), elapsed));
                }
                SectionOutcome::Skip => { /* silently skip */ }
                SectionOutcome::SoftFailed { .. } => { /* silently skip */ }
            }
        }

        // Step 5: Sort by (Layer, then resolved anchored order, then SectionId)
        bodies.sort_by(|(a_spec, a_layer, ..), (b_spec, b_layer, ..)| {
            let layer_cmp = a_layer.cmp(b_layer);
            if layer_cmp != std::cmp::Ordering::Equal {
                return layer_cmp;
            }
            // Within same layer, resolve Anchored positions
            let a_order = Self::resolve_anchored_order(a_spec, &specs);
            let b_order = Self::resolve_anchored_order(b_spec, &specs);
            a_order
                .cmp(&b_order)
                .then_with(|| a_spec.id.cmp(&b_spec.id))
        });

        // Step 6: Total budget enforcement + eviction
        let (kept, eviction_warnings) = self.apply_total_budget(bodies, budget);

        // Step 7: Render blocks + cache markers
        let mut text_parts: Vec<String> = Vec::new();
        let mut blocks: Vec<PromptBlock> = Vec::new();
        let mut audit: Vec<SectionAudit> = Vec::new();
        let mut warnings: Vec<SectionWarning> = Vec::new();
        let renderer = cx.renderer.as_ref();

        for (spec, layer, body, warn, source_elapsed) in &kept {
            let rendered = renderer.render_section(&spec.title, &body.markdown);
            text_parts.push(rendered.clone());

            blocks.push(PromptBlock {
                layer: *layer,
                text: rendered.clone(),
                cache_marker: None, // assigned by assign_cache_markers below
            });

            audit.push(SectionAudit {
                id: spec.id.clone(),
                layer: *layer,
                version: spec.version,
                bytes: body.markdown.len(),
                estimated_tokens: self.tokenizer.estimate(&rendered),
                source_kind: spec.source.source_kind(),
                elapsed: *source_elapsed,
                truncated: warn
                    .as_ref()
                    .map_or(false, |w| matches!(w, SectionWarning::Truncated { .. })),
                template_version: None,
                renderer: renderer.name(),
                tokenizer: self.tokenizer.name(),
            });

            if let Some(w) = warn {
                warnings.push(w.clone());
            }
        }

        warnings.extend(eviction_warnings);

        // Cache markers (§ 3.7.1): place ephemeral markers at end of the most stable
        // non-empty layers, skipping Ephemeral layer.
        Self::assign_cache_markers(&mut blocks, &cx.target_model);

        // Record marker slots via the arbiter so the message layer can coordinate
        // quota (≤4 Anthropic breakpoints across system prompt + runtime messages).
        if let Some(ref arbiter) = self.cache_arbiter {
            let mut byte_offset = 0usize;
            let slots: Vec<CacheMarkerSlot> = blocks
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let offset = byte_offset;
                    byte_offset += b.text.len();
                    b.cache_marker.as_ref().map(|_| CacheMarkerSlot {
                        layer: b.layer,
                        byte_offset_in_text: offset,
                        block_index: i,
                    })
                })
                .collect();
            arbiter.record_system_markers(&slots);
        }

        let text = text_parts.join(renderer.layer_separator());
        let text = self.redactor.redact(&text).into_owned();

        let total_estimated_tokens: usize = audit.iter().map(|a| a.estimated_tokens).sum();
        let truncated_sections = audit.iter().filter(|a| a.truncated).count();

        tracing::info!(
            target = "prompt.compose",
            surface = ?surface,
            schema_version = self.registry.schema_version(),
            sections = audit.len(),
            bytes = text.len(),
            estimated_tokens = total_estimated_tokens,
            warnings = warnings.len(),
            truncated_sections,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "system prompt composed"
        );

        Ok(ComposedPrompt {
            text,
            blocks,
            schema_version: self.registry.schema_version(),
            audit,
            warnings,
            cache_arbiter: self.cache_arbiter.clone(),
        })
    }

    /// Render a single section's body outside the main build pipeline.
    pub async fn render_section_only(
        &self,
        id: &SectionId,
        surface: &PromptSurface,
        cx: &BuildCx<'_>,
    ) -> Option<SectionBody> {
        let spec = self
            .registry
            .filter_for_surface(surface)
            .into_iter()
            .find(|s| &s.id == id)?;

        match spec.source.build(cx).await {
            Ok(SectionOutcome::Produced(body)) => Some(body),
            Ok(SectionOutcome::Degraded { body, .. }) => Some(body),
            _ => None,
        }
    }

    // ── Private helpers ───────────────────────────────────────────────

    /// Resolve the order of a section within its layer, handling Anchored positions.
    /// Returns (base_order, anchor_target_order) for topological comparison.
    fn resolve_anchored_order(spec: &SectionSpec, all_specs: &[&SectionSpec]) -> u32 {
        use super::layer::SectionOrder;

        match &spec.order_hint {
            SectionOrder::First => 0,
            SectionOrder::Last => u32::MAX,
            SectionOrder::Default => 50_000,
            SectionOrder::Anchored(anchor) => {
                // Find the anchor target and compute relative position
                let target_id = match anchor {
                    super::layer::SectionAnchor::Before(id)
                    | super::layer::SectionAnchor::After(id) => id,
                };
                // Find the anchor target's position within the same layer
                let target_spec = all_specs.iter().find(|s| &s.id == target_id);
                let target_order = match target_spec {
                    Some(ts) => Self::resolve_anchored_order(ts, all_specs),
                    None => 50_000, // Anchor missing; fall to default position
                };
                match anchor {
                    super::layer::SectionAnchor::Before(_) => target_order.saturating_sub(1),
                    super::layer::SectionAnchor::After(_) => target_order.saturating_add(1),
                }
            }
        }
    }

    fn apply_per_section_budget(
        &self,
        spec: &SectionSpec,
        body: &SectionBody,
        budget: &PromptBudget,
    ) -> (SectionBody, Option<SectionWarning>) {
        let limit = spec.max_chars.unwrap_or(budget.per_section_default_chars);
        let char_count = body.markdown.chars().count();
        if char_count <= limit {
            return (body.clone(), None);
        }

        let truncated: String = body.markdown.chars().take(limit).collect();
        let warning = SectionWarning::Truncated {
            section_id: spec.id.clone(),
            original_chars: char_count,
            truncated_to: truncated.chars().count(),
        };
        (
            SectionBody {
                markdown: truncated,
                meta: body.meta.clone(),
            },
            Some(warning),
        )
    }

    fn apply_total_budget<'a>(
        &self,
        sections: Vec<(
            &'a SectionSpec,
            PromptLayer,
            SectionBody,
            Option<SectionWarning>,
            std::time::Duration,
        )>,
        budget: &PromptBudget,
    ) -> (
        Vec<(
            &'a SectionSpec,
            PromptLayer,
            SectionBody,
            Option<SectionWarning>,
            std::time::Duration,
        )>,
        Vec<SectionWarning>,
    ) {
        let mut total: usize = 0;
        let mut kept = Vec::new();
        let mut warnings = Vec::new();

        for (spec, layer, body, warn, elapsed) in sections {
            let char_count = body.markdown.chars().count();
            if total + char_count <= budget.total_chars {
                total += char_count;
                kept.push((spec, layer, body, warn, elapsed));
            } else {
                warnings.push(SectionWarning::Evicted {
                    section_id: spec.id.clone(),
                    layer,
                });
            }
        }

        (kept, warnings)
    }

    /// Place ephemeral cache markers (§ 3.7.1 sliding rules):
    /// 1. Skip empty layers.
    /// 2. Place markers at the end of the most stable non-empty layers (StablePrefix > SessionStable > RuntimeOverlay).
    /// 3. Up to 2 markers (system reserves 2 of the 4 Anthropic breakpoints).
    /// 4. Skip Ephemeral layer (by definition unstable).
    /// 5. Skip layers below `min_marker_chars`.
    fn assign_cache_markers(
        blocks: &mut [PromptBlock],
        target: &super::build_context::ModelTarget,
    ) {
        if !target.supports_cache_control() {
            return;
        }
        // Anthropic recommends ≥ 1024 chars for cache_control to be cost-effective
        let min_marker_chars = 1024;

        // Group block indices by layer (preserving order within each layer)
        let mut by_layer: std::collections::BTreeMap<PromptLayer, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (i, b) in blocks.iter().enumerate() {
            if matches!(b.layer, PromptLayer::Ephemeral) {
                continue;
            }
            by_layer.entry(b.layer).or_default().push(i);
        }

        let mut marker_count = 0;
        // Iterate layers in increasing order (StablePrefix < SessionStable < RuntimeOverlay)
        for (_layer, indices) in by_layer.iter() {
            if marker_count >= 2 {
                break;
            }
            // Total chars of this layer
            let layer_chars: usize = indices
                .iter()
                .map(|i| blocks[*i].text.chars().count())
                .sum();
            if layer_chars < min_marker_chars {
                continue;
            }
            if let Some(last_idx) = indices.last() {
                blocks[*last_idx].cache_marker = Some(CacheMarker::Ephemeral);
                marker_count += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::budget::PromptBudget;
    use super::super::build_context::{BuildCx, ModelTarget};
    use super::super::clock::FixedClock;
    use super::super::redactor::NoopRedactor;
    use super::super::renderer::MarkdownRenderer;
    use super::super::run_mode::RunMode;
    use super::super::signals::SignalCache;
    use super::super::surface::PromptSurface;
    use super::*;
    use std::sync::Arc;

    #[test]
    fn cache_purity_stable_prefix_omits_dates_and_ids() {
        // § 3.13: StablePrefix must NEVER contain dates / thread_id / run_id / username.
        // We check the static templates that drive StablePrefix sections in the registry.
        let stable_templates: &[&str] = &[
            include_str!("templates/role.md"),
            include_str!("templates/behavioral_guidelines.md"),
            include_str!("templates/final_response_structure.md"),
        ];
        // ISO date / timestamp / common identifier patterns
        let date_re = regex::Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap();
        for tpl in stable_templates {
            assert!(
                !date_re.is_match(tpl),
                "StablePrefix template contains an ISO date — violates § 3.13 cache purity"
            );
            // thread_id placeholder leakage check
            assert!(
                !tpl.contains("thread_id"),
                "StablePrefix template contains 'thread_id' literal — violates § 3.13"
            );
            assert!(
                !tpl.contains("run_id"),
                "StablePrefix template contains 'run_id' literal — violates § 3.13"
            );
        }
    }

    #[test]
    fn assign_cache_markers_skips_ephemeral_and_short_layers() {
        let target = ModelTarget::AnthropicClaude {
            context_window: 200_000,
            supports_cache_control: true,
        };
        // Two short-but-non-empty stable blocks (< 1024 chars each so neither earns a marker)
        let mut blocks = vec![
            PromptBlock {
                layer: PromptLayer::StablePrefix,
                text: "short".to_string(),
                cache_marker: None,
            },
            PromptBlock {
                layer: PromptLayer::Ephemeral,
                text: "ephemeral".to_string(),
                cache_marker: None,
            },
        ];
        Composer::assign_cache_markers(&mut blocks, &target);
        // Ephemeral never gets a marker; short StablePrefix below threshold also skipped.
        assert!(blocks[0].cache_marker.is_none());
        assert!(blocks[1].cache_marker.is_none());
    }

    #[test]
    fn assign_cache_markers_marks_long_stable_layer() {
        let target = ModelTarget::AnthropicClaude {
            context_window: 200_000,
            supports_cache_control: true,
        };
        let long_text = "x".repeat(2048);
        let mut blocks = vec![
            PromptBlock {
                layer: PromptLayer::StablePrefix,
                text: long_text,
                cache_marker: None,
            },
            PromptBlock {
                layer: PromptLayer::Ephemeral,
                text: "ephemeral".to_string(),
                cache_marker: None,
            },
        ];
        Composer::assign_cache_markers(&mut blocks, &target);
        assert_eq!(blocks[0].cache_marker, Some(CacheMarker::Ephemeral));
        assert!(blocks[1].cache_marker.is_none());
    }

    #[test]
    fn assign_cache_markers_no_op_when_provider_unsupported() {
        let target = ModelTarget::OpenAiCompat {
            context_window: 128_000,
        };
        let mut blocks = vec![PromptBlock {
            layer: PromptLayer::StablePrefix,
            text: "x".repeat(2048),
            cache_marker: None,
        }];
        Composer::assign_cache_markers(&mut blocks, &target);
        assert!(blocks[0].cache_marker.is_none());
    }

    // ── § 3.7.1 cache_marker_quota: total markers must never exceed 4 ──

    #[test]
    fn cache_marker_quota_never_exceeds_four() {
        let target = ModelTarget::AnthropicClaude {
            context_window: 200_000,
            supports_cache_control: true,
        };
        // 6 blocks, all eligible for markers (long enough + non-Ephemeral layer)
        let long_text = "x".repeat(2048);
        let mut blocks: Vec<PromptBlock> = vec![
            PromptLayer::StablePrefix,
            PromptLayer::SessionStable,
            PromptLayer::SessionStable,
            PromptLayer::RuntimeOverlay,
            PromptLayer::RuntimeOverlay,
            PromptLayer::StablePrefix,
        ]
        .into_iter()
        .map(|layer| PromptBlock {
            layer,
            text: long_text.clone(),
            cache_marker: None,
        })
        .collect();

        Composer::assign_cache_markers(&mut blocks, &target);
        let marker_count = blocks.iter().filter(|b| b.cache_marker.is_some()).count();
        assert!(
            marker_count <= 4,
            "cache markers ({marker_count}) exceed maximum 4 — violates § 3.7.1"
        );
        // At least one marker should be assigned (StablePrefix has long enough text)
        assert!(marker_count >= 1, "expected at least one cache marker");
    }

    #[test]
    fn cache_marker_quota_skips_evicted_layer() {
        let target = ModelTarget::AnthropicClaude {
            context_window: 200_000,
            supports_cache_control: true,
        };
        // StablePrefix block is too short to earn a marker (< 1024 chars)
        let mut blocks = vec![PromptBlock {
            layer: PromptLayer::StablePrefix,
            text: "short".to_string(),
            cache_marker: None,
        }];
        Composer::assign_cache_markers(&mut blocks, &target);
        assert!(
            blocks[0].cache_marker.is_none(),
            "short StablePrefix should not get a marker — violates layer sliding rule"
        );
    }

    // ── § 3.18 source_idempotency: same BuildCx → equivalent output ──

    #[tokio::test]
    async fn source_idempotency_deterministic_template_source() {
        // Template sources with no dynamic data must produce identical output
        // on repeated calls with the same BuildCx.
        let registry = Arc::new(crate::core::prompt::registry::default_registry());
        let composer = Composer::new(
            registry,
            SourceExecPolicy::default(),
            Arc::new(NoopRedactor),
        );
        // Use Title surface as a simple deterministic case
        let surface = PromptSurface::Title;

        // First build
        let result1 = {
            let placeholder_pool =
                sqlx::SqlitePool::connect_lazy("sqlite::memory:").expect("placeholder pool");
            let cx = BuildCx {
                pool: &placeholder_pool,
                workspace_path: "/tmp/test",
                thread_id: None,
                run_id: None,
                raw_plan: None,
                run_mode: RunMode::Default,
                helper_profile: None,
                response_language: None,
                custom_subagent_slug: None,
                target_model: ModelTarget::AnthropicClaude {
                    context_window: 200_000,
                    supports_cache_control: true,
                },
                renderer: Arc::new(MarkdownRenderer),
                clock: Arc::new(FixedClock::new(chrono::Utc::now())),
                signals: Arc::new(SignalCache::standalone()),
            };
            let budget = PromptBudget::default();
            composer
                .build(&surface, &cx, &budget)
                .await
                .expect("build should succeed")
        };

        // Second build with identical parameters
        let result2 = {
            let placeholder_pool =
                sqlx::SqlitePool::connect_lazy("sqlite::memory:").expect("placeholder pool");
            let cx = BuildCx {
                pool: &placeholder_pool,
                workspace_path: "/tmp/test",
                thread_id: None,
                run_id: None,
                raw_plan: None,
                run_mode: RunMode::Default,
                helper_profile: None,
                response_language: None,
                custom_subagent_slug: None,
                target_model: ModelTarget::AnthropicClaude {
                    context_window: 200_000,
                    supports_cache_control: true,
                },
                renderer: Arc::new(MarkdownRenderer),
                clock: Arc::new(FixedClock::new(chrono::Utc::now())),
                signals: Arc::new(SignalCache::standalone()),
            };
            let budget = PromptBudget::default();
            composer
                .build(&surface, &cx, &budget)
                .await
                .expect("build should succeed")
        };

        assert_eq!(
            result1.text, result2.text,
            "same BuildCx must produce byte-equal output — violates § 3.18 idempotency"
        );
    }

    // ── § 3.18 source_determinism: no SystemTime::now() side effects ──

    #[tokio::test]
    async fn source_determinism_fixed_clock_produces_stable_output() {
        let registry = Arc::new(crate::core::prompt::registry::default_registry());
        let composer = Composer::new(
            registry,
            SourceExecPolicy::default(),
            Arc::new(NoopRedactor),
        );
        let surface = PromptSurface::MainAgent {
            run_mode: RunMode::Default,
        };

        // Fixed clock at two different points in time
        let t1 = chrono::Utc::now();
        let t2 = t1 + chrono::Duration::days(30);

        let result1 = {
            let placeholder_pool =
                sqlx::SqlitePool::connect_lazy("sqlite::memory:").expect("placeholder pool");
            let cx = BuildCx {
                pool: &placeholder_pool,
                workspace_path: "/tmp/test",
                thread_id: None,
                run_id: None,
                raw_plan: None,
                run_mode: RunMode::Default,
                helper_profile: None,
                response_language: None,
                custom_subagent_slug: None,
                target_model: ModelTarget::AnthropicClaude {
                    context_window: 200_000,
                    supports_cache_control: true,
                },
                renderer: Arc::new(MarkdownRenderer),
                clock: Arc::new(FixedClock::new(t1)),
                signals: Arc::new(SignalCache::standalone()),
            };
            let budget = PromptBudget::default();
            composer
                .build(&surface, &cx, &budget)
                .await
                .expect("build should succeed")
        };

        let result2 = {
            let placeholder_pool =
                sqlx::SqlitePool::connect_lazy("sqlite::memory:").expect("placeholder pool");
            let cx = BuildCx {
                pool: &placeholder_pool,
                workspace_path: "/tmp/test",
                thread_id: None,
                run_id: None,
                raw_plan: None,
                run_mode: RunMode::Default,
                helper_profile: None,
                response_language: None,
                custom_subagent_slug: None,
                target_model: ModelTarget::AnthropicClaude {
                    context_window: 200_000,
                    supports_cache_control: true,
                },
                renderer: Arc::new(MarkdownRenderer),
                clock: Arc::new(FixedClock::new(t2)),
                signals: Arc::new(SignalCache::standalone()),
            };
            let budget = PromptBudget::default();
            composer
                .build(&surface, &cx, &budget)
                .await
                .expect("build should succeed")
        };

        // With fixed clock, system prompt should be identical regardless of time
        // (current_date is in runtime_message, not system prompt)
        assert_eq!(
            result1.text, result2.text,
            "fixed clock must produce byte-equal output — violates § 3.18 determinism"
        );
    }
}
