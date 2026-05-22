use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::settings_repo;

pub const WEB_SEARCH_SETTINGS_KEY: &str = "web_search.settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default)]
    pub include_raw_content: bool,
}

impl Default for WebSearchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: default_engine(),
            api_key: None,
            base_url: None,
            max_results: default_max_results(),
            include_raw_content: false,
        }
    }
}

impl WebSearchSettings {
    pub fn sanitized(mut self) -> Self {
        self.api_key = self
            .api_key
            .and_then(|value| normalize_optional_string(&value));
        self.base_url = self
            .base_url
            .and_then(|value| normalize_optional_string(&value));
        self.max_results = self.max_results.clamp(1, 20);
        self
    }

    pub fn is_ready(&self) -> bool {
        self.enabled
            && self
                .api_key
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
    }
}

fn default_engine() -> WebSearchEngine {
    WebSearchEngine::Tavily
}

fn default_max_results() -> usize {
    5
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
            api_key: Some("  key  ".to_string()),
            base_url: Some("  https://example.com  ".to_string()),
            max_results: 99,
            include_raw_content: true,
        }
        .sanitized();

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(settings.base_url.as_deref(), Some("https://example.com"));
        assert_eq!(settings.max_results, 20);
        assert!(settings.is_ready());
    }
}
