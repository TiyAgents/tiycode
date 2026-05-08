use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::{mpsc, watch, Mutex};

use crate::core::agent_session::{AgentSession, AgentSessionSpec};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeSessionFinishState {
    Running,
    Completed,
    Panicked,
    Cancelled,
}

pub struct BuiltInAgentRuntime {
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    helper_orchestrator: Arc<HelperAgentOrchestrator>,
    sessions: Arc<Mutex<HashMap<String, RuntimeSessionEntry>>>,
}

impl BuiltInAgentRuntime {
    pub fn new(pool: SqlitePool, tool_gateway: Arc<ToolGateway>) -> Self {
        Self {
            helper_orchestrator: Arc::new(HelperAgentOrchestrator::new(
                pool.clone(),
                Arc::clone(&tool_gateway),
            )),
            pool,
            tool_gateway,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn start_session(
        &self,
        spec: AgentSessionSpec,
        event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
        active_runs: Arc<
            tokio::sync::Mutex<
                std::collections::HashMap<String, crate::core::agent_run_manager::ActiveRun>,
            >,
        >,
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
            active_runs,
        );
        let run_task = Arc::clone(&session).start();
        let (finish_state_tx, finish_state_rx) = watch::channel(RuntimeSessionFinishState::Running);

        tokio::spawn(async move {
            let finish_state = match run_task.await {
                Ok(()) => RuntimeSessionFinishState::Completed,
                Err(error) if error.is_cancelled() => RuntimeSessionFinishState::Cancelled,
                Err(_) => RuntimeSessionFinishState::Panicked,
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

    pub(crate) async fn session_state(&self, run_id: &str) -> RuntimeSessionState {
        match self.session_finish_state(run_id).await {
            None => RuntimeSessionState::Missing,
            Some(RuntimeSessionFinishState::Running) => RuntimeSessionState::Running,
            Some(
                RuntimeSessionFinishState::Completed
                | RuntimeSessionFinishState::Panicked
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
            .map(|entry| *entry.finish_state_rx.borrow())
    }

    pub async fn remove_session(&self, run_id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(run_id);
    }
}
