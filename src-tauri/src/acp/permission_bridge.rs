use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    SelectedPermissionOutcome, SessionId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Client, ConnectionTo};

use crate::core::tool_gateway::ToolGateway;
use crate::ipc::frontend_channels::ThreadStreamEvent;

const ALLOW_ONCE_OPTION_ID: &str = "allow_once";
const REJECT_ONCE_OPTION_ID: &str = "reject_once";
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn request_permission_and_resolve(
    connection: &ConnectionTo<Client>,
    tool_gateway: Arc<ToolGateway>,
    session_id: &SessionId,
    event: &ThreadStreamEvent,
) -> Result<(), agent_client_protocol::Error> {
    let ThreadStreamEvent::ApprovalRequired {
        tool_call_id,
        tool_name,
        reason,
        ..
    } = event
    else {
        return Ok(());
    };

    let approved =
        request_permission(connection, session_id, tool_call_id, tool_name, reason).await?;
    match tool_gateway.resolve_approval(tool_call_id, approved).await {
        Ok(true) => Ok(()),
        Ok(false) => {
            tracing::warn!(
                tool_call_id = %tool_call_id,
                approved,
                "ACP permission response did not match a pending tool approval"
            );
            Ok(())
        }
        Err(error) => Err(agent_client_protocol::util::internal_error(format!(
            "failed to resolve ACP permission response: {error}"
        ))),
    }
}

async fn request_permission(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    tool_name: &str,
    reason: &str,
) -> Result<bool, agent_client_protocol::Error> {
    let request = RequestPermissionRequest::new(
        session_id.clone(),
        ToolCallUpdate::new(
            tool_call_id.to_string(),
            ToolCallUpdateFields::new()
                .title(Some(format!(
                    "Approve {}: {reason}",
                    tool_name.replace('_', " ")
                )))
                .status(Some(ToolCallStatus::Pending)),
        ),
        vec![
            PermissionOption::new(
                ALLOW_ONCE_OPTION_ID,
                "Allow once",
                PermissionOptionKind::AllowOnce,
            ),
            PermissionOption::new(
                REJECT_ONCE_OPTION_ID,
                "Reject once",
                PermissionOptionKind::RejectOnce,
            ),
        ],
    );

    let response = match tokio::time::timeout(
        PERMISSION_TIMEOUT,
        connection.send_request(request).block_task(),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            tracing::warn!(
                tool_call_id,
                tool_name,
                timeout_ms = PERMISSION_TIMEOUT.as_millis() as u64,
                "ACP permission request timed out; rejecting tool call"
            );
            return Ok(false);
        }
    };

    Ok(match response.outcome {
        RequestPermissionOutcome::Cancelled => false,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            matches!(option_id.0.as_ref(), ALLOW_ONCE_OPTION_ID)
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_option_ids_match_expected_policy() {
        assert_eq!(ALLOW_ONCE_OPTION_ID, "allow_once");
        assert_eq!(REJECT_ONCE_OPTION_ID, "reject_once");
    }
}
