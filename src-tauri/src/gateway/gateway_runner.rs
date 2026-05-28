//! Gateway runner — main event loop that drives the IM platform adapter,
//! routes commands, executes agent prompts, and handles approvals.

use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::{broadcast, mpsc};

use crate::core::agent_session_model_plan;
use crate::gateway::platforms::wecom::WecomAdapter;
use crate::gateway::platforms::weixin::WeixinAdapter;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::thread::MessageAttachmentDto;
use crate::model::workspace::WorkspaceAddInput;

use super::approval_bridge;
use super::command_router::{self, GatewayCommand};
use super::config::GatewayConfig;
use super::message_formatter::{self, MessageAccumulator};
use super::traits::{Platform, PlatformAdapter};
use super::user_session::{SessionState, UserSession};
use super::GatewayState;

/// Run the gateway main loop with the given platform adapter.
pub async fn run(state: GatewayState, config: GatewayConfig) -> anyhow::Result<()> {
    tracing::info!(platform = %config.platform, "gateway runner starting");

    let adapter: Box<dyn PlatformAdapter> = match config.platform {
        Platform::Wecom => {
            let wecom_config = config
                .wecom
                .clone()
                .ok_or_else(|| anyhow::anyhow!("[wecom] config section missing"))?;
            Box::new(WecomAdapter::new(wecom_config))
        }
        Platform::Weixin => {
            let weixin_config = config
                .weixin
                .clone()
                .ok_or_else(|| anyhow::anyhow!("[weixin] config section missing"))?;
            Box::new(WeixinAdapter::new(weixin_config))
        }
    };

    let session = UserSession::load_or_create(&state.pool, config.platform, "default_user").await?;

    run_with_adapter(Arc::new(state), config, adapter, session).await
}

/// Core runner logic extracted for testability.
pub async fn run_with_adapter(
    state: Arc<GatewayState>,
    config: GatewayConfig,
    adapter: Box<dyn PlatformAdapter>,
    mut session: UserSession,
) -> anyhow::Result<()> {
    let platform = adapter.platform();
    let chat_id = session.user_id.clone();

    let mut adapter = adapter;
    adapter.connect().await?;
    tracing::info!(platform = %platform, user = %chat_id, "connected to platform");

    // Send welcome message if no workspace is set.
    if session.current_workspace_id.is_none() {
        let welcome = "👋 你好！我是 TiyCode AI 助手\n\n\
                       请先设置工作目录：\n  /ws add /path/to/your/project\n\n\
                       或查看已有 workspace：\n  /ws\n\n\
                       发送 /help 查看所有命令";
        let _ = adapter.send_text(&chat_id, welcome).await;
    }

    // Channel for passing approval responses from the message loop into the event pump.
    let (approval_tx, approval_rx) = mpsc::channel::<bool>(1);
    // Wrap in Arc<Mutex> so both the message loop and run_agent_prompt can access.
    let approval_rx = Arc::new(tokio::sync::Mutex::new(approval_rx));

    // Main message loop.
    {
        let mut messages = adapter.poll_messages();
        while let Some(msg_result) = messages.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, "error receiving message from platform");
                    continue;
                }
            };

            tracing::debug!(sender = %msg.sender_id, text = %msg.text, "inbound message");

            // Handle approval responses if awaiting.
            if session.is_awaiting_approval() {
                if let Some(approved) = approval_bridge::parse_approval_response(&msg.text) {
                    // Feed the approval decision into the event pump.
                    let _ = approval_tx.send(approved).await;
                    // The event pump will handle resolve_approval and state transition.
                    continue;
                } else {
                    let _ = adapter
                        .send_text(&chat_id, "请回复 Y(批准) 或 N(拒绝)")
                        .await;
                    continue;
                }
            }

            // Handle number selection when awaiting.
            if matches!(
                session.state,
                SessionState::AwaitingWorkspaceSelection | SessionState::AwaitingThreadSelection
            ) {
                if let Ok(index) = msg.text.trim().parse::<usize>() {
                    handle_number_selection(
                        &state,
                        &mut session,
                        &config,
                        index,
                        &*adapter,
                        &chat_id,
                    )
                    .await;
                    continue;
                }
                // If not a number, fall through to normal command parsing.
                session.state = SessionState::Idle;
            }

            // Parse and dispatch command.
            let cmd = command_router::parse(&msg.text);
            let response = dispatch_command(
                &state,
                &mut session,
                &config,
                cmd,
                &*adapter,
                &chat_id,
                Arc::clone(&approval_rx),
            )
            .await;

            if let Err(e) = response {
                let _ = adapter.send_text(&chat_id, &format!("❌ {e}")).await;
            }
        }
    } // drop `messages` stream to release borrow on `adapter`

    adapter.disconnect().await;
    Ok(())
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
    approval_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<bool>>>,
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
                        /stop — 停止当前运行\n\
                        /status — 查看当前状态\n\
                        /help — 显示此帮助";
            adapter.send_text(chat_id, help).await?;
        }

        GatewayCommand::Status => {
            let ws_name = session
                .current_workspace_id
                .as_deref()
                .unwrap_or("(未设置)");
            let thread_name = session.current_thread_id.as_deref().unwrap_or("(未设置)");
            let status = format!(
                "📊 当前状态:\n  Workspace: {ws_name}\n  会话: {thread_name}\n  状态: {:?}",
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

        GatewayCommand::Stop => {
            if let SessionState::AgentRunning { .. } = session.state {
                session.state = SessionState::Idle;
                adapter.send_text(chat_id, "⏹️ 已停止当前运行").await?;
            } else {
                adapter.send_text(chat_id, "当前没有运行中的任务").await?;
            }
        }

        GatewayCommand::PlainText(text) => {
            run_agent_prompt(state, session, config, adapter, chat_id, &text, approval_rx).await?;
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
    approval_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<bool>>>,
) -> anyhow::Result<()> {
    let thread_id = session.require_thread()?.to_string();

    // Resolve model plan.
    let profile_id = agent_session_model_plan::resolve_active_profile_id(&state.pool).await?;
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

    // Start agent run.
    let (run_id, mut event_rx) = state
        .agent_run_manager
        .start_run(
            &thread_id,
            prompt,
            None,                               // display_prompt
            None,                               // prompt_metadata
            Vec::<MessageAttachmentDto>::new(), // attachments
            "default",                          // run_mode
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
    let mut accumulator = MessageAccumulator::new();

    loop {
        match event_rx.recv().await {
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
                let mut rx_guard = approval_rx.lock().await;
                let approved = tokio::time::timeout(
                    std::time::Duration::from_secs(config.approval_timeout_seconds),
                    rx_guard.recv(),
                )
                .await
                .unwrap_or(None) // timeout → None
                .unwrap_or(false); // channel closed → deny
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
                    run_id: run_id.clone(),
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
            _ => {} // Other events (reasoning, tool completed, etc.) — skip.
        }
    }

    // Stop typing.
    let _ = adapter.stop_typing(chat_id).await;

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
