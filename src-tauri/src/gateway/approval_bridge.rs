//! Approval bridge: routes ToolGateway approval requests through IM.
//!
//! When an agent run encounters an `ApprovalRequired` event, this module
//! formats an approval request message for the IM user and provides helpers
//! to parse the user's Y/N response and resolve the pending approval.

use std::sync::Arc;

use crate::core::tool_gateway::ToolGateway;

/// Format an approval request message for display in IM.
pub fn format_approval_request(tool_name: &str, tool_input: &str) -> String {
    let truncated_input = truncate_input(tool_input, 300);
    format!(
        "⚠️ 工具审批请求\n\n\
         工具: `{tool_name}`\n\
         输入:\n{truncated_input}\n\n\
         回复 Y 批准 / N 拒绝"
    )
}

/// Format a plan approval request message for display in IM.
pub fn format_plan_approval_request(title: &str) -> String {
    format!(
        "📋 实施计划待审批\n\n\
         计划: {title}\n\n\
         回复 Y 批准 / N 拒绝"
    )
}

/// Parse a user's approval response.
/// Returns `Some(true)` for approval, `Some(false)` for rejection, `None` for unrecognized.
pub fn parse_approval_response(text: &str) -> Option<bool> {
    let trimmed = text.trim().to_lowercase();
    match trimmed.as_str() {
        "y" | "yes" | "是" | "批准" | "approve" | "ok" | "确认" => Some(true),
        "n" | "no" | "否" | "拒绝" | "reject" | "deny" | "取消" => Some(false),
        _ => None,
    }
}

/// Resolve a pending tool approval in the ToolGateway.
///
/// Returns `true` if a matching pending approval was found and resolved.
pub async fn resolve(
    tool_gateway: &Arc<ToolGateway>,
    tool_call_id: &str,
    approved: bool,
) -> anyhow::Result<bool> {
    tool_gateway
        .resolve_approval(tool_call_id, approved)
        .await
        .map_err(|e| anyhow::anyhow!("failed to resolve approval: {e}"))
}

/// Truncate tool input for display, preserving readability.
fn truncate_input(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let truncated: String = input.chars().take(max_chars).collect();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_approval_yes() {
        assert_eq!(parse_approval_response("Y"), Some(true));
        assert_eq!(parse_approval_response("yes"), Some(true));
        assert_eq!(parse_approval_response("是"), Some(true));
        assert_eq!(parse_approval_response("  ok  "), Some(true));
    }

    #[test]
    fn parse_approval_no() {
        assert_eq!(parse_approval_response("N"), Some(false));
        assert_eq!(parse_approval_response("no"), Some(false));
        assert_eq!(parse_approval_response("拒绝"), Some(false));
    }

    #[test]
    fn parse_approval_unrecognized() {
        assert_eq!(parse_approval_response("maybe"), None);
        assert_eq!(parse_approval_response("hello"), None);
        assert_eq!(parse_approval_response(""), None);
    }

    #[test]
    fn truncate_short_input() {
        assert_eq!(truncate_input("short", 100), "short");
    }

    #[test]
    fn truncate_long_input() {
        let long = "a".repeat(500);
        let result = truncate_input(&long, 300);
        assert!(result.ends_with("..."));
        assert!(result.len() < 310);
    }

    #[test]
    fn format_plan_approval_request_contains_title() {
        let msg = format_plan_approval_request("Refactor runtime");
        assert!(msg.contains("📋"));
        assert!(msg.contains("Refactor runtime"));
        assert!(msg.contains("Y"));
        assert!(msg.contains("N"));
    }
}
