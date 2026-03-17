//! Unified tool execution gateway.
//!
//! All privileged tool requests flow through here:
//! 1. Receive tool request (from sidecar via AgentRunManager)
//! 2. Evaluate with PolicyEngine
//! 3. If auto-allow → execute immediately
//! 4. If require-approval → pause and notify frontend
//! 5. If deny → reject immediately
//! 6. Execute tool via appropriate executor
//! 7. Write audit record
//! 8. Return result

use std::sync::Arc;
use tokio::sync::Mutex;

use sqlx::SqlitePool;

use crate::core::executors::{self, ToolOutput};
use crate::core::policy_engine::{PolicyCheck, PolicyEngine, PolicyVerdict};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::persistence::repo::{audit_repo, tool_call_repo};

/// Pending approval state.
struct PendingApproval {
    run_id: String,
    thread_id: String,
    tool_call_id: String,
    tool_name: String,
    tool_input: serde_json::Value,
    workspace_path: String,
    policy_check: PolicyCheck,
}

pub struct ToolGateway {
    pool: SqlitePool,
    policy_engine: PolicyEngine,
    pending_approvals: Arc<Mutex<Vec<PendingApproval>>>,
}

impl ToolGateway {
    pub fn new(pool: SqlitePool) -> Self {
        let policy_engine = PolicyEngine::new(pool.clone());
        Self {
            pool,
            policy_engine,
            pending_approvals: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Process a tool request from the sidecar.
    ///
    /// Returns:
    /// - `Ok(Some(event))` — an event to emit to the frontend (approval_required or tool result)
    /// - `Ok(None)` — tool was auto-allowed and result was sent back directly
    pub async fn handle_tool_request(
        &self,
        run_id: &str,
        thread_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
        workspace_path: &str,
        run_mode: &str,
    ) -> Result<ToolGatewayResult, crate::model::errors::AppError> {
        // 1. Policy evaluation
        let check = self
            .policy_engine
            .evaluate(tool_name, tool_input, Some(workspace_path), run_mode)
            .await?;

        let policy_json = serde_json::to_string(&check).unwrap_or_default();
        let verdict = check.verdict.clone();

        match &verdict {
            PolicyVerdict::Deny { reason } => {
                // Update tool call status
                tool_call_repo::update_status(&self.pool, tool_call_id, "denied").await?;

                // Audit
                self.write_audit(
                    run_id,
                    thread_id,
                    tool_call_id,
                    tool_name,
                    workspace_path,
                    "tool_denied",
                    &policy_json,
                    &serde_json::json!({"reason": reason}).to_string(),
                )
                .await?;

                Ok(ToolGatewayResult::Denied {
                    tool_call_id: tool_call_id.to_string(),
                    reason: reason.clone(),
                })
            }

            PolicyVerdict::RequireApproval { reason } => {
                tool_call_repo::update_status(&self.pool, tool_call_id, "waiting_approval").await?;

                // Store pending approval
                {
                    let mut pending = self.pending_approvals.lock().await;
                    pending.push(PendingApproval {
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        tool_call_id: tool_call_id.to_string(),
                        tool_name: tool_name.to_string(),
                        tool_input: tool_input.clone(),
                        workspace_path: workspace_path.to_string(),
                        policy_check: check,
                    });
                }

                Ok(ToolGatewayResult::ApprovalRequired {
                    event: ThreadStreamEvent::ApprovalRequired {
                        run_id: run_id.to_string(),
                        tool_call_id: tool_call_id.to_string(),
                        tool_name: tool_name.to_string(),
                        tool_input: tool_input.clone(),
                        reason: reason.clone(),
                    },
                })
            }

            PolicyVerdict::AutoAllow => {
                tool_call_repo::update_status(&self.pool, tool_call_id, "approved").await?;

                // Execute
                let output = self
                    .execute_and_audit(
                        run_id,
                        thread_id,
                        tool_call_id,
                        tool_name,
                        tool_input,
                        workspace_path,
                        &policy_json,
                    )
                    .await?;

                Ok(ToolGatewayResult::Executed {
                    tool_call_id: tool_call_id.to_string(),
                    output,
                })
            }
        }
    }

    /// Resolve a pending approval.
    pub async fn resolve_approval(
        &self,
        tool_call_id: &str,
        approved: bool,
    ) -> Result<Option<ToolGatewayResult>, crate::model::errors::AppError> {
        // Find and remove pending approval
        let pending = {
            let mut approvals = self.pending_approvals.lock().await;
            let idx = approvals
                .iter()
                .position(|p| p.tool_call_id == tool_call_id);
            idx.map(|i| approvals.remove(i))
        };

        let pending = match pending {
            Some(p) => p,
            None => return Ok(None),
        };

        if !approved {
            tool_call_repo::update_approval(&self.pool, tool_call_id, "denied", "denied").await?;

            self.write_audit(
                &pending.run_id,
                &pending.thread_id,
                tool_call_id,
                &pending.tool_name,
                &pending.workspace_path,
                "tool_approval_denied",
                &serde_json::to_string(&pending.policy_check).unwrap_or_default(),
                "{}",
            )
            .await?;

            return Ok(Some(ToolGatewayResult::Denied {
                tool_call_id: tool_call_id.to_string(),
                reason: "User denied the tool execution".to_string(),
            }));
        }

        // Approved — execute
        tool_call_repo::update_approval(&self.pool, tool_call_id, "approved", "approved").await?;

        let policy_json = serde_json::to_string(&pending.policy_check).unwrap_or_default();

        let output = self
            .execute_and_audit(
                &pending.run_id,
                &pending.thread_id,
                tool_call_id,
                &pending.tool_name,
                &pending.tool_input,
                &pending.workspace_path,
                &policy_json,
            )
            .await?;

        Ok(Some(ToolGatewayResult::Executed {
            tool_call_id: tool_call_id.to_string(),
            output,
        }))
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    async fn execute_and_audit(
        &self,
        run_id: &str,
        thread_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
        workspace_path: &str,
        policy_json: &str,
    ) -> Result<ToolOutput, crate::model::errors::AppError> {
        tool_call_repo::update_status(&self.pool, tool_call_id, "running").await?;

        let output = executors::execute_tool(tool_name, tool_input, workspace_path).await?;

        // Persist result
        let result_json = output.result.to_string();
        let status = if output.success {
            "completed"
        } else {
            "failed"
        };
        tool_call_repo::update_result(&self.pool, tool_call_id, &result_json, status).await?;

        // Audit
        self.write_audit(
            run_id,
            thread_id,
            tool_call_id,
            tool_name,
            workspace_path,
            &format!("tool_{status}"),
            policy_json,
            &result_json,
        )
        .await?;

        tracing::info!(
            tool_call_id,
            tool_name,
            success = output.success,
            "tool executed"
        );

        Ok(output)
    }

    async fn write_audit(
        &self,
        run_id: &str,
        thread_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        workspace_path: &str,
        action: &str,
        policy_json: &str,
        result_json: &str,
    ) -> Result<(), crate::model::errors::AppError> {
        audit_repo::insert(
            &self.pool,
            &audit_repo::AuditInsert {
                actor_type: "agent".to_string(),
                actor_id: Some(run_id.to_string()),
                source: "tool".to_string(),
                workspace_id: None,
                thread_id: Some(thread_id.to_string()),
                run_id: Some(run_id.to_string()),
                tool_call_id: Some(tool_call_id.to_string()),
                action: action.to_string(),
                target_type: Some("tool".to_string()),
                target_id: Some(tool_name.to_string()),
                policy_check_json: Some(policy_json.to_string()),
                result_json: Some(result_json.to_string()),
            },
        )
        .await
    }
}

/// Result from ToolGateway processing.
pub enum ToolGatewayResult {
    /// Tool was executed (auto-allow or post-approval).
    Executed {
        tool_call_id: String,
        output: ToolOutput,
    },
    /// Tool requires user approval before execution.
    ApprovalRequired { event: ThreadStreamEvent },
    /// Tool was denied by policy or user.
    Denied {
        tool_call_id: String,
        reason: String,
    },
}
