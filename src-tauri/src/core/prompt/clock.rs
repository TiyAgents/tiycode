use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Abstract clock to allow deterministic time in tests.
/// All time-dependent sources must use this instead of `Utc::now()` / `SystemTime::now()`.
pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
}

/// Default production clock using system time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Fixed clock for testing; always returns the same timestamp.
pub struct FixedClock {
    pub timestamp: DateTime<Utc>,
}

impl FixedClock {
    pub fn new(timestamp: DateTime<Utc>) -> Self {
        Self { timestamp }
    }
}

impl Clock for FixedClock {
    fn now_utc(&self) -> DateTime<Utc> {
        self.timestamp
    }
}

/// Convenience constructor for test clocks.
pub fn fixed_clock_for_test() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        DateTime::parse_from_rfc3339("2026-06-05T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
    ))
}
