/// § 3.18: Idempotency and determinism tests for SectionSource implementations.
/// These tests verify that sources produce stable output under identical inputs.

#[cfg(test)]
mod tests {
    use super::super::super::build_context::{BuildCx, ModelTarget};
    use super::super::super::clock::FixedClock;
    use super::super::super::renderer::MarkdownRenderer;
    use super::super::super::run_mode::RunMode;
    use super::super::super::section_source::SectionSource;
    use super::super::super::signals::SignalCache;
    use super::super::{RunModeSource, SystemEnvironmentSource};
    use std::sync::Arc;

    /// Construct a minimal BuildCx for testing sources that don't need DB access.
    async fn test_cx() -> BuildCx<'static> {
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let pool_ref = Box::leak(Box::new(pool));
        BuildCx {
            pool: pool_ref,
            workspace_path: "/test/workspace",
            thread_id: None,
            run_id: None,
            raw_plan: None,
            run_mode: RunMode::Default,
            helper_profile: None,
            custom_subagent_slug: None,
            target_model: ModelTarget::AnthropicClaude {
                context_window: 200_000,
                supports_cache_control: true,
            },
            clock: Arc::new(FixedClock::new(chrono::Utc::now())),
            signals: Arc::new(SignalCache::new()),
            renderer: Arc::new(MarkdownRenderer),
            response_language: None,
        }
    }

    /// § 3.18: source_idempotency — same BuildCx must produce the same output.
    /// SystemEnvironmentSource is fully deterministic (env::consts only).
    #[tokio::test]
    async fn source_idempotency_system_environment() {
        let cx = test_cx().await;
        let source = SystemEnvironmentSource::new(1);

        let out1 = source.build(&cx).await.unwrap();
        let out2 = source.build(&cx).await.unwrap();

        match (&out1, &out2) {
            (
                super::super::super::section_source::SectionOutcome::Produced(b1),
                super::super::super::section_source::SectionOutcome::Produced(b2),
            ) => {
                assert_eq!(
                    b1.markdown, b2.markdown,
                    "SystemEnvironmentSource is not idempotent"
                );
            }
            _ => panic!("expected Produced outcomes"),
        }
    }

    /// § 3.18: source_determinism — output must be stable under deterministic inputs.
    /// SystemEnvironment produces the same OS/arch/shell for a given machine.
    #[tokio::test]
    async fn source_determinism_system_environment() {
        let cx = test_cx().await;
        let source = SystemEnvironmentSource::new(1);

        // Build 3 times; all should be identical
        let mut prev: Option<String> = None;
        for i in 0..3 {
            let out = source.build(&cx).await.unwrap();
            if let super::super::super::section_source::SectionOutcome::Produced(b) = out {
                if let Some(ref p) = prev {
                    assert_eq!(
                        &b.markdown, p,
                        "SystemEnvironmentSource output diverged on build {}",
                        i
                    );
                }
                prev = Some(b.markdown);
            } else {
                panic!("expected Produced on build {}", i);
            }
        }
    }

    /// RunModeSource idempotency across both plan and default modes.
    #[tokio::test]
    async fn source_idempotency_run_mode() {
        let source = RunModeSource::new(1);

        for mode in &[RunMode::Default, RunMode::Plan] {
            let cx = test_cx().await;
            // Create a separate cx for each mode to avoid mutability issues.
            let cx_mode = BuildCx {
                run_mode: *mode,
                ..cx
            };

            let out1 = source.build(&cx_mode).await.unwrap();
            let out2 = source.build(&cx_mode).await.unwrap();

            match (&out1, &out2) {
                (
                    super::super::super::section_source::SectionOutcome::Produced(b1),
                    super::super::super::section_source::SectionOutcome::Produced(b2),
                ) => {
                    assert_eq!(
                        b1.markdown, b2.markdown,
                        "RunModeSource is not idempotent for {:?}",
                        mode
                    );
                }
                _ => panic!("expected Produced for {:?}", mode),
            }
        }
    }

    /// RunModeSource: plan mode and default mode must produce different outputs.
    #[tokio::test]
    async fn run_mode_plan_vs_default_differ() {
        let source = RunModeSource::new(1);
        let base_cx = test_cx().await;

        let cx_plan = BuildCx {
            run_mode: RunMode::Plan,
            ..base_cx.clone()
        };
        let cx_default = BuildCx {
            run_mode: RunMode::Default,
            ..base_cx
        };

        let out_plan = source.build(&cx_plan).await.unwrap();
        let out_default = source.build(&cx_default).await.unwrap();

        match (&out_plan, &out_default) {
            (
                super::super::super::section_source::SectionOutcome::Produced(bp),
                super::super::super::section_source::SectionOutcome::Produced(bd),
            ) => {
                assert_ne!(
                    bp.markdown, bd.markdown,
                    "plan and default run_mode must produce different output"
                );
            }
            _ => panic!("expected Produced outcomes"),
        }
    }
}
