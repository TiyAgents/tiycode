use super::layer::PromptLayer;

/// A content block in the composed prompt, aligned with LLM provider cache-control APIs.
/// Anthropic supports up to 4 cache_control breakpoints per request.
#[derive(Debug, Clone)]
pub struct PromptBlock {
    /// Which stability layer this block belongs to
    pub layer: PromptLayer,
    /// Rendered text content for this block
    pub text: String,
    /// Optional cache breakpoint marker at the end of this block
    pub cache_marker: Option<CacheMarker>,
}

/// Cache marker type sent to the LLM provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMarker {
    /// Standard ephemeral cache (Anthropic `cache_control: { type: "ephemeral" }`)
    Ephemeral,
    /// Reserved for future persistent/session-level cache
    Persistent,
}

/// Global cache marker arbiter for a single LLM request.
/// Enforces the ≤4 breakpoint limit across system prompt + messages.
pub trait CacheMarkerArbiter: Send + Sync + std::fmt::Debug {
    /// Called by Composer after rendering: records system prompt markers.
    fn record_system_markers(&self, markers: &[CacheMarkerSlot]);

    /// Called by the message layer before serialization: requests remaining quota.
    fn allocate_for_messages(&self, requested: usize) -> usize;

    /// Must be reset after each LLM call to prevent cross-request leakage.
    fn reset(&self);
}

/// Describes where a cache marker was placed.
#[derive(Debug, Clone)]
pub struct CacheMarkerSlot {
    pub layer: PromptLayer,
    pub byte_offset_in_text: usize,
    pub block_index: usize,
}

/// Standard implementation that enforces total ≤ 4 markers.
#[derive(Debug)]
pub struct DefaultCacheMarkerArbiter {
    max_total: usize,
    system_markers: std::sync::Mutex<Vec<CacheMarkerSlot>>,
}

impl DefaultCacheMarkerArbiter {
    pub fn new(max_total: usize) -> Self {
        Self {
            max_total,
            system_markers: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl CacheMarkerArbiter for DefaultCacheMarkerArbiter {
    fn record_system_markers(&self, markers: &[CacheMarkerSlot]) {
        let mut sys = self.system_markers.lock().unwrap();
        *sys = markers.to_vec();
    }

    fn allocate_for_messages(&self, requested: usize) -> usize {
        let sys = self.system_markers.lock().unwrap();
        let remaining = self.max_total.saturating_sub(sys.len());
        requested.min(remaining)
    }

    fn reset(&self) {
        let mut sys = self.system_markers.lock().unwrap();
        sys.clear();
    }
}
