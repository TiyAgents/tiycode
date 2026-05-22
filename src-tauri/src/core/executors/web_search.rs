use std::sync::OnceLock;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};

use reqwest::StatusCode;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::core::executors::ToolOutput;
use crate::core::web_search_settings::{
    load_web_search_settings, WebSearchEngine, WebSearchSettings,
};
use crate::model::errors::{AppError, ErrorSource};

const HTTP_TIMEOUT_SECS: u64 = 30;
const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024; // 5 MB
const MAX_QUERY_CHARS: usize = 500;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .expect("failed to build HTTP client")
    })
}

#[derive(Debug, Clone)]
struct WebSearchInput {
    query: String,
    max_results: usize,
    include_raw_content: bool,
    time_range: Option<String>,
    include_domains: Vec<String>,
    exclude_domains: Vec<String>,
    country: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct StandardSearchResult {
    title: String,
    url: String,
    snippet: Option<String>,
    content: Option<String>,
    published_at: Option<String>,
    score: Option<f64>,
}

pub async fn execute(input: &Value, pool: &SqlitePool) -> Result<ToolOutput, AppError> {
    let settings = load_web_search_settings(pool).await?;
    if !settings.enabled {
        return Ok(config_error(
            "Web Search is disabled in Settings / General.",
        ));
    }

    let Some(api_key) = settings.api_key_for_active_engine() else {
        return Ok(config_error(&format!(
            "Web Search API key for {} is not configured in Settings / General.",
            settings.engine.as_str()
        )));
    };

    let search_input = parse_input(input, &settings)?;
    let client = http_client();

    match settings.engine {
        WebSearchEngine::Tavily => search_tavily(&client, &settings, api_key, &search_input).await,
        WebSearchEngine::Brave => search_brave(&client, &settings, api_key, &search_input).await,
        WebSearchEngine::Exa => search_exa(&client, &settings, api_key, &search_input).await,
        WebSearchEngine::Firecrawl => {
            search_firecrawl(&client, &settings, api_key, &search_input).await
        }
    }
}

fn config_error(message: &str) -> ToolOutput {
    ToolOutput {
        success: false,
        result: json!({
            "error": message,
            "kind": "configuration",
        }),
    }
}

fn parse_input(input: &Value, settings: &WebSearchSettings) -> Result<WebSearchInput, AppError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::validation(ErrorSource::Tool, "web_search requires a non-empty query")
        })?
        .to_string();

    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(AppError::validation(
            ErrorSource::Tool,
            &format!("web_search query exceeds maximum length of {MAX_QUERY_CHARS} characters"),
        ));
    }

    let max_results = settings.max_results.clamp(1, 20);
    let include_raw_content = settings.include_raw_content;

    Ok(WebSearchInput {
        query,
        max_results,
        include_raw_content,
        time_range: input
            .get("timeRange")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        include_domains: string_array(input.get("includeDomains")),
        exclude_domains: string_array(input.get("excludeDomains")),
        country: input
            .get("country")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

async fn search_tavily(
    client: &reqwest::Client,
    settings: &WebSearchSettings,
    api_key: &str,
    input: &WebSearchInput,
) -> Result<ToolOutput, AppError> {
    let endpoint = settings
        .base_url_for_active_engine()
        .unwrap_or("https://api.tavily.com/search");
    let mut body = json!({
        "query": input.query,
        "search_depth": "basic",
        "max_results": input.max_results,
        "include_answer": true,
        "include_raw_content": input.include_raw_content,
    });
    insert_optional(
        &mut body,
        "topic",
        topic_from_time_range(input.time_range.as_deref()),
    );
    insert_string_array(&mut body, "include_domains", &input.include_domains);
    insert_string_array(&mut body, "exclude_domains", &input.exclude_domains);
    if let Some(country) = input.country.as_deref() {
        insert_string(&mut body, "country", country);
    }
    insert_string(&mut body, "api_key", api_key);

    let value = post_json(client.post(endpoint).json(&body)).await?;
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(map_tavily_result).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(success_output(
        WebSearchEngine::Tavily,
        &input.query,
        value
            .get("answer")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        results,
    ))
}

async fn search_brave(
    client: &reqwest::Client,
    settings: &WebSearchSettings,
    api_key: &str,
    input: &WebSearchInput,
) -> Result<ToolOutput, AppError> {
    let endpoint = settings
        .base_url_for_active_engine()
        .unwrap_or("https://api.search.brave.com/res/v1/web/search");
    let mut query = vec![
        (
            "q",
            brave_query(&input.query, &input.include_domains, &input.exclude_domains),
        ),
        ("count", input.max_results.to_string()),
        ("extra_snippets", "true".to_string()),
    ];
    if let Some(country) = input.country.as_deref() {
        query.push(("country", country.to_string()));
    }
    if let Some(freshness) = brave_freshness(input.time_range.as_deref()) {
        query.push(("freshness", freshness.to_string()));
    }

    let value = get_json(
        client
            .get(endpoint)
            .header("X-Subscription-Token", api_key)
            .query(&query),
    )
    .await?;
    let results = value
        .pointer("/web/results")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(map_brave_result).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(success_output(
        WebSearchEngine::Brave,
        &input.query,
        None,
        results,
    ))
}

async fn search_exa(
    client: &reqwest::Client,
    settings: &WebSearchSettings,
    api_key: &str,
    input: &WebSearchInput,
) -> Result<ToolOutput, AppError> {
    let endpoint = settings
        .base_url_for_active_engine()
        .unwrap_or("https://api.exa.ai/search");
    let mut contents = json!({
        "highlights": true,
        "summary": true,
    });
    if input.include_raw_content {
        contents["text"] = json!(true);
    }
    let mut body = json!({
        "query": input.query,
        "type": "auto",
        "numResults": input.max_results,
        "contents": contents,
    });
    insert_string_array(&mut body, "includeDomains", &input.include_domains);
    insert_string_array(&mut body, "excludeDomains", &input.exclude_domains);
    if let Some((start, end)) = exa_published_date_range(input.time_range.as_deref()) {
        insert_string(&mut body, "startPublishedDate", &start);
        insert_string(&mut body, "endPublishedDate", &end);
    }
    if let Some(country) = input.country.as_deref() {
        insert_string(&mut body, "userLocation", country);
    }

    let value = post_json(
        client
            .post(endpoint)
            .header("x-api-key", api_key)
            .json(&body),
    )
    .await?;
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(map_exa_result).collect::<Vec<_>>())
        .unwrap_or_default();
    let answer = value
        .pointer("/output/content")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    Ok(success_output(
        WebSearchEngine::Exa,
        &input.query,
        answer,
        results,
    ))
}

async fn search_firecrawl(
    client: &reqwest::Client,
    settings: &WebSearchSettings,
    api_key: &str,
    input: &WebSearchInput,
) -> Result<ToolOutput, AppError> {
    let endpoint = settings
        .base_url_for_active_engine()
        .unwrap_or("https://api.firecrawl.dev/v2/search");
    let mut body = json!({
        "query": input.query,
        "limit": input.max_results,
        "sources": ["web"],
    });
    insert_string_array(&mut body, "includeDomains", &input.include_domains);
    insert_string_array(&mut body, "excludeDomains", &input.exclude_domains);
    if let Some(country) = input.country.as_deref() {
        insert_string(&mut body, "country", country);
    }
    if let Some(tbs) = firecrawl_tbs(input.time_range.as_deref()) {
        insert_string(&mut body, "tbs", tbs);
    }
    if input.include_raw_content {
        body["scrapeOptions"] = json!({
            "formats": ["markdown"],
            "onlyMainContent": true,
        });
    }

    let value = post_json(client.post(endpoint).bearer_auth(api_key).json(&body)).await?;
    let results = value
        .pointer("/data/web")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(map_firecrawl_result).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(success_output(
        WebSearchEngine::Firecrawl,
        &input.query,
        None,
        results,
    ))
}

async fn post_json(builder: reqwest::RequestBuilder) -> Result<Value, AppError> {
    response_json(builder.send().await).await
}

async fn get_json(builder: reqwest::RequestBuilder) -> Result<Value, AppError> {
    response_json(builder.send().await).await
}

async fn response_json(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<Value, AppError> {
    let response = response.map_err(|error| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.web_search.request_failed",
            format!("Web Search request failed: {error}"),
        )
    })?;
    let status = response.status();
    let raw_body = read_limited_response_body(response).await?;
    let body = String::from_utf8_lossy(&raw_body);
    if !status.is_success() {
        return Err(http_error(status, &body));
    }
    serde_json::from_str::<Value>(&body).map_err(|error| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.web_search.invalid_response",
            format!("Web Search returned invalid JSON: {error}"),
        )
    })
}

async fn read_limited_response_body(mut response: reqwest::Response) -> Result<Vec<u8>, AppError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(response_too_large_error());
    }

    let mut raw_body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.web_search.response_failed",
            format!("Failed to read Web Search response: {error}"),
        )
    })? {
        if raw_body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(response_too_large_error());
        }
        raw_body.extend_from_slice(&chunk);
    }
    Ok(raw_body)
}

fn response_too_large_error() -> AppError {
    AppError::recoverable(
        ErrorSource::Tool,
        "tool.web_search.response_too_large",
        format!(
            "Web Search response exceeded size limit ({} bytes)",
            MAX_RESPONSE_BYTES
        ),
    )
}

fn http_error(status: StatusCode, body: &str) -> AppError {
    let preview: String = body.chars().take(500).collect();
    AppError::recoverable(
        ErrorSource::Tool,
        "tool.web_search.http_error",
        format!("Web Search request failed with HTTP {status}: {preview}"),
    )
}

fn success_output(
    engine: WebSearchEngine,
    query: &str,
    answer: Option<String>,
    results: Vec<StandardSearchResult>,
) -> ToolOutput {
    let sources: Vec<Value> = results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            json!({
                "index": index + 1,
                "title": result.title,
                "url": result.url,
            })
        })
        .collect();
    let result_values: Vec<Value> = results
        .into_iter()
        .map(|result| {
            let mut value = json!({
                "title": result.title,
                "url": result.url,
            });
            insert_optional(&mut value, "snippet", result.snippet);
            insert_optional(&mut value, "content", result.content);
            insert_optional(&mut value, "publishedAt", result.published_at);
            if let Some(score) = result.score {
                value["score"] = json!(score);
            }
            value
        })
        .collect();

    let mut output = json!({
        "query": query,
        "engine": engine.as_str(),
        "results": result_values,
        "sources": sources,
    });
    insert_optional(&mut output, "answer", answer);

    ToolOutput {
        success: true,
        result: output,
    }
}

fn map_tavily_result(value: &Value) -> StandardSearchResult {
    StandardSearchResult {
        title: string_field(value, "title").unwrap_or_else(|| "Untitled".to_string()),
        url: string_field(value, "url").unwrap_or_default(),
        snippet: string_field(value, "content"),
        content: string_field(value, "raw_content"),
        published_at: string_field(value, "published_date"),
        score: value.get("score").and_then(Value::as_f64),
    }
}

fn map_brave_result(value: &Value) -> StandardSearchResult {
    let extra = value
        .get("extra_snippets")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty());
    StandardSearchResult {
        title: string_field(value, "title").unwrap_or_else(|| "Untitled".to_string()),
        url: string_field(value, "url").unwrap_or_default(),
        snippet: string_field(value, "description"),
        content: extra,
        published_at: string_field(value, "age"),
        score: None,
    }
}

fn map_exa_result(value: &Value) -> StandardSearchResult {
    let highlights = value
        .get("highlights")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty());
    StandardSearchResult {
        title: string_field(value, "title").unwrap_or_else(|| "Untitled".to_string()),
        url: string_field(value, "url").unwrap_or_default(),
        snippet: string_field(value, "summary").or(highlights),
        content: string_field(value, "text"),
        published_at: string_field(value, "publishedDate"),
        score: None,
    }
}

fn map_firecrawl_result(value: &Value) -> StandardSearchResult {
    StandardSearchResult {
        title: string_field(value, "title")
            .or_else(|| {
                value
                    .pointer("/metadata/title")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "Untitled".to_string()),
        url: string_field(value, "url")
            .or_else(|| {
                value
                    .pointer("/metadata/sourceURL")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .unwrap_or_default(),
        snippet: string_field(value, "description").or_else(|| {
            value
                .pointer("/metadata/description")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        }),
        content: string_field(value, "markdown"),
        published_at: string_field(value, "date"),
        score: None,
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn insert_optional(target: &mut Value, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        target[key] = json!(value);
    }
}

fn insert_string(target: &mut Value, key: &str, value: &str) {
    if !value.trim().is_empty() {
        target[key] = json!(value.trim());
    }
}

fn insert_string_array(target: &mut Value, key: &str, values: &[String]) {
    if !values.is_empty() {
        target[key] = json!(values);
    }
}

fn brave_query(query: &str, include_domains: &[String], exclude_domains: &[String]) -> String {
    let mut parts = vec![query.trim().to_string()];
    parts.extend(
        include_domains
            .iter()
            .filter_map(|domain| sanitize_domain_filter(domain))
            .map(|domain| format!("site:{domain}")),
    );
    parts.extend(
        exclude_domains
            .iter()
            .filter_map(|domain| sanitize_domain_filter(domain))
            .map(|domain| format!("-site:{domain}")),
    );
    parts.join(" ")
}

fn sanitize_domain_filter(value: &str) -> Option<String> {
    let mut domain = value.trim();
    if domain.is_empty() || domain.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some((_, rest)) = domain.split_once("://") {
        domain = rest;
    }
    domain = domain
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches('.');
    if let Some((host, port)) = domain.rsplit_once(':') {
        if !host.contains(':') && port.chars().all(|ch| ch.is_ascii_digit()) {
            domain = host;
        }
    }
    if domain.is_empty()
        || domain.len() > 253
        || !domain
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
    {
        return None;
    }
    let labels_are_valid = domain.split('.').all(|label| {
        !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-')
    });
    labels_are_valid.then(|| domain.to_ascii_lowercase())
}

fn exa_published_date_range(time_range: Option<&str>) -> Option<(String, String)> {
    let now = Utc::now();
    let start = match time_range {
        Some("day") => now - ChronoDuration::days(1),
        Some("week") => now - ChronoDuration::weeks(1),
        Some("month") => now - ChronoDuration::days(31),
        Some("year") => now - ChronoDuration::days(366),
        _ => return None,
    };
    Some((
        start.to_rfc3339_opts(SecondsFormat::Secs, true),
        now.to_rfc3339_opts(SecondsFormat::Secs, true),
    ))
}

fn topic_from_time_range(time_range: Option<&str>) -> Option<String> {
    match time_range {
        Some("day" | "week") => Some("news".to_string()),
        _ => None,
    }
}

fn brave_freshness(time_range: Option<&str>) -> Option<&'static str> {
    match time_range {
        Some("day") => Some("pd"),
        Some("week") => Some("pw"),
        Some("month") => Some("pm"),
        Some("year") => Some("py"),
        _ => None,
    }
}

fn firecrawl_tbs(time_range: Option<&str>) -> Option<&'static str> {
    match time_range {
        Some("day") => Some("qdr:d"),
        Some("week") => Some("qdr:w"),
        Some("month") => Some("qdr:m"),
        Some("year") => Some("qdr:y"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_input_uses_settings_for_result_limit_and_raw_content() {
        let settings = WebSearchSettings {
            max_results: 5,
            include_raw_content: false,
            ..WebSearchSettings::default()
        };
        let input = parse_input(
            &json!({
                "query": "rust news",
                "maxResults": 10,
                "includeRawContent": true
            }),
            &settings,
        )
        .expect("valid input");

        assert_eq!(input.max_results, 5);
        assert!(!input.include_raw_content);
    }

    #[test]
    fn brave_query_applies_sanitized_domain_filters() {
        let query = brave_query(
            "rust async",
            &[
                "example.com".to_string(),
                " https://Docs.RS/std ".to_string(),
                "bad domain.com".to_string(),
                "example.com OR site:evil.test".to_string(),
            ],
            &[
                "spam.test".to_string(),
                "https://Ads.Example:443/path".to_string(),
                "-invalid.example".to_string(),
            ],
        );

        assert_eq!(
            query,
            "rust async site:example.com site:docs.rs -site:spam.test -site:ads.example"
        );
    }

    #[test]
    fn exa_date_range_maps_supported_time_ranges() {
        let Some((start, end)) = exa_published_date_range(Some("week")) else {
            panic!("expected date range");
        };

        assert!(start.ends_with('Z'));
        assert!(end.ends_with('Z'));
        assert!(exa_published_date_range(Some("invalid")).is_none());
    }

    #[test]
    fn maps_brave_result_with_extra_snippets() {
        let mapped = map_brave_result(&json!({
            "title": "Example",
            "url": "https://example.com",
            "description": "Snippet",
            "extra_snippets": ["A", "B"],
            "age": "2 days ago"
        }));

        assert_eq!(mapped.title, "Example");
        assert_eq!(mapped.content.as_deref(), Some("A\nB"));
        assert_eq!(mapped.published_at.as_deref(), Some("2 days ago"));
    }

    #[test]
    fn success_output_standardizes_sources() {
        let output = success_output(
            WebSearchEngine::Tavily,
            "query",
            Some("answer".to_string()),
            vec![StandardSearchResult {
                title: "Title".to_string(),
                url: "https://example.com".to_string(),
                snippet: Some("Snippet".to_string()),
                content: None,
                published_at: None,
                score: Some(0.9),
            }],
        );

        assert!(output.success);
        assert_eq!(output.result["engine"], "tavily");
        assert_eq!(output.result["results"][0]["score"], 0.9);
        assert_eq!(output.result["sources"][0]["url"], "https://example.com");
    }
}
