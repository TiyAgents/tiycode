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
use tiycore::agent::AbortSignal;
use tokio::sync::{oneshot, Mutex};

use crate::core::executors::{self, ToolOutput};
use crate::core::policy_engine::{PolicyEngine, PolicyVerdict};
use crate::core::terminal_manager::TerminalManager;
use crate::core::workspace_paths::parse_writable_roots;
use crate::extensions::{ExtensionsManager, ResolvedTool};
use crate::model::thread::MessageRecord;
use crate::persistence::repo::{
    audit_repo, message_repo, settings_repo, thread_repo, tool_call_repo,
};

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
    extensions_manager: Arc<ExtensionsManager>,
    pending_approvals: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    pending_clarifications: Arc<Mutex<HashMap<String, PendingClarification>>>,
}

struct PendingClarification {
    sender: oneshot::Sender<serde_json::Value>,
    thread_id: String,
}

impl ToolGateway {
    pub fn new(pool: SqlitePool, terminal_manager: Arc<TerminalManager>) -> Self {
        let policy_engine = PolicyEngine::new(pool.clone());
        Self {
            pool: pool.clone(),
            policy_engine,
            terminal_manager,
            extensions_manager: Arc::new(ExtensionsManager::new(pool.clone())),
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            pending_clarifications: Arc::new(Mutex::new(HashMap::new())),
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
        let resolved_tool = self
            .extensions_manager
            .resolve_tool(&request.tool_name)
            .await?;
        let provider_context = resolved_tool
            .as_ref()
            .map(ExtensionsManager::provider_context_from_resolved);
        let check = self
            .policy_engine
            .evaluate(
                &request.tool_name,
                &request.tool_input,
                Some(&request.workspace_path),
                &writable_roots,
                &request.run_mode,
                provider_context.as_ref(),
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
                    resolved_tool.as_ref(),
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
                if let Some(reason) = self.run_pre_tool_hooks(&request).await? {
                    return Ok(ToolGatewayOutcome {
                        approval_required: false,
                        result: ToolGatewayResult::Denied {
                            tool_call_id: request.tool_call_id,
                            reason,
                        },
                    });
                }

                on_execution_started();

                let output = self
                    .execute_and_audit(&request, &policy_json, resolved_tool.as_ref())
                    .await?;

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
                        resolved_tool.as_ref(),
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

                        if let Some(reason) = self.run_pre_tool_hooks(&request).await? {
                            return Ok(ToolGatewayOutcome {
                                approval_required: true,
                                result: ToolGatewayResult::Denied {
                                    tool_call_id: request.tool_call_id,
                                    reason,
                                },
                            });
                        }

                        let output = self
                            .execute_and_audit(&request, &policy_json, resolved_tool.as_ref())
                            .await?;

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
                            resolved_tool.as_ref(),
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
                            resolved_tool.as_ref(),
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

    pub async fn request_clarification<FR>(
        &self,
        request: ToolExecutionRequest,
        abort_signal: AbortSignal,
        mut on_input_required: FR,
    ) -> Result<ToolOutput, crate::model::errors::AppError>
    where
        FR: FnMut(),
    {
        tool_call_repo::update_status(&self.pool, &request.tool_call_id, "waiting_clarification")
            .await?;

        self.write_audit(
            &request.run_id,
            &request.thread_id,
            &request.tool_call_id,
            &request.tool_name,
            "tool_clarification_requested",
            "{}",
            &request.tool_input.to_string(),
            None,
        )
        .await?;

        let (response_tx, response_rx) = oneshot::channel::<serde_json::Value>();
        {
            let mut pending = self.pending_clarifications.lock().await;
            pending.insert(
                request.tool_call_id.clone(),
                PendingClarification {
                    sender: response_tx,
                    thread_id: request.thread_id.clone(),
                },
            );
        }

        on_input_required();

        let response = tokio::select! {
            _ = abort_signal.cancelled() => None,
            result = response_rx => result.ok(),
        };

        {
            let mut pending = self.pending_clarifications.lock().await;
            pending.remove(&request.tool_call_id);
        }

        match response {
            Some(response) => {
                tool_call_repo::update_result(
                    &self.pool,
                    &request.tool_call_id,
                    &response.to_string(),
                    "completed",
                )
                .await?;

                self.write_audit(
                    &request.run_id,
                    &request.thread_id,
                    &request.tool_call_id,
                    &request.tool_name,
                    "tool_clarification_resolved",
                    "{}",
                    &response.to_string(),
                    None,
                )
                .await?;

                Ok(ToolOutput {
                    success: true,
                    result: response,
                })
            }
            None => {
                tool_call_repo::update_status(&self.pool, &request.tool_call_id, "cancelled")
                    .await?;

                self.write_audit(
                    &request.run_id,
                    &request.thread_id,
                    &request.tool_call_id,
                    &request.tool_name,
                    "tool_cancelled",
                    "{}",
                    "{}",
                    None,
                )
                .await?;

                Err(crate::model::errors::AppError::recoverable(
                    crate::model::errors::ErrorSource::Tool,
                    "tool.clarification.cancelled",
                    "Clarification request was cancelled before a reply arrived",
                ))
            }
        }
    }

    /// Resolve a pending clarification request. Returns `true` when a waiter was found.
    pub async fn resolve_clarification(
        &self,
        tool_call_id: &str,
        response: serde_json::Value,
    ) -> Result<bool, crate::model::errors::AppError> {
        let pending = {
            let mut pending = self.pending_clarifications.lock().await;
            pending.remove(tool_call_id)
        };

        let Some(pending) = pending else {
            return Ok(false);
        };

        if let Some(response_text) = response
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let message = MessageRecord {
                id: uuid::Uuid::now_v7().to_string(),
                thread_id: pending.thread_id.clone(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: response_text.to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            };

            if let Err(error) = message_repo::insert(&self.pool, &message).await {
                tracing::warn!(
                    tool_call_id = %tool_call_id,
                    error = %error,
                    "failed to persist clarification response message"
                );
            } else if let Err(error) =
                thread_repo::touch_active(&self.pool, &pending.thread_id).await
            {
                tracing::warn!(
                    thread_id = %pending.thread_id,
                    error = %error,
                    "failed to touch thread after clarification response"
                );
            }
        }

        let _ = pending.sender.send(response);
        Ok(true)
    }

    async fn execute_and_audit(
        &self,
        request: &ToolExecutionRequest,
        policy_json: &str,
        resolved_tool: Option<&ResolvedTool>,
    ) -> Result<ToolOutput, crate::model::errors::AppError> {
        tool_call_repo::update_status(&self.pool, &request.tool_call_id, "running").await?;
        let writable_roots = self.load_writable_roots().await?;

        let output = match if let Some(resolved_tool) = resolved_tool {
            self.extensions_manager
                .execute_resolved_tool(
                    resolved_tool,
                    &request.tool_input,
                    &request.workspace_path,
                    &request.thread_id,
                )
                .await
        } else {
            executors::execute_tool(
                &request.tool_name,
                &request.tool_input,
                &request.workspace_path,
                &writable_roots,
                &request.thread_id,
                Some(&self.terminal_manager),
            )
            .await
        } {
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
                    resolved_tool,
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
            resolved_tool,
        )
        .await?;

        self.extensions_manager
            .run_post_tool_hooks(
                &request.tool_name,
                &request.tool_input,
                &output.result,
                &request.workspace_path,
                &request.thread_id,
                &request.run_id,
                &request.tool_call_id,
            )
            .await
            .ok();

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
        resolved_tool: Option<&ResolvedTool>,
    ) -> Result<(), crate::model::errors::AppError> {
        let source = resolved_tool
            .map(|resolved_tool| {
                format!(
                    "{}:{}",
                    resolved_tool.provider_type, resolved_tool.provider_id
                )
            })
            .unwrap_or_else(|| "tool".to_string());
        let target_type = resolved_tool
            .map(|resolved_tool| resolved_tool.provider_type.clone())
            .unwrap_or_else(|| "tool".to_string());
        let target_id = resolved_tool
            .map(|resolved_tool| resolved_tool.provider_id.clone())
            .unwrap_or_else(|| tool_name.to_string());
        audit_repo::insert(
            &self.pool,
            &audit_repo::AuditInsert {
                actor_type: "agent".to_string(),
                actor_id: Some(run_id.to_string()),
                source,
                workspace_id: None,
                thread_id: Some(thread_id.to_string()),
                run_id: Some(run_id.to_string()),
                tool_call_id: Some(tool_call_id.to_string()),
                action: action.to_string(),
                target_type: Some(target_type),
                target_id: Some(target_id),
                policy_check_json: Some(policy_json.to_string()),
                result_json: Some(result_json.to_string()),
            },
        )
        .await
    }

    async fn run_pre_tool_hooks(
        &self,
        request: &ToolExecutionRequest,
    ) -> Result<Option<String>, crate::model::errors::AppError> {
        self.extensions_manager
            .run_pre_tool_hooks(
                &request.tool_name,
                &request.tool_input,
                &request.workspace_path,
                &request.thread_id,
                &request.run_id,
                &request.tool_call_id,
            )
            .await
    }
}
