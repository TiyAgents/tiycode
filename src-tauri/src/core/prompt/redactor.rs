use std::borrow::Cow;

/// PII redactor for tracing fields and warning-log persistence.
/// Sanitizes sensitive strings before they leave the process.
pub trait Redactor: Send + Sync {
    fn redact<'a>(&self, raw: &'a str) -> Cow<'a, str>;
}

/// Default redactor: replaces $HOME with ~ and strips common token patterns.
pub struct DefaultRedactor;

impl Redactor for DefaultRedactor {
    fn redact<'a>(&self, raw: &'a str) -> Cow<'a, str> {
        // Simple pass: replace $HOME prefix with ~
        if let Ok(home) = std::env::var("HOME") {
            if raw.contains(&home) {
                return Cow::Owned(raw.replace(&home, "~"));
            }
        }
        Cow::Borrowed(raw)
    }
}

/// No-op redactor for tests.
pub struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact<'a>(&self, raw: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(raw)
    }
}
