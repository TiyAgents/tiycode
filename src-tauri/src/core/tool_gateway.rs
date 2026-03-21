//! Unified tool execution gateway.
//!
//! All privileged tool requests flow through here:
//! 1. Policy evaluation
//! 2. Optional approval suspension
//! 3. Tool execution
//! 4. Audit persistence

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;
use tiy_core::agent::AbortSignal;
use tokio::sync::{oneshot, Mutex};

use crate::core::executors::{self, ToolOutput};
use crate::core::policy_engine::{PolicyEngine, PolicyVerdict};
use crate::core::terminal_manager::TerminalManager;
use crate::persistence::repo::{audit_repo, settings_repo, tool_call_repo};

/// Request context for a single tool execution.
#[derive(Debug, Clone)]
pub struct ToolExecutionRequest {
    pub run_id: String,
    pub thread_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub workspace_path: String,
    pub run_mode: String,
}

/// Approval payload emitted back to the runtime when user input is required.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub run_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub reason: String,
}

/// Final execution result surfaced back to the runtime.
pub struct ToolGatewayOutcome {
    pub approval_required: bool,
    pub result: ToolGatewayResult,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionOptions {
    pub allow_user_approval: bool,
}

impl Default for ToolExecutionOptions {
    fn default() -> Self {
        Self {
            allow_user_approval: true,
        }
    }
}

/// Result from ToolGateway processing.
pub enum ToolGatewayResult {
    /// Tool was executed successfully (or completed with a structured failure payload).
    Executed {
        tool_call_id: String,
        output: ToolOutput,
    },
    /// Tool was denied by policy or by the user during approval.
    Denied {
        tool_call_id: String,
        reason: String,
    },
    /// Tool would require approval, but the caller requested folded escalation instead.
    EscalationRequired {
        tool_call_id: String,
        reason: String,
    },
    /// Tool was cancelled before approval or execution could finish.
    Cancelled { tool_call_id: String },
}

pub struct ToolGateway {
    pool: SqlitePool,
    policy_engine: PolicyEngine,
    terminal_manager: Arc<TerminalManager>,
    pending_approvals: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl ToolGateway {
    pub fn new(pool: SqlitePool, terminal_manager: Arc<TerminalManager>) -> Self {
        let policy_engine = PolicyEngine::new(pool.clone());
        Self {
            pool,
            policy_engine,
            terminal_manager,
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Evaluate, optionally suspend for approval, and then execute a tool call.
    pub async fn execute_tool_call<FA, FR>(
        &self,
        request: ToolExecutionRequest,
        abort_signal: AbortSignal,
        options: ToolExecutionOptions,
        mut on_approval_required: FA,
        mut on_execution_started: FR,
    ) -> Result<ToolGatewayOutcome, crate::model::errors::AppError>
    where
        FA: FnMut(ApprovalRequest),
        FR: FnMut(),
    {
        let writable_roots = self.load_writable_roots().await?;
        let check = self
            .policy_engine
            .evaluate(
                &request.tool_name,
                &request.tool_input,
                Some(&request.workspace_path),
                &writable_roots,
                &request.run_mode,
            )
            .await?;

        let policy_json = serde_json::to_string(&check).unwrap_or_default();
        let verdict = check.verdict.clone();

        match verdict {
            PolicyVerdict::Deny { reason } => {
                tool_call_repo::update_status(&self.pool, &request.tool_call_id, "denied").await?;

                self.write_audit(
                    &request.run_id,
                    &request.thread_id,
                    &request.tool_call_id,
                    &request.tool_name,
                    "tool_denied",
                    &policy_json,
                    &serde_json::json!({ "reason": reason }).to_string(),
                )
                .await?;

                Ok(ToolGatewayOutcome {
                    approval_required: false,
                    result: ToolGatewayResult::Denied {
                        tool_call_id: request.tool_call_id,
                        reason,
                    },
                })
            }
            PolicyVerdict::AutoAllow => {
                on_execution_started();

                let output = self.execute_and_audit(&request, &policy_json).await?;

                Ok(ToolGatewayOutcome {
                    approval_required: false,
                    result: ToolGatewayResult::Executed {
                        tool_call_id: request.tool_call_id,
                        output,
                    },
                })
            }
            PolicyVerdict::RequireApproval { reason } => {
                if !options.allow_user_approval {
                    tool_call_repo::update_approval(
                        &self.pool,
                        &request.tool_call_id,
                        "escalation_required",
                        "denied",
                    )
                    .await?;

                    self.write_audit(
                        &request.run_id,
                        &request.thread_id,
                        &request.tool_call_id,
                        &request.tool_name,
                        "tool_approval_escalated",
                        &policy_json,
                        &serde_json::json!({ "reason": reason }).to_string(),
                    )
                    .await?;

                    return Ok(ToolGatewayOutcome {
                        approval_required: false,
                        result: ToolGatewayResult::EscalationRequired {
                            tool_call_id: request.tool_call_id,
                            reason,
                        },
                    });
                }

                tool_call_repo::update_status(
                    &self.pool,
                    &request.tool_call_id,
                    "waiting_approval",
                )
                .await?;

                let (approval_tx, approval_rx) = oneshot::channel::<bool>();
                {
                    let mut approvals = self.pending_approvals.lock().await;
                    approvals.insert(request.tool_call_id.clone(), approval_tx);
                }

                on_approval_required(ApprovalRequest {
                    run_id: request.run_id.clone(),
                    tool_call_id: request.tool_call_id.clone(),
                    tool_name: request.tool_name.clone(),
                    tool_input: request.tool_input.clone(),
                    reason: reason.clone(),
                });

                let approval = tokio::select! {
                    _ = abort_signal.cancelled() => None,
                    result = approval_rx => result.ok(),
                };

                {
                    let mut approvals = self.pending_approvals.lock().await;
                    approvals.remove(&request.tool_call_id);
                }

                match approval {
                    Some(true) => {
                        tool_call_repo::update_approval(
                            &self.pool,
                            &request.tool_call_id,
                            "approved",
                            "approved",
                        )
                        .await?;

                        on_execution_started();

                        let output = self.execute_and_audit(&request, &policy_json).await?;

                        Ok(ToolGatewayOutcome {
                            approval_required: true,
                            result: ToolGatewayResult::Executed {
                                tool_call_id: request.tool_call_id,
                                output,
                            },
                        })
                    }
                    Some(false) => {
                        tool_call_repo::update_approval(
                            &self.pool,
                            &request.tool_call_id,
                            "denied",
                            "denied",
                        )
                        .await?;

                        self.write_audit(
                            &request.run_id,
                            &request.thread_id,
                            &request.tool_call_id,
                            &request.tool_name,
                            "tool_approval_denied",
                            &serde_json::to_string(&check).unwrap_or_default(),
                            "{}",
                        )
                        .await?;

                        Ok(ToolGatewayOutcome {
                            approval_required: true,
                            result: ToolGatewayResult::Denied {
                                tool_call_id: request.tool_call_id,
                                reason: "User denied the tool execution".to_string(),
                            },
                        })
                    }
                    None => {
                        tool_call_repo::update_status(
                            &self.pool,
                            &request.tool_call_id,
                            "cancelled",
                        )
                        .await?;

                        self.write_audit(
                            &request.run_id,
                            &request.thread_id,
                            &request.tool_call_id,
                            &request.tool_name,
                            "tool_cancelled",
                            &serde_json::to_string(&check).unwrap_or_default(),
                            "{}",
                        )
                        .await?;

                        Ok(ToolGatewayOutcome {
                            approval_required: true,
                            result: ToolGatewayResult::Cancelled {
                                tool_call_id: request.tool_call_id,
                            },
                        })
                    }
                }
            }
        }
    }

    /// Resolve a pending approval. Returns `true` when a waiter was found.
    pub async fn resolve_approval(
        &self,
        tool_call_id: &str,
        approved: bool,
    ) -> Result<bool, crate::model::errors::AppError> {
        let sender = {
            let mut approvals = self.pending_approvals.lock().await;
            approvals.remove(tool_call_id)
        };

        if let Some(sender) = sender {
            let _ = sender.send(approved);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn execute_and_audit(
        &self,
        request: &ToolExecutionRequest,
        policy_json: &str,
    ) -> Result<ToolOutput, crate::model::errors::AppError> {
        tool_call_repo::update_status(&self.pool, &request.tool_call_id, "running").await?;
        let writable_roots = self.load_writable_roots().await?;

        let output = match executors::execute_tool(
            &request.tool_name,
            &request.tool_input,
            &request.workspace_path,
            &writable_roots,
            &request.thread_id,
            Some(&self.terminal_manager),
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                let message = error.to_string();
                let result_json = serde_json::json!({ "error": message }).to_string();

                tool_call_repo::update_result(
                    &self.pool,
                    &request.tool_call_id,
                    &result_json,
                    "failed",
                )
                .await
                .ok();

                self.write_audit(
                    &request.run_id,
                    &request.thread_id,
                    &request.tool_call_id,
                    &request.tool_name,
                    "tool_failed",
                    policy_json,
                    &result_json,
                )
                .await
                .ok();

                return Err(error);
            }
        };

        let result_json = output.result.to_string();
        let status = if output.success {
            "completed"
        } else {
            "failed"
        };
        tool_call_repo::update_result(&self.pool, &request.tool_call_id, &result_json, status)
            .await?;

        self.write_audit(
            &request.run_id,
            &request.thread_id,
            &request.tool_call_id,
            &request.tool_name,
            &format!("tool_{status}"),
            policy_json,
            &result_json,
        )
        .await?;

        tracing::info!(
            tool_call_id = %request.tool_call_id,
            tool_name = %request.tool_name,
            success = output.success,
            "tool executed"
        );

        Ok(output)
    }

    async fn load_writable_roots(&self) -> Result<Vec<String>, crate::model::errors::AppError> {
        let record = settings_repo::policy_get(&self.pool, "writable_roots").await?;
        Ok(record
            .map(|record| parse_writable_roots(&record.value_json))
            .unwrap_or_default())
    }

    async fn write_audit(
        &self,
        run_id: &str,
        thread_id: &str,
        tool_call_id: &str,
        tool_name: &str,
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

fn parse_writable_roots(value_json: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(value_json).unwrap_or_default();
    parsed
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("path").and_then(serde_json::Value::as_str))
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
