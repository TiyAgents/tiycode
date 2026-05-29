//! Gateway runner — main event loop that drives the IM platform adapter,
//! routes commands, executes agent prompts, and handles approvals.
//!
//! The runner watches the config file for changes and dynamically reloads
//! adapters when the configuration is updated.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::StreamExt;
use tokio::sync::{broadcast, mpsc};

use crate::core::agent_session_model_plan;
use crate::core::plan_checkpoint;
use crate::gateway::platforms::wecom::WecomAdapter;
use crate::gateway::platforms::weixin::WeixinAdapter;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::thread::MessageAttachmentDto;
use crate::model::workspace::WorkspaceAddInput;
use crate::persistence::repo::profile_repo;
use crate::persistence::repo::provider_repo;
use crate::persistence::repo::thread_repo;

use super::approval_bridge;
use super::command_router::{self, GatewayCommand};
use super::config::GatewayConfig;
use super::message_formatter::{self, MessageAccumulator};
use super::traits::{Platform, PlatformAdapter};
use super::user_session::{SessionState, UserSession};
use super::GatewayState;

/// Config file poll interval for change detection.
const CONFIG_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Idle wait interval when no config or no enabled channels.
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Reconnect backoff schedule (seconds) applied after a disconnect or failure.
/// The attempt index is clamped to the last entry for sustained outages.
const RECONNECT_BACKOFF_SECONDS: &[u64] = &[2, 5, 10, 30, 60];

/// Get the modification time of a file, or None if it doesn't exist.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Try to load a valid config with at least one enabled channel.
/// Returns None if the file doesn't exist or has no enabled channels.
fn try_load_config(path: &Path) -> Option<GatewayConfig> {
    let config = GatewayConfig::load(path).ok()?;
    let has_enabled = config.weixin.as_ref().map_or(false, |w| w.enabled)
        || config.wecom.as_ref().map_or(false, |w| w.enabled);
    if has_enabled {
        Some(config)
    } else {
        None
    }
}

/// Run the gateway main loop with dynamic config watching.
///
/// The gateway starts unconditionally and watches `config_path` for changes.
/// When no config exists or no channels are enabled, it idles and polls.
/// When config changes are detected, adapters are reloaded.
pub async fn run(state: GatewayState, config_path: PathBuf) -> anyhow::Result<()> {
    tracing::info!(config = %config_path.display(), "gateway runner starting (config-watch mode)");

    let mut last_mtime: Option<SystemTime> = None;
    // Consecutive disconnect/failure counter for exponential reconnect backoff.
    let mut reconnect_attempt: usize = 0;

    loop {
        // Check for config changes.
        let current_mtime = file_mtime(&config_path);

        let config = match try_load_config(&config_path) {
            Some(c) => c,
            None => {
                if last_mtime.is_none() {
                    tracing::info!("no config or no enabled channels, waiting...");
                }
                last_mtime = current_mtime;
                tokio::time::sleep(IDLE_POLL_INTERVAL).await;
                continue;
            }
        };

        last_mtime = current_mtime;
        tracing::info!(platform = %config.platform, "config loaded, starting adapter");

        // Create adapter based on config.
        let adapter: Box<dyn PlatformAdapter> = match config.platform {
            Platform::Wecom => {
                let wecom_config = match config.wecom.clone() {
                    Some(c) => c,
                    None => {
                        tracing::warn!("[wecom] config section missing");
                        tokio::time::sleep(IDLE_POLL_INTERVAL).await;
                        continue;
                    }
                };
                Box::new(WecomAdapter::new(wecom_config))
            }
            Platform::Weixin => {
                let weixin_config = match config.weixin.clone() {
                    Some(c) => c,
                    None => {
                        tracing::warn!("[weixin] config section missing");
                        tokio::time::sleep(IDLE_POLL_INTERVAL).await;
                        continue;
                    }
                };
                Box::new(WeixinAdapter::new(weixin_config))
            }
        };

        let session =
            UserSession::load_or_create(&state.pool, config.platform, "default_user").await?;

        // Run the adapter with config change detection.
        // Returns when config changes or adapter disconnects.
        let result = run_with_adapter(
            Arc::new(state.clone()),
            config,
            adapter,
            session,
            &config_path,
            &mut last_mtime,
        )
        .await;

        match result {
            Ok(RunExitReason::ConfigChanged) => {
                tracing::info!("config changed, reloading adapter");
                reconnect_attempt = 0;
                continue;
            }
            Ok(RunExitReason::AdapterDisconnected) => {
                let delay = reconnect_backoff_delay(reconnect_attempt);
                reconnect_attempt = reconnect_attempt.saturating_add(1);
                tracing::info!(
                    delay_secs = delay.as_secs(),
                    attempt = reconnect_attempt,
                    "adapter disconnected, will retry with current config"
                );
                tokio::time::sleep(delay).await;
                continue;
            }
            Err(e) => {
                let delay = reconnect_backoff_delay(reconnect_attempt);
                reconnect_attempt = reconnect_attempt.saturating_add(1);
                tracing::error!(
                    error = %e,
                    delay_secs = delay.as_secs(),
                    attempt = reconnect_attempt,
                    "adapter run failed, will retry"
                );
                tokio::time::sleep(delay).await;
                continue;
            }
        }
    }
}

/// Compute the reconnect delay for a given consecutive-attempt index using the
/// fixed backoff schedule. The index is clamped to the final entry so sustained
/// outages settle at the longest interval instead of overflowing.
fn reconnect_backoff_delay(attempt: usize) -> Duration {
    let idx = attempt.min(RECONNECT_BACKOFF_SECONDS.len() - 1);
    Duration::from_secs(RECONNECT_BACKOFF_SECONDS[idx])
}

/// Reason the inner run loop exited.
enum RunExitReason {
    ConfigChanged,
    AdapterDisconnected,
}

/// Core runner logic extracted for testability.
async fn run_with_adapter(
    state: Arc<GatewayState>,
    config: GatewayConfig,
    adapter: Box<dyn PlatformAdapter>,
    mut session: UserSession,
    config_path: &Path,
    last_mtime: &mut Option<SystemTime>,
) -> anyhow::Result<RunExitReason> {
    let platform = adapter.platform();

    let mut adapter = adapter;
    adapter.connect().await?;
    tracing::info!(platform = %platform, user = %session.user_id, "connected to platform");

    // Track whether we have sent the welcome message (deferred until first inbound message
    // so we know the actual chat_id to reply to).
    let mut welcome_sent = session.current_workspace_id.is_some();

    // Channel for passing approval responses from the message loop into the event pump.
    let (approval_tx, approval_rx) = mpsc::channel::<bool>(1);
    // Wrap in Arc<Mutex> so both the message loop and run_agent_prompt can access.
    let approval_rx = Arc::new(tokio::sync::Mutex::new(approval_rx));

    // Channel for passing clarification responses from the message loop into the event pump.
    let (clarify_tx, clarify_rx) = mpsc::channel::<String>(4);
    let clarify_rx = Arc::new(tokio::sync::Mutex::new(clarify_rx));

    // Channel for passing /stop signals from the outer message loop into the event pump.
    let (stop_tx, stop_rx) = mpsc::channel::<()>(1);
    let stop_rx = Arc::new(tokio::sync::Mutex::new(stop_rx));

    // Main message loop with config change detection.
    // Re-snapshot mtime right before entering the loop to avoid false positives
    // from the outer loop's read vs inner loop's first check.
    *last_mtime = file_mtime(config_path);
    let exit_reason;
    {
        let mut messages = adapter.poll_messages();
        let mut config_check = tokio::time::interval(CONFIG_POLL_INTERVAL);
        config_check.tick().await; // consume first immediate tick

        exit_reason = loop {
            tokio::select! {
                msg_opt = messages.next() => {
                    let msg = match msg_opt {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            tracing::warn!(error = %e, "error receiving message from platform");
                            continue;
                        }
                        None => {
                            // Adapter stream ended (disconnected).
                            break RunExitReason::AdapterDisconnected;
                        }
                    };

                    tracing::info!(sender = %msg.sender_id, chat_id = %msg.chat_id, text = %msg.text, "inbound message received by runner");

                    // Use the inbound message's chat_id as the reply target
                    // (DM = sender_id, group = group_id).
                    let reply_to = msg.chat_id.clone();

                    // Send deferred welcome on first inbound message.
                    if !welcome_sent {
                        welcome_sent = true;
                        let welcome = "👋 你好！我是 TiyCode AI 助手\n\n\
                                       请先设置工作目录：\n  /ws add /path/to/your/project\n\n\
                                       或查看已有 workspace：\n  /ws\n\n\
                                       发送 /help 查看所有命令";
                        match adapter.send_text(&reply_to, welcome).await {
                            Ok(r) if !r.success => {
                                tracing::warn!(chat_id = %reply_to, err = ?r.error, "welcome send failed (API rejected)");
                            }
                            Err(e) => {
                                tracing::warn!(chat_id = %reply_to, error = %e, "welcome send failed (transport)");
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Handle approval responses if awaiting.
                    if session.is_awaiting_approval() || session.is_awaiting_plan_approval() || session.is_awaiting_clarify() {
                        // Allow /stop during approval to cancel the run.
                        let trimmed = msg.text.trim();
                        if trimmed.eq_ignore_ascii_case("/stop") {
                            if let Some(thread_id) = session.current_thread_id.as_deref() {
                                let _ = state.agent_run_manager.cancel_run(thread_id).await;
                            }
                            session.state = SessionState::Idle;
                            let _ = stop_tx.send(()).await;
                            let _ = adapter.send_text(&reply_to, "⏹️ 已停止当前运行").await;
                            continue;
                        }

                        // Clarification: free-text response
                        if session.is_awaiting_clarify() {
                            let text = msg.text.trim().to_string();
                            if !text.is_empty() {
                                let _ = clarify_tx.send(text).await;
                            }
                            continue;
                        }

                        // Plan approval: Y → execute_approved_plan, N → reject
                        if session.is_awaiting_plan_approval() {
                            if let Some(approved) = approval_bridge::parse_approval_response(&msg.text) {
                                let Ok(thread_id) = session.require_thread().map(|s| s.to_string()) else {
                                    let _ = adapter.send_text(&reply_to, "❌ 会话异常，缺少当前会话").await;
                                    continue;
                                };
                                if approved {
                                    let approval_message_id = match &session.state {
                                        SessionState::AwaitingPlanApproval { approval_message_id } => approval_message_id.clone(),
                                        _ => String::new(),
                                    };
                                    match state.agent_run_manager
                                        .execute_approved_plan(
                                            &thread_id,
                                            &approval_message_id,
                                            plan_checkpoint::PlanApprovalAction::ApplyPlan,
                                        )
                                        .await
                                    {
                                        Ok((run_id, event_rx)) => {
                                            session.pending_plan_approval_message_id = None;
                                            session.state = SessionState::AgentRunning { run_id: run_id.clone() };
                                            let _ = adapter.send_text(&reply_to, "✅ 计划已批准，开始执行…").await;
                                            run_event_pump(
                                                &state,
                                                &mut session,
                                                &config,
                                                adapter.as_ref(),
                                                &reply_to,
                                                &run_id,
                                                event_rx,
                                                approval_rx.clone(),
                                                clarify_rx.clone(),
                                                stop_rx.clone(),
                                            )
                                            .await?;
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "execute_approved_plan failed");
                                            // Clean up stale pending approval
                                            if let Err(e2) = state.agent_run_manager.expire_pending_plan_approval(&thread_id).await {
                                                tracing::warn!(error = %e2, "expire_pending_plan_approval after execute_approved_plan failure also failed");
                                            }
                                            session.pending_plan_approval_message_id = None;
                                            session.state = SessionState::Idle;
                                            let _ = adapter.send_text(&reply_to, &format!("❌ 启动实施失败: {e}")).await;
                                        }
                                    }
                                } else {
                                    // Reject: expire the pending approval and go idle
                                    if let Err(e) = state.agent_run_manager.expire_pending_plan_approval(&thread_id).await {
                                        tracing::warn!(error = %e, "expire_pending_plan_approval failed");
                                    }
                                    session.pending_plan_approval_message_id = None;
                                    session.state = SessionState::Idle;
                                    let _ = adapter.send_text(&reply_to, "❌ 计划已拒绝").await;
                                }
                                continue;
                            } else {
                                let _ = adapter
                                    .send_text(&reply_to, "请回复 Y(批准) 或 N(拒绝)")
                                    .await;
                                continue;
                            }
                        }

                        // Tool approval
                        if let Some(approved) = approval_bridge::parse_approval_response(&msg.text) {
                            let _ = approval_tx.send(approved).await;
                            continue;
                        } else {
                            let _ = adapter
                                .send_text(&reply_to, "请回复 Y(批准) 或 N(拒绝)")
                                .await;
                            continue;
                        }
                    }

                    // Handle /stop while agent is running — signal the event pump
                    // and cancel the run directly since the outer loop is blocked
                    // on run_agent_prompt and cannot reach dispatch_command.
                    if session.is_running() {
                        let trimmed = msg.text.trim();
                        if trimmed.eq_ignore_ascii_case("/stop") {
                            if let Some(thread_id) = session.current_thread_id.as_deref() {
                                let _ = state.agent_run_manager.cancel_run(thread_id).await;
                            }
                            session.state = SessionState::Idle;
                            let _ = stop_tx.send(()).await;
                            let _ = adapter.send_text(&reply_to, "⏹️ 已停止当前运行").await;
                            continue;
                        }

                        // Parse the message: gateway commands (e.g. /ws, /threads,
                        // /help) are handled normally since they control the
                        // session rather than the agent.  Plain text is enqueued
                        // as steer so channel users can redirect the agent
                        // mid-turn.
                        let cmd = command_router::parse(&msg.text);
                        match cmd {
                            GatewayCommand::PlainText(text) => {
                                if let Some(thread_id) =
                                    session.current_thread_id.as_deref()
                                {
                                    let text = text.trim().to_string();
                                    if !text.is_empty() {
                                        match state
                                            .agent_run_manager
                                            .enqueue_queue_message(
                                                thread_id,
                                                crate::core::agent_session_types::AgentQueueMessageKind::Steer,
                                                text,
                                                None,
                                            )
                                            .await
                                        {
                                            Ok(_) => {
                                                let _ = adapter
                                                    .send_text(
                                                        &reply_to,
                                                        "📨 消息已作为 steer 入队",
                                                    )
                                                    .await;
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    error = %e,
                                                    "failed to enqueue steer message from gateway"
                                                );
                                            }
                                        }
                                    }
                                }
                                continue;
                            }
                            // Non-PlainText commands fall through to normal
                            // dispatch below, even while the agent is running.
                            _ => {}
                        }
                    }

                    // Handle number selection when awaiting.
                    if matches!(
                        session.state,
                        SessionState::AwaitingWorkspaceSelection | SessionState::AwaitingThreadSelection
                    ) {
                        if let Ok(index) = msg.text.trim().parse::<usize>() {
                            let _ = adapter.send_typing(&reply_to).await;
                            handle_number_selection(
                                &state,
                                &mut session,
                                &config,
                                index,
                                &*adapter,
                                &reply_to,
                            )
                            .await;
                            let _ = adapter.stop_typing(&reply_to).await;
                            continue;
                        }
                        session.state = SessionState::Idle;
                    }

                    // Parse and dispatch command.
                    let cmd = command_router::parse(&msg.text);

                    // Show typing indicator for command-level interactions too
                    // (agent prompts have their own periodic refresh inside
                    // run_agent_prompt; commands are quick so a single send covers them).
                    let _ = adapter.send_typing(&reply_to).await;

                    let response = dispatch_command(
                        &state,
                        &mut session,
                        &config,
                        cmd,
                        &*adapter,
                        &reply_to,
                        &msg.media_attachments,
                        Arc::clone(&approval_rx),
                        Arc::clone(&clarify_rx),
                        Arc::clone(&stop_rx),
                    )
                    .await;

                    if let Err(e) = response {
                        match adapter.send_text(&reply_to, &format!("❌ {e}")).await {
                            Ok(r) if !r.success => {
                                tracing::warn!(chat_id = %reply_to, err = ?r.error, "error reply send failed (API rejected)");
                            }
                            Err(send_err) => {
                                tracing::warn!(chat_id = %reply_to, error = %send_err, "error reply send failed (transport)");
                            }
                            _ => {}
                        }
                    }

                    // Stop typing indicator after command completes.  Non-PlainText
                    // commands (e.g. /help, /ws, /threads) send a quick reply then
                    // return without ever calling stop_typing, leaving the bubble
                    // visible until iLink's ~15s auto-expiry.
                    let _ = adapter.stop_typing(&reply_to).await;
                }
                _ = config_check.tick() => {
                    // Periodic config change check.
                    let current_mtime = file_mtime(config_path);
                    if current_mtime != *last_mtime {
                        tracing::info!("config file changed, triggering reload");
                        *last_mtime = current_mtime;
                        break RunExitReason::ConfigChanged;
                    }
                }
            }
        };
    } // drop `messages` stream before calling disconnect

    adapter.disconnect().await;
    Ok(exit_reason)
}

/// Handle numeric selection for workspace/thread lists.
async fn handle_number_selection(
    state: &Arc<GatewayState>,
    session: &mut UserSession,
    _config: &GatewayConfig,
    index: usize,
    adapter: &dyn PlatformAdapter,
    chat_id: &str,
) {
    let adjusted = index.saturating_sub(1);

    match session.state {
        SessionState::AwaitingWorkspaceSelection => {
            let ws_info = session
                .cached_workspaces
                .get(adjusted)
                .map(|ws| (ws.id.clone(), ws.name.clone(), ws.display_path.clone()));
            if let Some((id, name, path)) = ws_info {
                if session.switch_workspace(&state.pool, &id).await.is_ok() {
                    let _ = adapter
                        .send_text(chat_id, &format!("✅ 已切换到: {name} ({path})"))
                        .await;
                }
            } else {
                let _ = adapter
                    .send_text(chat_id, "❌ 无效编号，请先发送 /ws 查看列表")
                    .await;
            }
            session.state = SessionState::Idle;
        }
        SessionState::AwaitingThreadSelection => {
            let thread_info = session.cached_threads.get(adjusted).map(|t| {
                let title = if t.title.is_empty() {
                    "(无标题)".to_string()
                } else {
                    t.title.clone()
                };
                (t.id.clone(), title)
            });
            if let Some((id, title)) = thread_info {
                if session.switch_thread(&state.pool, &id).await.is_ok() {
                    let _ = adapter
                        .send_text(chat_id, &format!("✅ 已进入会话: {title}"))
                        .await;
                }
            } else {
                let _ = adapter
                    .send_text(chat_id, "❌ 无效编号，请先发送 /threads 查看列表")
                    .await;
            }
            session.state = SessionState::Idle;
        }
        _ => {}
    }
}

/// Dispatch a parsed command.
async fn dispatch_command(
    state: &Arc<GatewayState>,
    session: &mut UserSession,
    config: &GatewayConfig,
    cmd: GatewayCommand,
    adapter: &dyn PlatformAdapter,
    chat_id: &str,
    media_attachments: &[crate::gateway::platforms::weixin_media::MediaAttachment],
    approval_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<bool>>>,
    clarify_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<String>>>,
    stop_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<()>>>,
) -> anyhow::Result<()> {
    match cmd {
        GatewayCommand::Help => {
            let help = "📖 命令列表:\n\
                        /ws — 查看 workspace 列表\n\
                        /ws add <路径> — 添加 workspace\n\
                        /ws <N> — 切换到第 N 个 workspace\n\
                        /threads — 查看当前 workspace 的会话\n\
                        /new [标题] — 创建新会话\n\
                        /resume <N> — 进入第 N 个会话\n\
                        /profile — 查看 Profile 列表\n\
                        /profile <N> — 切换当前会话 Profile\n\
                        /stop — 停止当前运行\n\
                        /status — 查看当前状态\n\
                        /help — 显示此帮助";
            adapter.send_text(chat_id, help).await?;
        }

        GatewayCommand::Status => {
            let ws_display = match session.current_workspace_id.as_deref() {
                Some(id) => {
                    use crate::persistence::repo::workspace_repo;
                    workspace_repo::find_by_id(&state.pool, id)
                        .await
                        .ok()
                        .flatten()
                        .map(|w| w.name)
                        .unwrap_or_else(|| id.to_string())
                }
                None => "(未设置)".to_string(),
            };
            let thread_display = match session.current_thread_id.as_deref() {
                Some(id) => thread_repo::find_by_id(&state.pool, id)
                    .await
                    .ok()
                    .flatten()
                    .map(|t| {
                        if t.title.is_empty() {
                            "(无标题)".to_string()
                        } else {
                            t.title
                        }
                    })
                    .unwrap_or_else(|| id.to_string()),
                None => "(未设置)".to_string(),
            };
            let status = format!(
                "📊 当前状态:\n  Workspace: {ws_display}\n  会话: {thread_display}\n  状态: {:?}",
                session.state
            );
            adapter.send_text(chat_id, &status).await?;
        }

        GatewayCommand::WorkspaceList => {
            let workspaces = state.workspace_manager.list().await?;
            session.cached_workspaces = workspaces.clone();
            session.state = SessionState::AwaitingWorkspaceSelection;
            let text = message_formatter::render_workspace_list(
                &workspaces,
                session.current_workspace_id.as_deref(),
            );
            adapter.send_text(chat_id, &text).await?;
        }

        GatewayCommand::WorkspaceAdd { path, name } => {
            let input = WorkspaceAddInput {
                path: path.clone(),
                name,
            };
            match state.workspace_manager.add(input).await {
                Ok(ws) => {
                    session.switch_workspace(&state.pool, &ws.id).await?;
                    adapter
                        .send_text(
                            chat_id,
                            &format!("✅ 已添加并切换到: {} ({})", ws.name, ws.display_path),
                        )
                        .await?;
                }
                Err(e) => {
                    adapter
                        .send_text(chat_id, &format!("❌ 添加 workspace 失败: {e}"))
                        .await?;
                }
            }
        }

        GatewayCommand::WorkspaceSwitch { index } => {
            let ws_info = session
                .cached_workspaces
                .get(index.saturating_sub(1))
                .map(|ws| (ws.id.clone(), ws.name.clone()));
            if let Some((id, name)) = ws_info {
                session.switch_workspace(&state.pool, &id).await?;
                adapter
                    .send_text(chat_id, &format!("✅ 已切换到: {name}"))
                    .await?;
            } else {
                adapter
                    .send_text(chat_id, "❌ 无效编号，请先发送 /ws 查看列表")
                    .await?;
            }
        }

        GatewayCommand::ThreadList => {
            let ws_id = session.require_workspace()?;
            let threads = state.thread_manager.list(ws_id, Some(20), Some(0)).await?;
            session.cached_threads = threads.clone();
            session.state = SessionState::AwaitingThreadSelection;
            let text = message_formatter::render_thread_list(
                &threads,
                session.current_thread_id.as_deref(),
            );
            adapter.send_text(chat_id, &text).await?;
        }

        GatewayCommand::ThreadNew { title } => {
            let ws_id = session.require_workspace()?.to_string();
            let profile_id =
                agent_session_model_plan::resolve_active_profile_id(&state.pool).await?;
            let display_title = title.as_deref().unwrap_or("IM 会话");
            let thread = state
                .thread_manager
                .create(&ws_id, Some(display_title.to_string()), profile_id.clone())
                .await?;
            session.switch_thread(&state.pool, &thread.id).await?;
            adapter
                .send_text(
                    chat_id,
                    &format!("✅ 新会话已创建: {display_title}\n发送消息开始对话"),
                )
                .await?;
        }

        GatewayCommand::ThreadResume { index } => {
            let thread_info = session
                .cached_threads
                .get(index.saturating_sub(1))
                .map(|t| {
                    let title = if t.title.is_empty() {
                        "(无标题)".to_string()
                    } else {
                        t.title.clone()
                    };
                    (t.id.clone(), title)
                });
            if let Some((id, title)) = thread_info {
                session.switch_thread(&state.pool, &id).await?;
                adapter
                    .send_text(chat_id, &format!("✅ 已进入会话: {title}"))
                    .await?;
            } else {
                adapter
                    .send_text(chat_id, "❌ 无效编号，请先发送 /threads 查看列表")
                    .await?;
            }
        }

        GatewayCommand::ProfileList => {
            let thread_id = session.require_thread()?;
            let current_profile_id = thread_repo::find_by_id(&state.pool, thread_id)
                .await
                .ok()
                .flatten()
                .and_then(|r| r.profile_id);
            let profiles = profile_repo::list_all(&state.pool).await?;
            session.cached_profiles = profiles.clone();

            // Resolve model record IDs to human-readable display names
            let mut model_names = std::collections::HashMap::new();
            for p in &profiles {
                if let Some(model_id) = p.primary_model_id.as_deref().filter(|id| !id.is_empty()) {
                    if !model_names.contains_key(model_id) {
                        if let Ok(Some(model)) =
                            provider_repo::find_model_by_id(&state.pool, model_id).await
                        {
                            let name = model.display_name.unwrap_or(model.model_name);
                            model_names.insert(model_id.to_string(), name);
                        }
                    }
                }
            }

            let text = message_formatter::render_profile_list(
                &profiles,
                current_profile_id.as_deref(),
                &model_names,
            );
            adapter.send_text(chat_id, &text).await?;
        }

        GatewayCommand::ProfileSwitch { index } => {
            let thread_id = session.require_thread()?.to_string();
            let profile_info = session
                .cached_profiles
                .get(index.saturating_sub(1))
                .map(|p| (p.id.clone(), p.name.clone()));
            if let Some((id, name)) = profile_info {
                thread_repo::update_profile(&state.pool, &thread_id, Some(&id)).await?;
                adapter
                    .send_text(chat_id, &format!("✅ 已切换 Profile: {name}"))
                    .await?;
            } else {
                adapter
                    .send_text(chat_id, "❌ 无效编号，请先发送 /profile 查看列表")
                    .await?;
            }
        }

        GatewayCommand::Stop => {
            if let SessionState::AgentRunning { .. } = session.state {
                if let Some(thread_id) = session.current_thread_id.as_deref() {
                    let _ = state.agent_run_manager.cancel_run(thread_id).await;
                }
                session.state = SessionState::Idle;
                adapter.send_text(chat_id, "⏹️ 已停止当前运行").await?;
            } else if let SessionState::AwaitingApproval { .. } = session.state {
                if let Some(thread_id) = session.current_thread_id.as_deref() {
                    let _ = state.agent_run_manager.cancel_run(thread_id).await;
                }
                session.state = SessionState::Idle;
                adapter.send_text(chat_id, "⏹️ 已停止当前运行").await?;
            } else {
                adapter.send_text(chat_id, "当前没有运行中的任务").await?;
            }
        }

        GatewayCommand::PlainText(text) => {
            run_agent_prompt(
                state,
                session,
                config,
                adapter,
                chat_id,
                &text,
                media_attachments,
                approval_rx,
                clarify_rx,
                stop_rx,
            )
            .await?;
        }
    }

    Ok(())
}

/// Execute an agent prompt: start run → pump events → accumulate → send result.
///
/// When a tool approval is required, this function waits inline for the approval
/// response via `approval_rx` (fed by the outer message loop), then continues
/// consuming events. This ensures `event_rx` is never dropped mid-run.
async fn run_agent_prompt(
    state: &Arc<GatewayState>,
    session: &mut UserSession,
    config: &GatewayConfig,
    adapter: &dyn PlatformAdapter,
    chat_id: &str,
    prompt: &str,
    media_attachments: &[crate::gateway::platforms::weixin_media::MediaAttachment],
    approval_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<bool>>>,
    clarify_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<String>>>,
    stop_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<()>>>,
) -> anyhow::Result<()> {
    let thread_id = session.require_thread()?.to_string();

    // Resolve model plan: prefer thread's stored profile_id, fallback to global active profile.
    let thread_profile_id = thread_repo::find_by_id(&state.pool, &thread_id)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.profile_id)
        .filter(|id| !id.is_empty());
    let profile_id = match thread_profile_id {
        Some(id) => Some(id),
        None => agent_session_model_plan::resolve_active_profile_id(&state.pool).await?,
    };
    let model_plan = match &profile_id {
        Some(pid) => agent_session_model_plan::build_model_plan_from_profile(&state.pool, pid)
            .await
            .map_err(|e| anyhow::anyhow!("model plan resolution failed: {e}"))?,
        None => {
            adapter
                .send_text(
                    chat_id,
                    "❌ 未配置 Agent Profile，请在 TiyCode 设置中配置模型",
                )
                .await?;
            return Ok(());
        }
    };

    // Send typing indicator.
    let _ = adapter.send_typing(chat_id).await;

    // Merge voice transcriptions into the prompt so the agent can "hear" voice messages.
    let voice_transcriptions: Vec<&str> = media_attachments
        .iter()
        .filter_map(|a| a.transcription.as_deref())
        .collect();
    let effective_prompt = if !voice_transcriptions.is_empty() {
        if prompt.is_empty() {
            voice_transcriptions.join("\n")
        } else {
            format!("{}\n\n{}", prompt, voice_transcriptions.join("\n"))
        }
    } else {
        prompt.to_string()
    };

    // Convert media attachments to MessageAttachmentDto for agent consumption.
    // For images with AES keys, download from CDN, decrypt, and produce data: URLs
    // so the LLM can actually see the image content.
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let mut attachments: Vec<MessageAttachmentDto> = Vec::new();
    for att in media_attachments {
        let name = att
            .file_name
            .clone()
            .unwrap_or_else(|| format!("{}.{}", att.media_type, mime_to_extension(&att.mime_type)));

        let url = if att.media_type == crate::gateway::platforms::weixin_media::MediaType::Image
            && att.aes_key.is_some()
        {
            // Download + decrypt image → data: URL for LLM vision
            match crate::gateway::platforms::weixin_media::download_media_as_data_url(
                &http_client,
                &att.url,
                att.aes_key.as_deref().unwrap(),
                &att.mime_type,
            )
            .await
            {
                Ok(data_url) => Some(data_url),
                Err(e) => {
                    tracing::warn!(
                        url = %att.url,
                        error = %e,
                        "failed to download/decrypt image from CDN, falling back to URL"
                    );
                    Some(att.url.clone())
                }
            }
        } else {
            Some(att.url.clone())
        };

        attachments.push(MessageAttachmentDto {
            id: uuid::Uuid::now_v7().to_string(),
            name,
            media_type: Some(att.mime_type.clone()),
            url,
        });
    }

    // Start agent run.
    let (run_id, event_rx) = state
        .agent_run_manager
        .start_run(
            &thread_id,
            &effective_prompt,
            None,        // display_prompt
            None,        // prompt_metadata
            attachments, // media attachments from inbound message
            "default",   // run_mode
            profile_id,
            None, // provider_id
            None, // model_id
            model_plan,
        )
        .await
        .map_err(|e| anyhow::anyhow!("failed to start run: {e}"))?;

    session.state = SessionState::AgentRunning {
        run_id: run_id.clone(),
    };

    // Event pump: accumulate response, handle approvals inline.
    run_event_pump(
        state,
        session,
        config,
        adapter,
        chat_id,
        &run_id,
        event_rx,
        approval_rx,
        clarify_rx,
        stop_rx,
    )
    .await
}

/// Drive the event pump for an active agent run: accumulate deltas, handle
/// approvals inline, and send the final response when the run completes.
///
/// This is factored out so it can be reused both for a fresh `start_run` and
/// for `execute_approved_plan` (which also returns an event receiver).
async fn run_event_pump(
    state: &Arc<GatewayState>,
    session: &mut UserSession,
    config: &GatewayConfig,
    adapter: &dyn PlatformAdapter,
    chat_id: &str,
    run_id: &str,
    mut event_rx: broadcast::Receiver<ThreadStreamEvent>,
    approval_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<bool>>>,
    clarify_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<String>>>,
    stop_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<()>>>,
) -> anyhow::Result<()> {
    let mut accumulator = MessageAccumulator::new();

    // Drain any stale /stop signals left from a previous run so they don't
    // immediately cancel this new agent run.
    {
        let mut guard = stop_rx.lock().await;
        while guard.try_recv().is_ok() {}
    }

    // Periodically refresh typing indicator (WeChat auto-expires after ~15s).
    let mut typing_interval = tokio::time::interval(Duration::from_secs(10));
    typing_interval.tick().await; // consume the first immediate tick

    loop {
        tokio::select! {
            // Refresh typing indicator every 10 seconds.
            _ = typing_interval.tick() => {
                let _ = adapter.send_typing(chat_id).await;
                continue;
            }
            // Watch for /stop signals from the outer message loop.
            _ = async {
                let mut guard = stop_rx.lock().await;
                guard.recv().await
            } => {
                accumulator.push_text("\n\n⏹️ 运行已取消");
                break;
            }
            event = event_rx.recv() => { match event {
            Ok(ThreadStreamEvent::MessageDelta { delta, .. }) => {
                accumulator.push_text(&delta);
            }
            Ok(ThreadStreamEvent::ApprovalRequired {
                tool_call_id,
                tool_name,
                tool_input,
                ..
            }) => {
                // Send approval request to the IM user.
                let input_str = serde_json::to_string_pretty(&tool_input)
                    .unwrap_or_else(|_| tool_input.to_string());
                let msg = approval_bridge::format_approval_request(&tool_name, &input_str);
                let _ = adapter.send_text(chat_id, &msg).await;

                // Mark session as awaiting approval (outer loop will see this).
                session.state = SessionState::AwaitingApproval {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                };

                // Wait for the approval decision from the outer message loop.
                // The outer loop feeds Y/N responses into approval_tx.
                // Also listen for /stop signals so the user can cancel mid-approval.
                let mut rx_guard = approval_rx.lock().await;
                let approved = tokio::select! {
                    result = tokio::time::timeout(
                        std::time::Duration::from_secs(config.approval_timeout_seconds),
                        rx_guard.recv(),
                    ) => {
                        result.unwrap_or(None).unwrap_or(false)
                    }
                    _ = async {
                        let mut stop_guard = stop_rx.lock().await;
                        stop_guard.recv().await
                    } => {
                        // /stop received during approval — treat as deny + cancel.
                        false
                    }
                };
                drop(rx_guard);

                // Resolve the approval in ToolGateway.
                match approval_bridge::resolve(&state.tool_gateway, &tool_call_id, approved).await {
                    Ok(true) => {
                        let response = if approved {
                            "✅ 已批准"
                        } else {
                            "❌ 已拒绝"
                        };
                        let _ = adapter.send_text(chat_id, response).await;
                    }
                    Ok(false) => {
                        let _ = adapter.send_text(chat_id, "⚠️ 审批请求已过期").await;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "approval resolution failed");
                    }
                }

                // Restore running state and continue event loop.
                session.state = SessionState::AgentRunning {
                    run_id: run_id.to_string(),
                };
                // Continue the loop — agent will emit further events after approval.
            }
            Ok(ThreadStreamEvent::RunCompleted { .. }) => break,
            Ok(ThreadStreamEvent::RunFailed { error, .. }) => {
                accumulator.push_error(&error);
                break;
            }
            Ok(ThreadStreamEvent::RunCancelled { .. }) => {
                accumulator.push_text("\n\n⏹️ 运行已取消");
                break;
            }
            Ok(ThreadStreamEvent::RunLimitReached { error, .. }) => {
                accumulator.push_error(&format!("达到轮次上限: {error}"));
                break;
            }
            Ok(ThreadStreamEvent::RunInterrupted { .. }) => {
                accumulator.push_text("\n\n⚠️ 运行被中断");
                break;
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(dropped = n, "gateway event receiver lagged");
                accumulator.push_text(&format!("\n[丢失 {n} 条事件]\n"));
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
            Ok(ThreadStreamEvent::PlanUpdated { plan, .. }) => {
                // Display plan Design section in IM, then prompt for Y/N approval.
                let artifact = serde_json::from_value::<plan_checkpoint::PlanArtifact>(plan.clone())
                    .unwrap_or_else(|_| {
                        plan_checkpoint::build_plan_artifact_from_tool_input(&plan, 0)
                    });
                let design_md = plan_checkpoint::plan_design_markdown(&artifact);
                let chunks = message_formatter::format_and_split(&design_md, config.platform);
                for chunk in &chunks {
                    adapter.send_text(chat_id, chunk).await?;
                    if chunks.len() > 1 {
                        tokio::time::sleep(config.send_chunk_delay()).await;
                    }
                }
                let approval_msg =
                    approval_bridge::format_plan_approval_request(&artifact.title);
                adapter.send_text(chat_id, &approval_msg).await?;
                // Look up the pending approval message ID so we can route Y/N later.
                if let Some(thread_id) = session.current_thread_id.as_deref() {
                    if let Ok(Some((msg, _))) = state
                        .agent_run_manager
                        .find_latest_pending_plan_approval(thread_id)
                        .await
                    {
                        session.pending_plan_approval_message_id = Some(msg.id);
                    }
                }
            }
            Ok(ThreadStreamEvent::RunCheckpointed { .. }) => {
                // Plan checkpoint reached — transition to plan approval state.
                match session.pending_plan_approval_message_id {
                    Some(ref msg_id) => {
                        session.state = SessionState::AwaitingPlanApproval {
                            approval_message_id: msg_id.clone(),
                        };
                    }
                    None => {
                        // Fallback: try fetching the pending approval from DB.
                        if let Some(thread_id) = session.current_thread_id.as_deref() {
                            if let Ok(Some((msg, _))) = state
                                .agent_run_manager
                                .find_latest_pending_plan_approval(thread_id)
                                .await
                            {
                                session.pending_plan_approval_message_id = Some(msg.id.clone());
                                session.state = SessionState::AwaitingPlanApproval {
                                    approval_message_id: msg.id,
                                };
                            } else {
                                tracing::warn!("RunCheckpointed received but no pending plan approval found");
                            }
                        }
                    }
                }
                break;
            }
            Ok(ThreadStreamEvent::ClarifyRequired {
                tool_call_id,
                tool_name,
                tool_input,
                ..
            }) => {
                // Send clarification question to the IM user.
                let input_str = serde_json::to_string_pretty(&tool_input)
                    .unwrap_or_else(|_| tool_input.to_string());
                let msg = approval_bridge::format_clarify_request(&tool_name, &input_str);
                let _ = adapter.send_text(chat_id, &msg).await;

                // Mark session as awaiting clarification.
                session.state = SessionState::AwaitingClarify {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                };

                // Wait for the user's free-text response from the outer message loop.
                // The outer loop feeds responses into clarify_rx.
                let mut rx_guard = clarify_rx.lock().await;
                let response = tokio::select! {
                    result = tokio::time::timeout(
                        std::time::Duration::from_secs(config.approval_timeout_seconds),
                        rx_guard.recv(),
                    ) => {
                        result.unwrap_or(None)
                    }
                    _ = async {
                        let mut stop_guard = stop_rx.lock().await;
                        stop_guard.recv().await
                    } => {
                        // /stop received during clarification — cancel.
                        None
                    }
                };
                drop(rx_guard);

                // Resolve the clarification in ToolGateway.
                let response_value = response
                    .map(|text| serde_json::json!({ "text": text }))
                    .unwrap_or_else(|| serde_json::json!({ "text": "" }));

                let resolved = state
                    .tool_gateway
                    .resolve_clarification(&tool_call_id, response_value)
                    .await
                    .unwrap_or(false);

                if !resolved {
                    let _ = adapter
                        .send_text(chat_id, "⚠️ 补充信息请求已过期")
                        .await;
                }

                // Restore running state and continue event loop.
                session.state = SessionState::AgentRunning {
                    run_id: run_id.to_string(),
                };
            }
            _ => {} // Other events (reasoning, tool completed, etc.) — skip.
        } } // close match + event arm
        } // close tokio::select!
    }

    // Stop typing.
    let _ = adapter.stop_typing(chat_id).await;

    // If we broke out due to RunCheckpointed (plan approval), skip sending
    // accumulated text and keep the AwaitingPlanApproval state.
    if matches!(session.state, SessionState::AwaitingPlanApproval { .. }) {
        session.save(&state.pool).await?;
        return Ok(());
    }

    // Format and send the accumulated response.
    let final_text = accumulator.finalize();
    if final_text.is_empty() {
        adapter.send_text(chat_id, "(Agent 未产生输出)").await?;
    } else {
        let chunks = message_formatter::format_and_split(&final_text, config.platform);
        for chunk in &chunks {
            adapter.send_text(chat_id, chunk).await?;
            if chunks.len() > 1 {
                tokio::time::sleep(config.send_chunk_delay()).await;
            }
        }
    }

    session.state = SessionState::Idle;
    session.save(&state.pool).await?;
    Ok(())
}

/// Map MIME type to a sensible file extension for naming generated attachments.
fn mime_to_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/amr" => "amr",
        "audio/silk" => "silk",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => "bin",
    }
}
