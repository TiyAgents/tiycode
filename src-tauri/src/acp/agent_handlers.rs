use std::path::{Path, PathBuf};

use agent_client_protocol::schema::{
    AgentCapabilities, AuthenticateRequest, AuthenticateResponse, CancelNotification,
    CloseSessionRequest, CloseSessionResponse, ContentBlock, ContentChunk, Implementation,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse,
    PromptCapabilities, PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SessionCapabilities, SessionCloseCapabilities, SessionId, SessionListCapabilities,
    SessionNotification, SessionResumeCapabilities, SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::{Agent, Client, ConnectTo, ConnectionTo, Dispatch};
use tokio::sync::broadcast;

use crate::acp::event_bridge::{content_blocks_to_markdown, map_thread_event_to_acp};
use crate::acp::permission_bridge;
use crate::acp::session_map::AcpSessionRecord;
use crate::acp::AcpServerState;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::workspace::WorkspaceAddInput;
use crate::persistence::repo::{thread_repo, workspace_repo};

pub async fn serve_connection(
    state: AcpServerState,
    transport: impl ConnectTo<Agent>,
) -> Result<(), agent_client_protocol::Error> {
    Agent
        .builder()
        .name("tiycode-acp")
        .on_receive_request(
            {
                async move |request: InitializeRequest, responder, _cx| {
                    responder.respond(handle_initialize(request))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                async move |_request: AuthenticateRequest, responder, _cx| {
                    responder.respond(AuthenticateResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: NewSessionRequest, responder, _cx| {
                    responder.respond_with_result(handle_new_session(state.clone(), request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: LoadSessionRequest, responder, _cx| {
                    responder.respond_with_result(handle_load_session(state.clone(), request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: ResumeSessionRequest, responder, _cx| {
                    responder
                        .respond_with_result(handle_resume_session(state.clone(), request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: ListSessionsRequest, responder, _cx| {
                    responder
                        .respond_with_result(handle_list_sessions(state.clone(), request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: PromptRequest, responder, cx| {
                    let state = state.clone();
                    let connection = cx.clone();
                    cx.spawn(async move {
                        responder
                            .respond_with_result(handle_prompt(state, request, connection).await)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: CloseSessionRequest, responder, _cx| {
                    responder
                        .respond_with_result(handle_close_session(state.clone(), request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                async move |notification: CancelNotification, _cx| {
                    handle_cancel(state.clone(), notification).await
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_dispatch(
            async move |message: Dispatch, cx: ConnectionTo<Client>| {
                message.respond_with_error(agent_client_protocol::Error::method_not_found(), cx)
            },
            agent_client_protocol::on_receive_dispatch!(),
        )
        .connect_to(transport)
        .await
}

fn handle_initialize(request: InitializeRequest) -> InitializeResponse {
    let session_capabilities = SessionCapabilities::new()
        .list(Some(SessionListCapabilities::new()))
        .resume(Some(SessionResumeCapabilities::new()))
        .close(Some(SessionCloseCapabilities::new()));

    InitializeResponse::new(request.protocol_version)
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
                .session_capabilities(session_capabilities),
        )
        .agent_info(Some(
            Implementation::new("tiycode", env!("CARGO_PKG_VERSION"))
                .title(Some("TiyCode".to_string())),
        ))
}

async fn handle_new_session(
    state: AcpServerState,
    request: NewSessionRequest,
) -> Result<NewSessionResponse, agent_client_protocol::Error> {
    let workspace = resolve_or_create_workspace(&state, &request.cwd).await?;
    let thread = state
        .thread_manager
        .create(&workspace.id, Some("ACP Session".to_string()), None)
        .await
        .map_err(to_acp_error)?;
    let session_id = thread.id.clone();
    let record = AcpSessionRecord::new(
        session_id.clone(),
        thread.id,
        workspace.id,
        PathBuf::from(workspace.canonical_path),
        thread.profile_id,
        Some(thread.title),
        Some(thread.last_active_at),
    );
    state.sessions.insert(record).await;
    Ok(NewSessionResponse::new(SessionId::new(session_id)))
}

async fn handle_load_session(
    state: AcpServerState,
    request: LoadSessionRequest,
) -> Result<LoadSessionResponse, agent_client_protocol::Error> {
    ensure_session_loaded(
        &state,
        request.session_id.clone(),
        Some(request.cwd.clone()),
    )
    .await?;
    Ok(LoadSessionResponse::new())
}

async fn handle_resume_session(
    state: AcpServerState,
    request: ResumeSessionRequest,
) -> Result<ResumeSessionResponse, agent_client_protocol::Error> {
    ensure_session_loaded(
        &state,
        request.session_id.clone(),
        Some(request.cwd.clone()),
    )
    .await?;
    Ok(ResumeSessionResponse::new())
}

async fn handle_list_sessions(
    state: AcpServerState,
    request: ListSessionsRequest,
) -> Result<ListSessionsResponse, agent_client_protocol::Error> {
    let mut infos = Vec::new();
    let workspaces = state.workspace_manager.list().await.map_err(to_acp_error)?;
    let cwd_filter = match request.cwd.as_ref() {
        Some(cwd) => Some(canonicalize_existing(cwd).await?),
        None => None,
    };

    for workspace in workspaces {
        let workspace_path = PathBuf::from(&workspace.canonical_path);
        if let Some(filter) = cwd_filter.as_ref() {
            if &workspace_path != filter {
                continue;
            }
        }
        let threads = state
            .thread_manager
            .list(&workspace.id, Some(100), Some(0))
            .await
            .map_err(to_acp_error)?;
        for thread in threads {
            infos.push(
                AcpSessionRecord::new(
                    thread.id.clone(),
                    thread.id,
                    workspace.id.clone(),
                    workspace_path.clone(),
                    thread.profile_id,
                    Some(thread.title),
                    Some(thread.last_active_at),
                )
                .to_session_info(),
            );
        }
    }

    Ok(ListSessionsResponse::new(infos))
}

async fn handle_close_session(
    state: AcpServerState,
    request: CloseSessionRequest,
) -> Result<CloseSessionResponse, agent_client_protocol::Error> {
    let record = ensure_session_loaded(&state, request.session_id.clone(), None).await?;
    let _ = state.agent_run_manager.cancel_run(&record.thread_id).await;
    state.sessions.remove(&request.session_id).await;
    Ok(CloseSessionResponse::new())
}

async fn handle_cancel(
    state: AcpServerState,
    notification: CancelNotification,
) -> Result<(), agent_client_protocol::Error> {
    let Some(record) = state.sessions.get(&notification.session_id).await else {
        return Ok(());
    };
    state
        .agent_run_manager
        .cancel_run(&record.thread_id)
        .await
        .map_err(to_acp_error)?;
    Ok(())
}

async fn handle_prompt(
    state: AcpServerState,
    request: PromptRequest,
    connection: ConnectionTo<Client>,
) -> Result<PromptResponse, agent_client_protocol::Error> {
    let record = ensure_session_loaded(&state, request.session_id.clone(), None).await?;
    let prompt = content_blocks_to_markdown(&request.prompt);
    if prompt.trim().is_empty() {
        return Err(agent_client_protocol::Error::invalid_params().data("prompt is empty"));
    }

    let (run_id, event_rx) = state
        .agent_run_manager
        .start_run(
            &record.thread_id,
            &prompt,
            None,
            None,
            Vec::new(),
            "default",
            record.profile_id.clone(),
            None,
            None,
            serde_json::json!({}),
        )
        .await
        .map_err(to_acp_error)?;

    pump_run_events(
        state,
        request.session_id.clone(),
        record.thread_id,
        run_id,
        event_rx,
        connection,
    )
    .await
}

async fn pump_run_events(
    state: AcpServerState,
    session_id: SessionId,
    thread_id: String,
    run_id: String,
    mut event_rx: broadcast::Receiver<ThreadStreamEvent>,
    connection: ConnectionTo<Client>,
) -> Result<PromptResponse, agent_client_protocol::Error> {
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                let mapping = map_thread_event_to_acp(&event);
                for update in mapping.updates {
                    connection.send_notification(
                        agent_client_protocol::schema::SessionNotification::new(
                            session_id.clone(),
                            update,
                        ),
                    )?;
                }

                if matches!(event, ThreadStreamEvent::ApprovalRequired { .. }) {
                    if let Err(error) = permission_bridge::request_permission_and_resolve(
                        &connection,
                        state.tool_gateway.clone(),
                        &session_id,
                        &event,
                    )
                    .await
                    {
                        reject_approval_if_pending(&state, &event).await;
                        return Err(error);
                    }
                }

                if let Some(stop_reason) = mapping.stop_reason {
                    if let Some(error) = mapping.terminal_error {
                        tracing::warn!(run_id = %run_id, error = %error, "ACP prompt ended with terminal error");
                    }
                    return Ok(PromptResponse::new(stop_reason));
                }
            }
            Err(broadcast::error::RecvError::Lagged(dropped_events)) => {
                state
                    .agent_run_manager
                    .cancel_run(&thread_id)
                    .await
                    .map_err(|error| {
                        tracing::warn!(
                            run_id = %run_id,
                            error = %error,
                            "failed to cancel ACP run after lagged stream"
                        );
                        to_acp_error(error)
                    })?;
                send_lagged_stream_cancel_notification(
                    &connection,
                    &session_id,
                    &run_id,
                    dropped_events,
                )?;
                return Ok(PromptResponse::new(StopReason::Cancelled));
            }
            Err(broadcast::error::RecvError::Closed) => {
                return Ok(PromptResponse::new(StopReason::Refusal));
            }
        }
    }
}

fn lagged_stream_cancel_message(dropped_events: u64) -> String {
    format!(
        "ACP event stream lagged and dropped {dropped_events} events; cancelling this run to avoid missing approval or terminal state."
    )
}

fn lagged_stream_cancel_update(dropped_events: u64) -> SessionUpdate {
    SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(TextContent::new(
        lagged_stream_cancel_message(dropped_events),
    ))))
}

fn send_lagged_stream_cancel_notification(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    run_id: &str,
    dropped_events: u64,
) -> Result<(), agent_client_protocol::Error> {
    tracing::warn!(
        run_id,
        dropped_events,
        "ACP event stream lagged; cancelling run to avoid missing approval state"
    );
    connection.send_notification(SessionNotification::new(
        session_id.clone(),
        lagged_stream_cancel_update(dropped_events),
    ))
}

async fn reject_approval_if_pending(state: &AcpServerState, event: &ThreadStreamEvent) {
    if let ThreadStreamEvent::ApprovalRequired { tool_call_id, .. } = event {
        let _ = state
            .tool_gateway
            .resolve_approval(tool_call_id, false)
            .await;
    }
}

async fn ensure_session_loaded(
    state: &AcpServerState,
    session_id: SessionId,
    cwd: Option<PathBuf>,
) -> Result<AcpSessionRecord, agent_client_protocol::Error> {
    if let Some(record) = state.sessions.get(&session_id).await {
        return Ok(record);
    }

    let thread = thread_repo::find_by_id(&state.pool, session_id.0.as_ref())
        .await
        .map_err(to_acp_error)?
        .ok_or_else(|| {
            agent_client_protocol::Error::invalid_params().data("unknown ACP session id")
        })?;
    let workspace = workspace_repo::find_by_id(&state.pool, &thread.workspace_id)
        .await
        .map_err(to_acp_error)?
        .ok_or_else(|| agent_client_protocol::util::internal_error("thread workspace missing"))?;
    if let Some(cwd) = cwd {
        let requested = canonicalize_existing(&cwd).await?;
        let workspace_cwd = PathBuf::from(&workspace.canonical_path);
        if requested != workspace_cwd {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("session cwd does not match thread workspace"));
        }
    }

    let record = AcpSessionRecord::new(
        session_id.0.to_string(),
        thread.id,
        workspace.id,
        PathBuf::from(workspace.canonical_path),
        thread.profile_id,
        Some(thread.title),
        Some(thread.last_active_at),
    );
    state.sessions.insert(record.clone()).await;
    Ok(record)
}

async fn resolve_or_create_workspace(
    state: &AcpServerState,
    cwd: &Path,
) -> Result<crate::model::workspace::WorkspaceRecord, agent_client_protocol::Error> {
    let canonical = canonicalize_existing(cwd).await?;
    let canonical_str = canonical.to_string_lossy().to_string();
    if let Some(workspace) = workspace_repo::find_by_canonical_path(&state.pool, &canonical_str)
        .await
        .map_err(to_acp_error)?
    {
        return Ok(workspace);
    }

    state
        .workspace_manager
        .add(WorkspaceAddInput {
            path: canonical_str,
            name: None,
        })
        .await
        .map_err(to_acp_error)
}

async fn canonicalize_existing(path: &Path) -> Result<PathBuf, agent_client_protocol::Error> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || dunce::canonicalize(&path))
        .await
        .map_err(|error| {
            agent_client_protocol::util::internal_error(format!(
                "workspace canonicalization task failed: {error}"
            ))
        })?
        .map_err(|error| {
            agent_client_protocol::Error::invalid_params()
                .data(format!("cannot canonicalize cwd: {error}"))
        })
}

fn to_acp_error(error: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::util::internal_error(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lagged_stream_cancel_update_explains_hard_cancel_policy() {
        let update = lagged_stream_cancel_update(3);

        match update {
            SessionUpdate::AgentThoughtChunk(chunk) => match chunk.content {
                ContentBlock::Text(text) => {
                    assert!(text.text.contains("dropped 3 events"));
                    assert!(text.text.contains("cancelling this run"));
                    assert!(text.text.contains("missing approval"));
                }
                other => panic!("unexpected content block: {other:?}"),
            },
            other => panic!("unexpected session update: {other:?}"),
        }
    }
}
