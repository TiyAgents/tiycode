use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use sqlx::SqlitePool;
use tokio::sync::{mpsc, watch, Mutex};

use crate::core::agent_session::{AgentSession, AgentSessionSpec};
use crate::core::agent_session_types::{AgentQueueMessageKind, RuntimeQueueSnapshotDto};
use crate::core::app_state::GoalRuntimeState;
use crate::core::subagent::HelperAgentOrchestrator;
use crate::core::tool_gateway::ToolGateway;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::AppError;

struct RuntimeSessionEntry {
    session: Arc<AgentSession>,
    finish_state_rx: watch::Receiver<RuntimeSessionFinishState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeSessionState {
    Missing,
    Running,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeSessionFinishState {
    Running,
    Completed,
    Panicked(String),
    Cancelled,
}

pub struct BuiltInAgentRuntime {
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    helper_orchestrator: Arc<HelperAgentOrchestrator>,
    sessions: Arc<Mutex<HashMap<String, RuntimeSessionEntry>>>,
    goal_runtime: Arc<StdMutex<GoalRuntimeState>>,
}

impl BuiltInAgentRuntime {
    pub fn new(
        pool: SqlitePool,
        tool_gateway: Arc<ToolGateway>,
        goal_runtime: Arc<StdMutex<GoalRuntimeState>>,
    ) -> Self {
        Self {
            helper_orchestrator: Arc::new(HelperAgentOrchestrator::new(
                pool.clone(),
                Arc::clone(&tool_gateway),
            )),
            pool,
            tool_gateway,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            goal_runtime,
        }
    }

    pub(crate) async fn start_session(
        &self,
        spec: AgentSessionSpec,
        event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
    ) -> Result<watch::Receiver<RuntimeSessionFinishState>, AppError> {
        let max_turns =
            crate::core::agent_runtime_limits::desktop_agent_max_turns(&self.pool).await;
        let session = AgentSession::new(
            self.pool.clone(),
            Arc::clone(&self.tool_gateway),
            Arc::clone(&self.helper_orchestrator),
            event_tx,
            spec.clone(),
            max_turns,
            Arc::clone(&self.goal_runtime),
        );
        let run_task = Arc::clone(&session).start();
        let (finish_state_tx, finish_state_rx) = watch::channel(RuntimeSessionFinishState::Running);

        tokio::spawn(async move {
            let finish_state = match run_task.await {
                Ok(()) => RuntimeSessionFinishState::Completed,
                Err(error) if error.is_cancelled() => RuntimeSessionFinishState::Cancelled,
                Err(error) if error.is_panic() => RuntimeSessionFinishState::Panicked(format!(
                    "Runtime task panicked: {}",
                    panic_payload_to_string(error.into_panic())
                )),
                Err(error) => RuntimeSessionFinishState::Panicked(format!(
                    "Runtime task failed to join: {error}"
                )),
            };
            let _ = finish_state_tx.send(finish_state);
        });

        let mut sessions = self.sessions.lock().await;
        sessions.insert(
            spec.run_id.clone(),
            RuntimeSessionEntry {
                session,
                finish_state_rx: finish_state_rx.clone(),
            },
        );

        Ok(finish_state_rx)
    }

    pub async fn cancel_session(&self, run_id: &str) -> Result<bool, AppError> {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(run_id).map(|entry| Arc::clone(&entry.session))
        };

        if let Some(session) = session {
            session.cancel().await;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn enqueue_queue_message(
        &self,
        run_id: &str,
        kind: AgentQueueMessageKind,
        message: String,
        metadata: Option<serde_json::Value>,
    ) -> Result<Option<RuntimeQueueSnapshotDto>, AppError> {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(run_id).map(|entry| Arc::clone(&entry.session))
        };

        session
            .map(|session| session.enqueue_queue_message(kind, message, metadata))
            .transpose()
    }

    pub async fn runtime_queue_snapshot(&self, run_id: &str) -> Option<RuntimeQueueSnapshotDto> {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(run_id).map(|entry| Arc::clone(&entry.session))
        };

        session.map(|session| session.runtime_queue_snapshot())
    }

    pub async fn clear_runtime_queue(
        &self,
        run_id: &str,
        kind: Option<AgentQueueMessageKind>,
    ) -> Option<RuntimeQueueSnapshotDto> {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(run_id).map(|entry| Arc::clone(&entry.session))
        };

        session.map(|session| session.clear_runtime_queue(kind))
    }

    pub async fn cancel_runtime_queue_message(
        &self,
        run_id: &str,
        message_id: &str,
    ) -> Result<Option<RuntimeQueueSnapshotDto>, AppError> {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(run_id).map(|entry| Arc::clone(&entry.session))
        };

        session
            .map(|session| session.cancel_runtime_queue_message(message_id))
            .transpose()
    }

    pub async fn promote_runtime_queue_message(
        &self,
        run_id: &str,
        message_id: &str,
    ) -> Result<Option<RuntimeQueueSnapshotDto>, AppError> {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(run_id).map(|entry| Arc::clone(&entry.session))
        };

        session
            .map(|session| session.promote_runtime_queue_message(message_id))
            .transpose()
    }

    pub(crate) async fn session_state(&self, run_id: &str) -> RuntimeSessionState {
        match self.session_finish_state(run_id).await {
            None => RuntimeSessionState::Missing,
            Some(RuntimeSessionFinishState::Running) => RuntimeSessionState::Running,
            Some(
                RuntimeSessionFinishState::Completed
                | RuntimeSessionFinishState::Panicked(_)
                | RuntimeSessionFinishState::Cancelled,
            ) => RuntimeSessionState::Finished,
        }
    }

    pub(crate) async fn session_finish_state(
        &self,
        run_id: &str,
    ) -> Option<RuntimeSessionFinishState> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(run_id)
            .map(|entry| entry.finish_state_rx.borrow().clone())
    }

    pub async fn remove_session(&self, run_id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(run_id);
    }
}

fn panic_payload_to_string(payload: Box<dyn Any + Send + 'static>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_string(),
            Err(_) => "non-string panic payload".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::panic_payload_to_string;

    #[test]
    fn panic_payload_to_string_extracts_string_payload() {
        assert_eq!(
            panic_payload_to_string(Box::new("boom".to_string())),
            "boom"
        );
    }

    #[test]
    fn panic_payload_to_string_extracts_static_str_payload() {
        assert_eq!(panic_payload_to_string(Box::new("boom")), "boom");
    }

    #[test]
    fn panic_payload_to_string_handles_non_string_payload() {
        assert_eq!(
            panic_payload_to_string(Box::new(42usize)),
            "non-string panic payload"
        );
    }
}
