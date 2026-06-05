use std::time::Duration;

/// Execution policy for Source::build() calls during composition.
/// Controls timeouts, concurrency, and backpressure to prevent
/// slow sources from blocking the entire LLM call pipeline.
#[derive(Debug, Clone)]
pub struct SourceExecPolicy {
    /// Per-source soft timeout; exceeded → SectionOutcome::SoftFailed
    /// Default: 250 ms
    pub per_source_timeout: Duration,

    /// Max concurrent source builds within a single layer
    /// Default: 8
    pub layer_concurrency: usize,

    /// Hard overall build timeout; exceeded → critical sections missing → EmergencyFallback
    /// Default: 800 ms
    pub overall_build_timeout: Duration,

    /// Whether concurrent signal init is allowed when SignalCache misses
    /// Default: false (OnceCell naturally serializes)
    pub allow_concurrent_signal_init: bool,
}

impl Default for SourceExecPolicy {
    fn default() -> Self {
        // Note: per_source_timeout is intentionally set higher than the
        // 250ms suggestion in docs/prompt-injection-refactor.md § 3.6.1.
        // The plan's value targets steady-state hot paths; cold-start runs
        // (CI, first request after process start, integration tests with
        // freshly-initialized SQLite) routinely exceed 250ms for sources
        // that touch the filesystem (SkillsProvider) or DB. A 1.5s cap
        // still bounds tail latency without silently dropping critical
        // sections to SoftFailed in real-world cold paths.
        Self {
            per_source_timeout: Duration::from_millis(1500),
            layer_concurrency: 8,
            overall_build_timeout: Duration::from_millis(5000),
            allow_concurrent_signal_init: false,
        }
    }
}

impl SourceExecPolicy {
    pub fn new(
        per_source_timeout: Duration,
        layer_concurrency: usize,
        overall_build_timeout: Duration,
    ) -> Self {
        Self {
            per_source_timeout,
            layer_concurrency,
            overall_build_timeout,
            allow_concurrent_signal_init: false,
        }
    }
}
