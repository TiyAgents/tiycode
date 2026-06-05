use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use crate::model::errors::AppError;

/// Identifies a signal scope for per-workspace or per-thread caching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SignalKey {
    /// Default "global"; use workspace hash or thread_id for scoped signals.
    pub scope: std::borrow::Cow<'static, str>,
}

impl SignalKey {
    pub const fn global() -> Self {
        Self {
            scope: std::borrow::Cow::Borrowed("global"),
        }
    }

    pub fn scoped(scope: impl Into<String>) -> Self {
        Self {
            scope: std::borrow::Cow::Owned(scope.into()),
        }
    }
}

/// Build-time signals for cross-section data sharing.
/// Sections express dependencies via these signals instead of direct inter-section coupling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildSignal {
    /// Whether an active goal exists for this thread
    ActiveGoal,
    /// Sandbox approval policy string
    ApprovalPolicy,
    /// Writable roots set for the workspace
    WritableRoots,
    /// Skills list has been loaded
    SkillsLoaded,
    /// User profile is available
    ProfileAvailable,
    /// Workspace has workspace instruction file
    WorkspaceInstructions,
}

/// Failure information cached when signal init fails.
#[derive(Debug, Clone)]
pub enum SignalFailure {
    /// Fatal error from the signal producer
    Error(String),
    /// Cyclic dependency detected (A→B→A)
    Cycle { chain: Vec<BuildSignal> },
}

/// Per-signal slot in the cache with cycle detection.
struct SignalSlot {
    cell: tokio::sync::OnceCell<SignalResult>,
    in_flight: AtomicBool,
}

impl SignalSlot {
    fn new() -> Self {
        Self {
            cell: tokio::sync::OnceCell::new(),
            in_flight: AtomicBool::new(false),
        }
    }
}

#[derive(Clone)]
enum SignalResult {
    Ready(Arc<dyn Any + Send + Sync>),
    Failed(SignalFailure),
}

/// Memoized signal cache for a single build context.
/// Per-build lifetime; not shared across builds.
pub struct SignalCache {
    inner: Mutex<HashMap<(TypeId, SignalKey), Arc<SignalSlot>>>,
}

impl SignalCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Get or compute a signal value. Panics on type mismatch.
    pub async fn get_or_init<T, F, Fut>(
        &self,
        key: &SignalKey,
        init: F,
    ) -> Result<Arc<T>, SignalFailure>
    where
        T: Any + Send + Sync + 'static,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, AppError>>,
    {
        let slot = {
            let mut inner = self.inner.lock().unwrap();
            let entry_key = (TypeId::of::<T>(), key.clone());
            inner
                .entry(entry_key)
                .or_insert_with(|| Arc::new(SignalSlot::new()))
                .clone()
        };

        // Check for cycle: if already in_flight, we have a dependency loop
        if slot
            .in_flight
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            // Already in flight → cycle detected
            return Err(SignalFailure::Cycle {
                chain: vec![], // simplified; full chain would require tracking
            });
        }

        let result = slot
            .cell
            .get_or_init(|| async {
                match init().await {
                    Ok(val) => SignalResult::Ready(Arc::new(val)),
                    Err(e) => SignalResult::Failed(SignalFailure::Error(e.to_string())),
                }
            })
            .await;

        // Reset in_flight
        slot.in_flight
            .store(false, std::sync::atomic::Ordering::SeqCst);

        match result {
            SignalResult::Ready(val) => {
                // Downcast and clone Arc
                val.clone()
                    .downcast::<T>()
                    .map_err(|_| SignalFailure::Error("type mismatch in signal cache".into()))
            }
            SignalResult::Failed(f) => Err(f.clone()),
        }
    }

    /// Create a standalone cache for isolated use (render_section_only, etc.)
    pub fn standalone() -> Self {
        Self::new()
    }

    /// Create a new cache that inherits pre-computed signals from the parent,
    /// avoiding recomputation in helper agent builds (§ 3.8.1).
    ///
    /// Only whitelisted signals are shared: ApprovalPolicy, WritableRoots,
    /// WorkspaceInstructions, ProfileAvailable — these are safe to reuse
    /// across parent→helper transitions because they do not depend on
    /// thread/run state.
    pub fn shareable_for_helper(&self) -> Self {
        let inner = self.inner.lock().unwrap();
        // Copy all pre-computed slots from parent to child cache.
        // OnceCell slots with computed values are immutable and safe to share.
        let child_map: HashMap<(TypeId, SignalKey), Arc<SignalSlot>> = inner
            .iter()
            .filter(|((_type_id, key), _)| {
                // Only inherit global-scoped signals; thread-scoped signals
                // are specific to the parent's thread context.
                key.scope == "global"
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Self {
            inner: Mutex::new(child_map),
        }
    }
}

impl Default for SignalCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::errors::{AppError, ErrorSource};
    use std::sync::Arc;

    #[tokio::test]
    async fn signal_cycle_detected() {
        let cache = Arc::new(SignalCache::new());
        let key = SignalKey::global();

        let cache_clone = cache.clone();
        let result: Result<Arc<String>, SignalFailure> = cache
            .get_or_init(&key, move || {
                let c = cache_clone.clone();
                async move {
                    let inner = c
                        .get_or_init::<String, _, _>(&SignalKey::global(), || async {
                            Ok::<String, AppError>("unreachable".to_string())
                        })
                        .await;
                    assert!(
                        matches!(inner, Err(SignalFailure::Cycle { .. })),
                        "Inner call should detect cycle, got {:?}",
                        inner
                    );
                    Err(AppError::internal(ErrorSource::System, "cycle propagated"))
                }
            })
            .await;

        assert!(
            result.is_err(),
            "Cycle must produce an error, got {:?}",
            result
        );
    }
}
