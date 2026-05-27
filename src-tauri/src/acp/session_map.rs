use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::schema::{SessionId, SessionInfo};
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpSessionRecord {
    pub session_id: String,
    pub thread_id: String,
    pub workspace_id: String,
    pub cwd: PathBuf,
    pub profile_id: Option<String>,
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

impl AcpSessionRecord {
    pub fn new(
        session_id: impl Into<String>,
        thread_id: impl Into<String>,
        workspace_id: impl Into<String>,
        cwd: impl Into<PathBuf>,
        profile_id: Option<String>,
        title: Option<String>,
        updated_at: Option<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            thread_id: thread_id.into(),
            workspace_id: workspace_id.into(),
            cwd: cwd.into(),
            profile_id,
            title,
            updated_at,
        }
    }

    pub fn to_session_info(&self) -> SessionInfo {
        SessionInfo::new(SessionId::new(self.session_id.clone()), self.cwd.clone())
            .title(self.title.clone())
            .updated_at(self.updated_at.clone())
    }
}

#[derive(Debug, Default)]
pub struct AcpSessionMap {
    inner: RwLock<AcpSessionMapInner>,
}

#[derive(Debug, Default)]
struct AcpSessionMapInner {
    by_session_id: HashMap<String, AcpSessionRecord>,
    session_by_thread_id: HashMap<String, String>,
}

impl AcpSessionMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, record: AcpSessionRecord) {
        let mut inner = self.inner.write().await;
        if let Some(existing) = inner
            .session_by_thread_id
            .insert(record.thread_id.clone(), record.session_id.clone())
        {
            inner.by_session_id.remove(&existing);
        }
        inner
            .by_session_id
            .insert(record.session_id.clone(), record);
    }

    pub async fn get(&self, session_id: &SessionId) -> Option<AcpSessionRecord> {
        let inner = self.inner.read().await;
        inner.by_session_id.get(session_id.0.as_ref()).cloned()
    }

    pub async fn get_by_str(&self, session_id: &str) -> Option<AcpSessionRecord> {
        let inner = self.inner.read().await;
        inner.by_session_id.get(session_id).cloned()
    }

    pub async fn session_for_thread(&self, thread_id: &str) -> Option<AcpSessionRecord> {
        let inner = self.inner.read().await;
        let session_id = inner.session_by_thread_id.get(thread_id)?;
        inner.by_session_id.get(session_id).cloned()
    }

    pub async fn remove(&self, session_id: &SessionId) -> Option<AcpSessionRecord> {
        let mut inner = self.inner.write().await;
        let removed = inner.by_session_id.remove(session_id.0.as_ref())?;
        inner.session_by_thread_id.remove(&removed.thread_id);
        Some(removed)
    }

    pub async fn list(&self) -> Vec<AcpSessionRecord> {
        let inner = self.inner.read().await;
        inner.by_session_id.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session_id: &str, thread_id: &str) -> AcpSessionRecord {
        AcpSessionRecord::new(
            session_id,
            thread_id,
            "workspace-1",
            PathBuf::from("/tmp/workspace"),
            None,
            Some("Title".to_string()),
            Some("2026-05-27T00:00:00Z".to_string()),
        )
    }

    #[tokio::test]
    async fn insert_get_and_remove_session_records() {
        let sessions = AcpSessionMap::new();
        sessions.insert(record("session-1", "thread-1")).await;

        let loaded = sessions
            .get(&SessionId::new("session-1"))
            .await
            .expect("session should exist");
        assert_eq!(loaded.thread_id, "thread-1");

        let by_thread = sessions
            .session_for_thread("thread-1")
            .await
            .expect("thread reverse lookup should exist");
        assert_eq!(by_thread.session_id, "session-1");

        let removed = sessions
            .remove(&SessionId::new("session-1"))
            .await
            .expect("session should be removed");
        assert_eq!(removed.thread_id, "thread-1");
        assert!(sessions.session_for_thread("thread-1").await.is_none());
    }

    #[tokio::test]
    async fn inserting_same_thread_replaces_old_session() {
        let sessions = AcpSessionMap::new();
        sessions.insert(record("session-1", "thread-1")).await;
        sessions.insert(record("session-2", "thread-1")).await;

        assert!(sessions.get(&SessionId::new("session-1")).await.is_none());
        assert_eq!(
            sessions
                .session_for_thread("thread-1")
                .await
                .expect("new session should exist")
                .session_id,
            "session-2"
        );
    }
}
