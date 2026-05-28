//! Gateway command router — placeholder for Step 3.

// TODO(step-3): Implement GatewayCommand enum and parse() function.

/// Commands recognized by the gateway.
#[derive(Debug, Clone, PartialEq)]
pub enum GatewayCommand {
    WorkspaceList,
    WorkspaceAdd { path: String, name: Option<String> },
    WorkspaceSwitch { index: usize },
    ThreadList,
    ThreadNew { title: Option<String> },
    ThreadResume { index: usize },
    Stop,
    Status,
    Help,
    PlainText(String),
}

/// Parse a raw message text into a gateway command.
pub fn parse(text: &str) -> GatewayCommand {
    let trimmed = text.trim();

    if trimmed.eq_ignore_ascii_case("/help") {
        return GatewayCommand::Help;
    }
    if trimmed.eq_ignore_ascii_case("/stop") {
        return GatewayCommand::Stop;
    }
    if trimmed.eq_ignore_ascii_case("/status") {
        return GatewayCommand::Status;
    }
    if trimmed.eq_ignore_ascii_case("/ws") || trimmed.eq_ignore_ascii_case("/workspaces") {
        return GatewayCommand::WorkspaceList;
    }
    if trimmed.eq_ignore_ascii_case("/threads") || trimmed.eq_ignore_ascii_case("/sessions") {
        return GatewayCommand::ThreadList;
    }

    // /ws add <path> [name]
    if let Some(rest) = trimmed
        .strip_prefix("/ws add ")
        .or_else(|| trimmed.strip_prefix("/ws add\t"))
    {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let path = parts[0].to_string();
        let name = parts.get(1).map(|s| s.to_string());
        return GatewayCommand::WorkspaceAdd { path, name };
    }

    // /ws <N>
    if let Some(rest) = trimmed.strip_prefix("/ws ") {
        if let Ok(index) = rest.trim().parse::<usize>() {
            return GatewayCommand::WorkspaceSwitch { index };
        }
    }

    // /new [title]
    if trimmed.eq_ignore_ascii_case("/new") {
        return GatewayCommand::ThreadNew { title: None };
    }
    if let Some(rest) = trimmed.strip_prefix("/new ") {
        return GatewayCommand::ThreadNew {
            title: Some(rest.trim().to_string()),
        };
    }

    // /resume <N>
    if let Some(rest) = trimmed.strip_prefix("/resume ") {
        if let Ok(index) = rest.trim().parse::<usize>() {
            return GatewayCommand::ThreadResume { index };
        }
    }

    GatewayCommand::PlainText(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_workspace_commands() {
        assert_eq!(parse("/ws"), GatewayCommand::WorkspaceList);
        assert_eq!(parse("/workspaces"), GatewayCommand::WorkspaceList);
        assert_eq!(
            parse("/ws add /tmp/project"),
            GatewayCommand::WorkspaceAdd {
                path: "/tmp/project".to_string(),
                name: None,
            }
        );
        assert_eq!(
            parse("/ws add /tmp/project My Project"),
            GatewayCommand::WorkspaceAdd {
                path: "/tmp/project".to_string(),
                name: Some("My Project".to_string()),
            }
        );
        assert_eq!(parse("/ws 2"), GatewayCommand::WorkspaceSwitch { index: 2 });
    }

    #[test]
    fn parse_thread_commands() {
        assert_eq!(parse("/threads"), GatewayCommand::ThreadList);
        assert_eq!(parse("/new"), GatewayCommand::ThreadNew { title: None });
        assert_eq!(
            parse("/new Fix login bug"),
            GatewayCommand::ThreadNew {
                title: Some("Fix login bug".to_string()),
            }
        );
        assert_eq!(
            parse("/resume 3"),
            GatewayCommand::ThreadResume { index: 3 }
        );
    }

    #[test]
    fn parse_control_commands() {
        assert_eq!(parse("/stop"), GatewayCommand::Stop);
        assert_eq!(parse("/status"), GatewayCommand::Status);
        assert_eq!(parse("/help"), GatewayCommand::Help);
    }

    #[test]
    fn parse_plain_text() {
        assert_eq!(
            parse("hello world"),
            GatewayCommand::PlainText("hello world".to_string())
        );
        assert_eq!(
            parse("  fix the bug  "),
            GatewayCommand::PlainText("fix the bug".to_string())
        );
    }
}
