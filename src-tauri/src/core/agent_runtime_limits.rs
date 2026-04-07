use sqlx::SqlitePool;

use crate::persistence::repo::settings_repo;

pub const DESKTOP_AGENT_MAX_TURNS_SETTING_KEY: &str = "agent.runtime.max_turns";
pub const DEFAULT_DESKTOP_AGENT_MAX_TURNS: usize = 4096;

pub async fn desktop_agent_max_turns(pool: &SqlitePool) -> usize {
    match settings_repo::get(pool, DESKTOP_AGENT_MAX_TURNS_SETTING_KEY).await {
        Ok(Some(record)) => parse_desktop_agent_max_turns_value(&record.value_json),
        Ok(None) => DEFAULT_DESKTOP_AGENT_MAX_TURNS,
        Err(error) => {
            tracing::warn!(
                setting = DESKTOP_AGENT_MAX_TURNS_SETTING_KEY,
                error = %error,
                fallback = DEFAULT_DESKTOP_AGENT_MAX_TURNS,
                "failed to load desktop agent max turns setting"
            );
            DEFAULT_DESKTOP_AGENT_MAX_TURNS
        }
    }
}

fn parse_desktop_agent_max_turns_value(value_json: &str) -> usize {
    match serde_json::from_str::<usize>(value_json) {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            tracing::warn!(
                setting = DESKTOP_AGENT_MAX_TURNS_SETTING_KEY,
                fallback = DEFAULT_DESKTOP_AGENT_MAX_TURNS,
                "desktop agent max turns must be greater than zero"
            );
            DEFAULT_DESKTOP_AGENT_MAX_TURNS
        }
        Err(error) => {
            tracing::warn!(
                setting = DESKTOP_AGENT_MAX_TURNS_SETTING_KEY,
                error = %error,
                fallback = DEFAULT_DESKTOP_AGENT_MAX_TURNS,
                "failed to parse desktop agent max turns setting"
            );
            DEFAULT_DESKTOP_AGENT_MAX_TURNS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_desktop_agent_max_turns_value, DEFAULT_DESKTOP_AGENT_MAX_TURNS};

    #[test]
    fn desktop_agent_max_turns_defaults_to_4096() {
        assert_eq!(DEFAULT_DESKTOP_AGENT_MAX_TURNS, 4096);
    }

    #[test]
    fn parse_desktop_agent_max_turns_accepts_positive_integer() {
        assert_eq!(parse_desktop_agent_max_turns_value("1234"), 1234);
    }

    #[test]
    fn parse_desktop_agent_max_turns_rejects_zero() {
        assert_eq!(
            parse_desktop_agent_max_turns_value("0"),
            DEFAULT_DESKTOP_AGENT_MAX_TURNS
        );
    }

    #[test]
    fn parse_desktop_agent_max_turns_rejects_invalid_json() {
        assert_eq!(
            parse_desktop_agent_max_turns_value("\"abc\""),
            DEFAULT_DESKTOP_AGENT_MAX_TURNS
        );
    }
}
