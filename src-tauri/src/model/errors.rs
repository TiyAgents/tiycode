use serde::Serialize;

/// Unified error type for all cross-layer propagation.
///
/// Format: `<source>.<kind>.<detail>`
/// Examples: `tool.policy.denied`, `git.remote.auth_failed`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub error_code: String,
    pub category: ErrorCategory,
    pub source: ErrorSource,
    pub user_message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorCategory {
    Fatal,
    Recoverable,
    Informational,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSource {
    Thread,
    Tool,
    Git,
    Terminal,
    Index,
    Settings,
    Workspace,
    Database,
    System,
}

impl AppError {
    pub fn internal(source: ErrorSource, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            error_code: format!("{}._internal", source.as_str()),
            category: ErrorCategory::Fatal,
            source,
            user_message: message.clone(),
            detail: Some(message),
            retryable: false,
        }
    }

    pub fn recoverable(
        source: ErrorSource,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            error_code: code.into(),
            category: ErrorCategory::Recoverable,
            source,
            user_message: message.into(),
            detail: None,
            retryable: true,
        }
    }

    pub fn not_found(source: ErrorSource, what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            error_code: format!("{}.not_found", source.as_str()),
            category: ErrorCategory::Recoverable,
            source,
            user_message: format!("{what} not found"),
            detail: None,
            retryable: false,
        }
    }
}

impl ErrorSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Thread => "thread",
            Self::Tool => "tool",
            Self::Git => "git",
            Self::Terminal => "terminal",
            Self::Index => "index",
            Self::Settings => "settings",
            Self::Workspace => "workspace",
            Self::Database => "database",
            Self::System => "system",
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.error_code, self.user_message)
    }
}

impl std::error::Error for AppError {}

// Allow sqlx errors to convert into AppError
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::internal(ErrorSource::Database, err.to_string())
    }
}

// Allow anyhow errors to convert into AppError
impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::internal(ErrorSource::System, err.to_string())
    }
}

// Allow std::io errors to convert into AppError
impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::internal(ErrorSource::System, err.to_string())
    }
}
