use std::borrow::Cow;

use tiycore::catalog::{CatalogMetadataStore, CatalogModelMatch};
use tiycore::types::Provider;

/// Wraps a catalog metadata store and normalizes provider-specific model suffixes
/// before delegating to the underlying store.
pub struct NormalizingCatalogMetadataStore<S: CatalogMetadataStore> {
    inner: S,
}

impl<S: CatalogMetadataStore> NormalizingCatalogMetadataStore<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: CatalogMetadataStore> CatalogMetadataStore for NormalizingCatalogMetadataStore<S> {
    fn find_by_raw_or_alias(
        &self,
        provider: &Provider,
        raw_id: &str,
        normalized_aliases: &[String],
    ) -> Option<CatalogModelMatch> {
        let normalized_raw_id = normalize_model_id_for_catalog_match(raw_id);
        let normalized_aliases_for_lookup = normalize_aliases_for_catalog_match(
            normalized_aliases,
            suffix_token_for_catalog_match(raw_id).as_deref(),
        );
        let aliases_for_lookup = normalized_aliases_for_lookup
            .as_deref()
            .unwrap_or(normalized_aliases);

        self.inner
            .find_by_raw_or_alias(provider, normalized_raw_id.as_ref(), aliases_for_lookup)
    }
}

fn normalize_model_id_for_catalog_match(raw_id: &str) -> Cow<'_, str> {
    let trimmed = raw_id.trim();
    let without_free = trimmed.strip_suffix("-free").unwrap_or(trimmed);
    if let Some((base, suffix)) = without_free.rsplit_once(':') {
        if !base.is_empty() && !suffix.is_empty() {
            return Cow::Owned(base.to_string());
        }
    }

    if without_free.len() != trimmed.len() {
        Cow::Owned(without_free.to_string())
    } else if trimmed.len() != raw_id.len() {
        Cow::Owned(trimmed.to_string())
    } else {
        Cow::Borrowed(raw_id)
    }
}

fn suffix_token_for_catalog_match(raw_id: &str) -> Option<String> {
    let trimmed = raw_id.trim();
    let without_free = trimmed.strip_suffix("-free").unwrap_or(trimmed);
    without_free
        .rsplit_once(':')
        .and_then(|(_, suffix)| normalize_suffix_token(suffix))
}

fn normalize_aliases_for_catalog_match(
    aliases: &[String],
    route_suffix_token: Option<&str>,
) -> Option<Vec<String>> {
    let normalized = aliases
        .iter()
        .map(|alias| normalize_alias_for_catalog_match(alias, route_suffix_token))
        .collect::<Vec<_>>();

    if normalized
        .iter()
        .zip(aliases.iter())
        .any(|(normalized, original)| normalized != original)
    {
        Some(dedupe_strings(normalized))
    } else {
        None
    }
}

fn normalize_alias_for_catalog_match(alias: &str, route_suffix_token: Option<&str>) -> String {
    let mut normalized = alias.to_string();

    if let Some(base) = normalized.strip_suffix("-free") {
        normalized = base.to_string();
    }

    if let Some(token) = route_suffix_token {
        let suffix = format!("-{token}");
        if let Some(base) = normalized.strip_suffix(&suffix) {
            normalized = base.to_string();
        }
    }

    normalized
}

fn normalize_suffix_token(suffix: &str) -> Option<String> {
    let mut out = String::with_capacity(suffix.len());
    let mut last_dash = false;

    for ch in suffix.trim().to_lowercase().chars() {
        let mapped = match ch {
            'a'..='z' | '0'..='9' => Some(ch),
            '/' | '_' | ' ' | ':' | '-' => Some('-'),
            _ => None,
        };

        if let Some(ch) = mapped {
            if ch == '-' {
                if last_dash {
                    continue;
                }
                last_dash = true;
            } else {
                last_dash = false;
            }
            out.push(ch);
        }
    }

    let normalized = out.trim_matches('-').to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;
    use tiycore::catalog::{CatalogModelMatch, CatalogModelMetadata};

    use super::*;

    #[derive(Default)]
    struct RecordingCatalogStore {
        calls: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    }

    impl CatalogMetadataStore for RecordingCatalogStore {
        fn find_by_raw_or_alias(
            &self,
            _provider: &Provider,
            raw_id: &str,
            normalized_aliases: &[String],
        ) -> Option<CatalogModelMatch> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((raw_id.to_string(), normalized_aliases.to_vec()));
            Some(CatalogModelMatch {
                metadata: CatalogModelMetadata {
                    canonical_model_key: raw_id.to_string(),
                    aliases: Vec::new(),
                    display_name: None,
                    description: None,
                    context_window: None,
                    max_output_tokens: None,
                    max_input_tokens: None,
                    modalities: None,
                    capabilities: None,
                    reasoning_content_constrained: false,
                    pricing: None,
                    source: "test".to_string(),
                    raw: json!({}),
                },
                confidence: 1.0,
                matched_alias: Some(raw_id.to_string()),
            })
        }
    }

    #[test]
    fn normalizes_colon_route_suffix_before_single_lookup() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let store = NormalizingCatalogMetadataStore::new(RecordingCatalogStore {
            calls: Arc::clone(&calls),
        });

        store.find_by_raw_or_alias(
            &Provider::OpenAI,
            "deepseek-v4-pro:deepseek",
            &["deepseek-v4-pro-deepseek".to_string()],
        );

        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "deepseek-v4-pro");
        assert_eq!(calls[0].1, vec!["deepseek-v4-pro"]);
    }

    #[test]
    fn normalizes_colon_free_suffix_before_single_lookup() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let store = NormalizingCatalogMetadataStore::new(RecordingCatalogStore {
            calls: Arc::clone(&calls),
        });

        store.find_by_raw_or_alias(
            &Provider::OpenRouter,
            "deepseek/deepseek-v4-pro:free",
            &[
                "deepseek-deepseek-v4-pro-free".to_string(),
                "deepseek-v4-pro-free".to_string(),
            ],
        );

        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "deepseek/deepseek-v4-pro");
        assert_eq!(
            calls[0].1,
            vec!["deepseek-deepseek-v4-pro", "deepseek-v4-pro"]
        );
    }

    #[test]
    fn normalizes_dash_free_suffix_before_single_lookup() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let store = NormalizingCatalogMetadataStore::new(RecordingCatalogStore {
            calls: Arc::clone(&calls),
        });

        store.find_by_raw_or_alias(
            &Provider::Zenmux,
            "deepseek/deepseek-v4-pro-free",
            &[
                "deepseek-deepseek-v4-pro-free".to_string(),
                "deepseek-v4-pro-free".to_string(),
            ],
        );

        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "deepseek/deepseek-v4-pro");
        assert_eq!(
            calls[0].1,
            vec!["deepseek-deepseek-v4-pro", "deepseek-v4-pro"]
        );
    }

    #[test]
    fn keeps_plain_model_id_unchanged() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let store = NormalizingCatalogMetadataStore::new(RecordingCatalogStore {
            calls: Arc::clone(&calls),
        });

        store.find_by_raw_or_alias(
            &Provider::DeepSeek,
            "deepseek-v4-pro",
            &["deepseek-v4-pro".to_string()],
        );

        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "deepseek-v4-pro");
        assert_eq!(calls[0].1, vec!["deepseek-v4-pro"]);
    }
}
