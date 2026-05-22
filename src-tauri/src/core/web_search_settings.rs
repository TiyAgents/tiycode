use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::settings_repo;

pub const WEB_SEARCH_SETTINGS_KEY: &str = "web_search.settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchEngine {
    Tavily,
    Brave,
    Exa,
    Firecrawl,
}

impl WebSearchEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tavily => "tavily",
            Self::Brave => "brave",
            Self::Exa => "exa",
            Self::Firecrawl => "firecrawl",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_engine")]
    pub engine: WebSearchEngine,
    #[serde(default, rename = "apiKeys")]
    pub api_keys: BTreeMap<WebSearchEngine, String>,
    #[serde(default, rename = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, rename = "baseUrls")]
    pub base_urls: BTreeMap<WebSearchEngine, String>,
    #[serde(default, rename = "baseUrl", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default = "default_include_raw_content")]
    pub include_raw_content: bool,
}

impl Default for WebSearchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: default_engine(),
            api_keys: BTreeMap::new(),
            api_key: None,
            base_urls: BTreeMap::new(),
            base_url: None,
            max_results: default_max_results(),
            include_raw_content: default_include_raw_content(),
        }
    }
}

impl WebSearchSettings {
    pub fn sanitized(mut self) -> Self {
        self.api_keys = self
            .api_keys
            .into_iter()
            .filter_map(|(engine, value)| {
                normalize_optional_string(&value).map(|value| (engine, value))
            })
            .collect();
        self.api_key = self
            .api_key
            .and_then(|value| normalize_optional_string(&value));
        if let Some(api_key) = self.api_key.as_ref() {
            self.api_keys
                .entry(self.engine)
                .or_insert_with(|| api_key.clone());
        }
        self.api_key = None;
        self.base_urls = self
            .base_urls
            .into_iter()
            .filter_map(|(engine, value)| {
                normalize_optional_string(&value).map(|value| (engine, value))
            })
            .collect();
        self.base_url = self
            .base_url
            .and_then(|value| normalize_optional_string(&value));
        if let Some(base_url) = self.base_url.as_ref() {
            self.base_urls
                .entry(self.engine)
                .or_insert_with(|| base_url.clone());
        }
        self.base_url = None;
        self.max_results = self.max_results.clamp(1, 20);
        self
    }

    pub fn api_key_for_active_engine(&self) -> Option<&str> {
        self.api_keys
            .get(&self.engine)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn base_url_for_active_engine(&self) -> Option<&str> {
        self.base_urls
            .get(&self.engine)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn is_ready(&self) -> bool {
        self.enabled && self.api_key_for_active_engine().is_some()
    }
}

fn default_engine() -> WebSearchEngine {
    WebSearchEngine::Tavily
}

fn default_max_results() -> usize {
    5
}

fn default_include_raw_content() -> bool {
    true
}

fn normalize_optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn load_web_search_settings(pool: &SqlitePool) -> Result<WebSearchSettings, AppError> {
    let Some(record) = settings_repo::get(pool, WEB_SEARCH_SETTINGS_KEY).await? else {
        return Ok(WebSearchSettings::default());
    };

    serde_json::from_str::<WebSearchSettings>(&record.value_json)
        .map(WebSearchSettings::sanitized)
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.web_search.invalid_json",
                format!("Invalid Web Search settings: {error}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_api_key_and_limits() {
        let settings = WebSearchSettings {
            enabled: true,
            engine: WebSearchEngine::Exa,
            api_keys: BTreeMap::from([
                (WebSearchEngine::Tavily, "  tavily-key  ".to_string()),
                (WebSearchEngine::Brave, "   ".to_string()),
            ]),
            api_key: Some("  exa-key  ".to_string()),
            base_urls: BTreeMap::from([
                (
                    WebSearchEngine::Tavily,
                    "  https://tavily.example  ".to_string(),
                ),
                (WebSearchEngine::Brave, "   ".to_string()),
            ]),
            base_url: Some("  https://example.com  ".to_string()),
            max_results: 99,
            include_raw_content: true,
        }
        .sanitized();

        assert_eq!(settings.api_key, None);
        assert_eq!(
            settings
                .api_keys
                .get(&WebSearchEngine::Tavily)
                .map(String::as_str),
            Some("tavily-key")
        );
        assert!(!settings.api_keys.contains_key(&WebSearchEngine::Brave));
        assert_eq!(settings.api_key_for_active_engine(), Some("exa-key"));
        assert_eq!(settings.base_url, None);
        assert_eq!(
            settings
                .base_urls
                .get(&WebSearchEngine::Tavily)
                .map(String::as_str),
            Some("https://tavily.example")
        );
        assert!(!settings.base_urls.contains_key(&WebSearchEngine::Brave));
        assert_eq!(
            settings.base_url_for_active_engine(),
            Some("https://example.com")
        );
        assert_eq!(settings.max_results, 20);
        assert!(settings.is_ready());
    }

    #[test]
    fn deserializes_api_keys_from_persisted_json() {
        let settings = serde_json::from_str::<WebSearchSettings>(
            r#"{
                "enabled": true,
                "engine": "brave",
                "apiKeys": {
                    "brave": "  brave-key  ",
                    "exa": "exa-key"
                },
                "baseUrls": {
                    "brave": "  https://brave.example/search  ",
                    "exa": "https://exa.example/search"
                },
                "maxResults": 0,
                "includeRawContent": true
            }"#,
        )
        .expect("valid settings")
        .sanitized();

        assert_eq!(settings.api_key_for_active_engine(), Some("brave-key"));
        assert_eq!(
            settings.base_url_for_active_engine(),
            Some("https://brave.example/search")
        );
        assert_eq!(
            settings
                .base_urls
                .get(&WebSearchEngine::Exa)
                .map(String::as_str),
            Some("https://exa.example/search")
        );
        assert_eq!(
            settings
                .api_keys
                .get(&WebSearchEngine::Exa)
                .map(String::as_str),
            Some("exa-key")
        );
        assert_eq!(settings.max_results, 1);
        assert!(settings.include_raw_content);
    }

    #[test]
    fn missing_include_raw_content_defaults_to_enabled() {
        let settings = serde_json::from_str::<WebSearchSettings>(
            r#"{
                "enabled": true,
                "engine": "tavily",
                "maxResults": 5
            }"#,
        )
        .expect("valid settings")
        .sanitized();

        assert!(settings.include_raw_content);
    }

    #[test]
    fn current_engine_api_keys_take_precedence_over_legacy_api_key() {
        let settings = WebSearchSettings {
            enabled: true,
            engine: WebSearchEngine::Brave,
            api_keys: BTreeMap::from([(WebSearchEngine::Brave, "brave-key".to_string())]),
            api_key: Some("legacy-key".to_string()),
            base_urls: BTreeMap::new(),
            base_url: None,
            max_results: 5,
            include_raw_content: false,
        }
        .sanitized();

        assert_eq!(settings.api_key_for_active_engine(), Some("brave-key"));
    }
}
