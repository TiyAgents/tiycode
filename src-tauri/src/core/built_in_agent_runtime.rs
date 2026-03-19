use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::{mpsc, Mutex};

use crate::core::agent_session::{AgentSession, AgentSessionSpec};
use crate::core::subagent::HelperAgentOrchestrator;
use crate::core::tool_gateway::ToolGateway;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::AppError;

pub struct BuiltInAgentRuntime {
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    helper_orchestrator: Arc<HelperAgentOrchestrator>,
    sessions: Arc<Mutex<HashMap<String, Arc<AgentSession>>>>,
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

    pub async fn start_session(
        &self,
        spec: AgentSessionSpec,
        event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
    ) -> Result<(), AppError> {
        let session = AgentSession::new(
            self.pool.clone(),
            Arc::clone(&self.tool_gateway),
            Arc::clone(&self.helper_orchestrator),
            event_tx,
            spec.clone(),
        );

        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(spec.run_id.clone(), Arc::clone(&session));
        }

        session.start();
        Ok(())
    }

    pub async fn cancel_session(&self, run_id: &str) -> Result<bool, AppError> {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(run_id).cloned()
        };

        if let Some(session) = session {
            session.cancel().await;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn remove_session(&self, run_id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(run_id);
    }
}
