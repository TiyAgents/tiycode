use std::collections::HashMap;

/// Runtime feature flags for prompt composition.
/// Controls A/B experiments and gradual rollouts without redeployment.
#[derive(Debug, Clone)]
pub struct PromptFeatureSet {
    flags: HashMap<&'static str, FeatureValue>,
}

#[derive(Debug, Clone)]
pub enum FeatureValue {
    Bool(bool),
    Percent(u8), // 0..=100, bucketed by thread_id hash
    Variant(&'static str),
}

impl PromptFeatureSet {
    pub fn empty() -> Self {
        Self {
            flags: HashMap::new(),
        }
    }

    pub fn with_flag(mut self, key: &'static str, value: FeatureValue) -> Self {
        self.flags.insert(key, value);
        self
    }

    /// Check if a boolean flag is enabled for the given salt (thread_id or workspace_path).
    pub fn is_enabled(&self, key: &'static str, salt: &str) -> bool {
        match self.flags.get(key) {
            Some(FeatureValue::Bool(b)) => *b,
            Some(FeatureValue::Percent(pct)) => {
                // Simple hash-based bucketing
                let hash = salt
                    .bytes()
                    .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                (hash % 100) < (*pct as u64)
            }
            Some(FeatureValue::Variant(_)) => true, // variants are always "enabled" (call variant() to get the value)
            None => false,
        }
    }

    /// Get a variant value for a flag, if set and matching the salt bucket.
    pub fn variant(&self, key: &'static str, salt: &str) -> Option<&'static str> {
        match self.flags.get(key) {
            Some(FeatureValue::Variant(v)) => {
                let hash = salt
                    .bytes()
                    .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                if (hash % 100) < 100 {
                    Some(v)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Record which flags were read during a build for audit.
    pub fn snapshot_accessed(&self, _accessed: &[&'static str]) -> HashMap<&'static str, String> {
        let mut snapshot = HashMap::new();
        for key in _accessed {
            if let Some(val) = self.flags.get(key) {
                snapshot.insert(*key, format!("{:?}", val));
            }
        }
        snapshot
    }
}

impl Default for PromptFeatureSet {
    fn default() -> Self {
        Self::empty()
    }
}
