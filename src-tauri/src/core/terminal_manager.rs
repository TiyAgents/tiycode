use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use sqlx::SqlitePool;
use tokio::sync::{broadcast, RwLock};

use crate::ipc::frontend_channels::TerminalStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::terminal::{
    TerminalAttachDto, TerminalSessionDto, TerminalSessionRecord, TerminalSessionStatus,
};
use crate::persistence::repo::{terminal_session_repo, thread_repo, workspace_repo};

const DEFAULT_TERMINAL_COLS: u16 = 120;
const DEFAULT_TERMINAL_ROWS: u16 = 36;
const REPLAY_BUFFER_MAX_BYTES: usize = 128 * 1024;

pub struct TerminalAttachment {
    pub attach: TerminalAttachDto,
    pub receiver: broadcast::Receiver<TerminalStreamEvent>,
}

pub struct TerminalManager {
    pool: SqlitePool,
    sessions_by_thread: Arc<RwLock<HashMap<String, Arc<TerminalSessionRuntime>>>>,
}

struct TerminalSessionRuntime {
    session_id: String,
    thread_id: String,
    state: Mutex<TerminalSessionState>,
    broadcaster: broadcast::Sender<TerminalStreamEvent>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    finished: AtomicBool,
}

struct TerminalSessionState {
    meta: TerminalSessionDto,
    replay: ReplayBuffer,
}

struct ReplayBuffer {
    chunks: VecDeque<String>,
    total_bytes: usize,
}

impl ReplayBuffer {
    fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            total_bytes: 0,
        }
    }

    fn clear(&mut self) {
        self.chunks.clear();
        self.total_bytes = 0;
    }

    fn push(&mut self, chunk: String) {
        let (chunk, resets_screen) = trim_replay_to_last_screen_reset(chunk);
        if resets_screen {
            self.clear();
        }

        if chunk.is_empty() {
            return;
        }

        self.total_bytes += chunk.len();
        self.chunks.push_back(chunk);

        while self.total_bytes > REPLAY_BUFFER_MAX_BYTES {
            if let Some(removed) = self.chunks.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(removed.len());
            } else {
                break;
            }
        }
    }

    fn snapshot(&self) -> String {
        self.chunks.iter().cloned().collect::<Vec<_>>().join("")
    }
}

impl TerminalSessionRuntime {
    fn snapshot_for_attach(&self) -> TerminalAttachDto {
        let mut state = self.state.lock().expect("terminal session state poisoned");
        state.meta.has_unread_output = false;

        TerminalAttachDto {
            session: state.meta.clone(),
            replay: state.replay.snapshot(),
        }
    }

    fn current_meta(&self) -> TerminalSessionDto {
        self.state
            .lock()
            .expect("terminal session state poisoned")
            .meta
            .clone()
    }

    fn recent_output(&self) -> String {
        self.state
            .lock()
            .expect("terminal session state poisoned")
            .replay
            .snapshot()
    }

    fn push_output(&self, data: &str) {
        let mut state = self.state.lock().expect("terminal session state poisoned");
        state.replay.push(data.to_string());
        state.meta.has_unread_output = true;
        state.meta.last_output_at = Some(Utc::now().to_rfc3339());
    }

    fn clear_replay(&self) {
        let mut state = self.state.lock().expect("terminal session state poisoned");
        state.replay.clear();
        state.meta.has_unread_output = false;
        state.meta.last_output_at = Some(Utc::now().to_rfc3339());
    }

    fn update_size(&self, cols: u16, rows: u16) {
        let mut state = self.state.lock().expect("terminal session state poisoned");
        state.meta.cols = cols;
        state.meta.rows = rows;
    }

    fn mark_running(&self) -> TerminalSessionDto {
        let mut state = self.state.lock().expect("terminal session state poisoned");
        state.meta.status = TerminalSessionStatus::Running;
        state.meta.clone()
    }

    fn finish(&self, exit_code: Option<i32>) -> Option<TerminalSessionDto> {
        if self.finished.swap(true, Ordering::SeqCst) {
            return None;
        }

        let mut state = self.state.lock().expect("terminal session state poisoned");
        state.meta.status = TerminalSessionStatus::Exited;
        state.meta.exit_code = exit_code;
        state.meta.last_output_at = Some(Utc::now().to_rfc3339());

        Some(state.meta.clone())
    }
}

impl TerminalManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            sessions_by_thread: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn recover_orphaned_sessions(&self) -> Result<(), AppError> {
        let count = terminal_session_repo::mark_all_active_exited(&self.pool).await?;
        if count > 0 {
            tracing::warn!(count, "recovered orphaned terminal sessions");
        }
        Ok(())
    }

    pub async fn create_or_attach(
        self: &Arc<Self>,
        thread_id: &str,
        cols: Option<u16>,
        rows: Option<u16>,
        shell_path: Option<&str>,
        shell_args: Option<&str>,
        term_env: Option<&str>,
    ) -> Result<TerminalAttachment, AppError> {
        if let Some(existing) = self.get_session(thread_id).await {
            let receiver = existing.broadcaster.subscribe();
            let attach = existing.snapshot_for_attach();
            return Ok(TerminalAttachment { attach, receiver });
        }

        if let Some(stale) =
            terminal_session_repo::find_active_by_thread(&self.pool, thread_id).await?
        {
            terminal_session_repo::update_exited(&self.pool, &stale.id, stale.exit_code).await?;
        }

        let cols = cols.unwrap_or(DEFAULT_TERMINAL_COLS);
        let rows = rows.unwrap_or(DEFAULT_TERMINAL_ROWS);
        let (thread, workspace) = self.resolve_context(thread_id).await?;
        let shell = resolve_shell(shell_path);
        let cwd = PathBuf::from(&workspace.canonical_path);
        let session_id = uuid::Uuid::now_v7().to_string();
        let created_at = Utc::now().to_rfc3339();

        let record = TerminalSessionRecord {
            id: session_id.clone(),
            thread_id: thread.id.clone(),
            workspace_id: workspace.id.clone(),
            shell_path: Some(shell.clone()),
            cwd: Some(cwd.display().to_string()),
            status: TerminalSessionStatus::Starting,
            pid: None,
            exit_code: None,
            created_at: created_at.clone(),
            exited_at: None,
        };
        terminal_session_repo::insert(&self.pool, &record).await?;

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Terminal,
                    "terminal.pty.open_failed",
                    format!("Failed to allocate PTY: {error}"),
                )
            })?;

        let mut command = CommandBuilder::new(shell.clone());
        command.cwd(cwd.clone());

        if let Some(args) = shell_args {
            let args = args.trim();
            if !args.is_empty() {
                for arg in args.split_whitespace() {
                    command.arg(arg);
                }
            }
        }

        let term_value = term_env
            .filter(|v| !v.is_empty())
            .unwrap_or("xterm-256color");
        command.env("TERM", term_value);
        command.env("COLORTERM", "truecolor");

        let child = pair.slave.spawn_command(command).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Terminal,
                "terminal.spawn_failed",
                format!("Failed to spawn shell: {error}"),
            )
        })?;

        let pid = child.process_id().map(|value| value as i64);
        terminal_session_repo::update_running(&self.pool, &session_id, pid).await?;

        let reader = pair.master.try_clone_reader().map_err(|error| {
            AppError::recoverable(
                ErrorSource::Terminal,
                "terminal.reader_failed",
                format!("Failed to clone PTY reader: {error}"),
            )
        })?;
        let writer = pair.master.take_writer().map_err(|error| {
            AppError::recoverable(
                ErrorSource::Terminal,
                "terminal.writer_failed",
                format!("Failed to create PTY writer: {error}"),
            )
        })?;
        let killer = child.clone_killer();
        let (sender, _) = broadcast::channel(512);

        let runtime = Arc::new(TerminalSessionRuntime {
            session_id: session_id.clone(),
            thread_id: thread.id.clone(),
            state: Mutex::new(TerminalSessionState {
                meta: TerminalSessionDto {
                    session_id: session_id.clone(),
                    thread_id: thread.id.clone(),
                    workspace_id: workspace.id.clone(),
                    shell: shell.clone(),
                    cwd: cwd.display().to_string(),
                    cols,
                    rows,
                    status: TerminalSessionStatus::Running,
                    has_unread_output: false,
                    last_output_at: None,
                    exit_code: None,
                    created_at: created_at.clone(),
                },
                replay: ReplayBuffer::new(),
            }),
            broadcaster: sender.clone(),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            killer: Mutex::new(killer),
            finished: AtomicBool::new(false),
        });

        {
            let mut sessions = self.sessions_by_thread.write().await;
            sessions.insert(thread.id.clone(), Arc::clone(&runtime));
        }

        let running_meta = runtime.mark_running();
        let _ = sender.send(TerminalStreamEvent::SessionCreated {
            thread_id: thread.id.clone(),
            session: running_meta.clone(),
        });
        let _ = sender.send(TerminalStreamEvent::StatusChanged {
            thread_id: thread.id.clone(),
            status: running_meta.status.clone(),
        });

        self.spawn_reader_task(Arc::clone(&runtime), reader);
        self.spawn_exit_task(thread.id.clone(), session_id.clone(), child);

        let receiver = runtime.broadcaster.subscribe();
        let attach = runtime.snapshot_for_attach();

        tracing::info!(
            thread_id = %thread.id,
            workspace_id = %workspace.id,
            session_id = %session_id,
            "terminal session created"
        );

        Ok(TerminalAttachment { attach, receiver })
    }

    pub async fn write_input(&self, thread_id: &str, data: &str) -> Result<(), AppError> {
        let session = self
            .get_session(thread_id)
            .await
            .ok_or_else(|| AppError::not_found(ErrorSource::Terminal, "terminal session"))?;

        let mut writer = session
            .writer
            .lock()
            .expect("terminal session writer poisoned");
        writer.write_all(data.as_bytes()).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Terminal,
                "terminal.write_failed",
                format!("Failed to write to terminal: {error}"),
            )
        })?;
        writer.flush().map_err(|error| {
            AppError::recoverable(
                ErrorSource::Terminal,
                "terminal.write_failed",
                format!("Failed to flush terminal input: {error}"),
            )
        })?;

        if data.contains('\u{0c}') {
            session.clear_replay();
        }

        Ok(())
    }

    pub async fn write_input_or_create(
        self: &Arc<Self>,
        thread_id: &str,
        data: &str,
    ) -> Result<TerminalSessionDto, AppError> {
        if self.get_session(thread_id).await.is_none() {
            self.create_or_attach(thread_id, None, None, None, None, None).await?;
        }

        self.write_input(thread_id, data).await?;
        self.get_status(thread_id).await
    }

    pub async fn resize(&self, thread_id: &str, cols: u16, rows: u16) -> Result<(), AppError> {
        let session = self
            .get_session(thread_id)
            .await
            .ok_or_else(|| AppError::not_found(ErrorSource::Terminal, "terminal session"))?;

        session
            .master
            .lock()
            .expect("terminal session master poisoned")
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Terminal,
                    "terminal.resize_failed",
                    format!("Failed to resize terminal: {error}"),
                )
            })?;
        session.update_size(cols, rows);

        Ok(())
    }

    pub async fn restart(
        self: &Arc<Self>,
        thread_id: &str,
        cols: Option<u16>,
        rows: Option<u16>,
        shell_path: Option<&str>,
        shell_args: Option<&str>,
        term_env: Option<&str>,
    ) -> Result<TerminalAttachment, AppError> {
        self.close(thread_id).await?;
        self.create_or_attach(thread_id, cols, rows, shell_path, shell_args, term_env).await
    }

    pub async fn close(&self, thread_id: &str) -> Result<(), AppError> {
        let session = {
            let mut sessions = self.sessions_by_thread.write().await;
            sessions.remove(thread_id)
        };

        let Some(session) = session else {
            return Ok(());
        };

        if let Some(meta) = session.finish(None) {
            terminal_session_repo::update_exited(&self.pool, &meta.session_id, None).await?;
            let _ = session
                .broadcaster
                .send(TerminalStreamEvent::StatusChanged {
                    thread_id: thread_id.to_string(),
                    status: meta.status.clone(),
                });
            let _ = session
                .broadcaster
                .send(TerminalStreamEvent::SessionExited {
                    thread_id: thread_id.to_string(),
                    exit_code: None,
                });
        }

        let result = session
            .killer
            .lock()
            .expect("terminal session killer poisoned")
            .kill();

        if let Err(error) = result {
            tracing::warn!(thread_id, error = %error, "failed to kill terminal session");
        }

        Ok(())
    }

    pub async fn close_for_thread(&self, thread_id: &str) -> Result<(), AppError> {
        self.close(thread_id).await
    }

    pub async fn list(&self) -> Vec<TerminalSessionDto> {
        let sessions = self.sessions_by_thread.read().await;
        sessions
            .values()
            .map(|session| session.current_meta())
            .collect()
    }

    pub async fn get_status(&self, thread_id: &str) -> Result<TerminalSessionDto, AppError> {
        let session = self
            .get_session(thread_id)
            .await
            .ok_or_else(|| AppError::not_found(ErrorSource::Terminal, "terminal session"))?;

        Ok(session.current_meta())
    }

    pub async fn get_recent_output(&self, thread_id: &str) -> Result<String, AppError> {
        let session = self
            .get_session(thread_id)
            .await
            .ok_or_else(|| AppError::not_found(ErrorSource::Terminal, "terminal session"))?;

        Ok(sanitize_terminal_output_for_agent(&session.recent_output()))
    }

    async fn get_session(&self, thread_id: &str) -> Option<Arc<TerminalSessionRuntime>> {
        self.sessions_by_thread.read().await.get(thread_id).cloned()
    }

    async fn resolve_context(
        &self,
        thread_id: &str,
    ) -> Result<
        (
            crate::model::thread::ThreadRecord,
            crate::model::workspace::WorkspaceRecord,
        ),
        AppError,
    > {
        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;
        let workspace = workspace_repo::find_by_id(&self.pool, &thread.workspace_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

        Ok((thread, workspace))
    }

    fn spawn_reader_task(
        &self,
        session: Arc<TerminalSessionRuntime>,
        mut reader: Box<dyn Read + Send>,
    ) {
        tokio::task::spawn_blocking(move || {
            let mut buffer = [0_u8; 8192];
            let mut pending_utf8 = Vec::new();

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        if let Some(chunk) = flush_utf8_tail(&mut pending_utf8) {
                            session.push_output(&chunk);
                            let _ = session.broadcaster.send(TerminalStreamEvent::StdoutChunk {
                                thread_id: session.thread_id.clone(),
                                data: chunk,
                            });
                        }
                        break;
                    }
                    Ok(size) => {
                        let chunk = decode_utf8_chunk(&mut pending_utf8, &buffer[..size]);
                        if !chunk.is_empty() {
                            session.push_output(&chunk);
                            let _ = session.broadcaster.send(TerminalStreamEvent::StdoutChunk {
                                thread_id: session.thread_id.clone(),
                                data: chunk,
                            });
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            thread_id = %session.thread_id,
                            session_id = %session.session_id,
                            error = %error,
                            "terminal reader stopped with error"
                        );
                        break;
                    }
                }
            }
        });
    }

    fn spawn_exit_task(
        self: &Arc<Self>,
        thread_id: String,
        session_id: String,
        mut child: Box<dyn portable_pty::Child + Send>,
    ) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(move || child.wait()).await {
                Ok(Ok(status)) => {
                    let exit_code = i32::try_from(status.exit_code()).ok();
                    if let Err(error) = manager
                        .handle_session_exit(&thread_id, &session_id, exit_code)
                        .await
                    {
                        tracing::warn!(
                            thread_id = %thread_id,
                            session_id = %session_id,
                            error = %error,
                            "failed to finalize terminal exit"
                        );
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(
                        thread_id = %thread_id,
                        session_id = %session_id,
                        error = %error,
                        "terminal wait failed"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        thread_id = %thread_id,
                        session_id = %session_id,
                        error = %error,
                        "terminal wait task cancelled"
                    );
                }
            }
        });
    }

    async fn handle_session_exit(
        &self,
        thread_id: &str,
        session_id: &str,
        exit_code: Option<i32>,
    ) -> Result<(), AppError> {
        let runtime = {
            let sessions = self.sessions_by_thread.read().await;
            sessions.get(thread_id).cloned()
        };

        let Some(runtime) = runtime else {
            terminal_session_repo::update_exited(&self.pool, session_id, exit_code).await?;
            return Ok(());
        };

        if runtime.session_id != session_id {
            terminal_session_repo::update_exited(&self.pool, session_id, exit_code).await?;
            return Ok(());
        }

        let finished = runtime.finish(exit_code);
        if finished.is_none() {
            return Ok(());
        }

        {
            let mut sessions = self.sessions_by_thread.write().await;
            if sessions
                .get(thread_id)
                .is_some_and(|session| session.session_id == session_id)
            {
                sessions.remove(thread_id);
            }
        }

        terminal_session_repo::update_exited(&self.pool, session_id, exit_code).await?;

        let meta = runtime.current_meta();
        let _ = runtime
            .broadcaster
            .send(TerminalStreamEvent::StatusChanged {
                thread_id: thread_id.to_string(),
                status: meta.status,
            });
        let _ = runtime
            .broadcaster
            .send(TerminalStreamEvent::SessionExited {
                thread_id: thread_id.to_string(),
                exit_code,
            });

        Ok(())
    }
}

fn decode_utf8_chunk(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    pending.extend_from_slice(chunk);
    let mut output = String::new();

    loop {
        match std::str::from_utf8(pending) {
            Ok(valid) => {
                output.push_str(valid);
                pending.clear();
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let valid = std::str::from_utf8(&pending[..valid_up_to])
                        .expect("utf8 prefix should be valid");
                    output.push_str(valid);
                }

                match error.error_len() {
                    Some(error_len) => {
                        let invalid_end = valid_up_to + error_len;
                        output
                            .push_str(&String::from_utf8_lossy(&pending[valid_up_to..invalid_end]));
                        pending.drain(..invalid_end);
                        if pending.is_empty() {
                            break;
                        }
                    }
                    None => {
                        pending.drain(..valid_up_to);
                        break;
                    }
                }
            }
        }
    }

    output
}

fn trim_replay_to_last_screen_reset(chunk: String) -> (String, bool) {
    const SCREEN_RESET_SEQUENCES: &[&str] = &[
        "\u{1b}c",
        "\u{1b}[3J\u{1b}[H\u{1b}[2J",
        "\u{1b}[3J\u{1b}[2J\u{1b}[H",
        "\u{1b}[H\u{1b}[2J",
        "\u{1b}[2J\u{1b}[H",
        "\u{1b}[H\u{1b}[J",
        "\u{1b}[J\u{1b}[H",
        "\u{1b}[2J",
    ];

    let mut reset_range: Option<(usize, usize)> = None;
    for pattern in SCREEN_RESET_SEQUENCES {
        if let Some(start) = chunk.rfind(pattern) {
            let end = start + pattern.len();
            let should_replace = match reset_range {
                Some((current_start, current_end)) => {
                    end > current_end || (end == current_end && start < current_start)
                }
                None => true,
            };

            if should_replace {
                reset_range = Some((start, end));
            }
        }
    }

    match reset_range {
        Some((start, _)) => (chunk[start..].to_string(), true),
        None => (chunk, false),
    }
}

fn flush_utf8_tail(pending: &mut Vec<u8>) -> Option<String> {
    if pending.is_empty() {
        return None;
    }

    let chunk = String::from_utf8_lossy(pending).to_string();
    pending.clear();
    Some(chunk)
}

fn sanitize_terminal_output_for_agent(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut output = String::with_capacity(raw.len());
    let mut index = 0;

    while index < chars.len() {
        match chars[index] {
            '\u{1b}' => {
                index += 1;
                if index >= chars.len() {
                    break;
                }

                match chars[index] {
                    '[' => {
                        index += 1;
                        while index < chars.len() {
                            let ch = chars[index];
                            index += 1;
                            if ('@'..='~').contains(&ch) {
                                break;
                            }
                        }
                    }
                    ']' => {
                        index += 1;
                        while index < chars.len() {
                            match chars[index] {
                                '\u{7}' => {
                                    index += 1;
                                    break;
                                }
                                '\u{1b}' if chars.get(index + 1).copied() == Some('\\') => {
                                    index += 2;
                                    break;
                                }
                                _ => {
                                    index += 1;
                                }
                            }
                        }
                    }
                    'P' | 'X' | '^' | '_' => {
                        index += 1;
                        while index < chars.len() {
                            if chars[index] == '\u{1b}'
                                && chars.get(index + 1).copied() == Some('\\')
                            {
                                index += 2;
                                break;
                            }
                            index += 1;
                        }
                    }
                    '(' | ')' | '*' | '+' | '-' | '.' | '/' => {
                        index += 2;
                    }
                    _ => {
                        index += 1;
                    }
                }
            }
            '\u{8}' => {
                output.pop();
                index += 1;
            }
            '\r' => {
                index += 1;
            }
            ch if ch.is_control() && ch != '\n' && ch != '\t' => {
                index += 1;
            }
            ch => {
                output.push(ch);
                index += 1;
            }
        }
    }

    output
}

fn resolve_shell(override_path: Option<&str>) -> String {
    if let Some(path) = override_path {
        let path = path.trim();
        if !path.is_empty() {
            return path.to_string();
        }
    }

    std::env::var("SHELL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            {
                "cmd.exe".to_string()
            }
            #[cfg(not(target_os = "windows"))]
            {
                "/bin/zsh".to_string()
            }
        })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellOption {
    pub path: String,
    pub name: String,
}

pub fn list_available_shells() -> Vec<ShellOption> {
    let mut shells = Vec::new();

    #[cfg(target_os = "windows")]
    {
        use std::path::Path;

        // cmd.exe - always available on Windows
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let cmd_path = format!(r"{}\System32\cmd.exe", system_root);
        if Path::new(&cmd_path).exists() {
            shells.push(ShellOption {
                path: cmd_path,
                name: "Command Prompt".to_string(),
            });
        }

        // Windows PowerShell (5.x)
        let ps_path = format!(r"{}\System32\WindowsPowerShell\v1.0\powershell.exe", system_root);
        if Path::new(&ps_path).exists() {
            shells.push(ShellOption {
                path: ps_path,
                name: "Windows PowerShell".to_string(),
            });
        }

        // PowerShell Core (7+)
        let program_files = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        let pwsh_path = format!(r"{}\PowerShell\7\pwsh.exe", program_files);
        if Path::new(&pwsh_path).exists() {
            shells.push(ShellOption {
                path: pwsh_path,
                name: "PowerShell 7".to_string(),
            });
        }

        // Git Bash
        let git_bash_candidates = [
            format!(r"{}\Git\bin\bash.exe", program_files),
            format!(r"{}\Git\usr\bin\bash.exe", program_files),
            r"C:\Program Files (x86)\Git\bin\bash.exe".to_string(),
        ];
        for candidate in &git_bash_candidates {
            if Path::new(candidate).exists() {
                shells.push(ShellOption {
                    path: candidate.clone(),
                    name: "Git Bash".to_string(),
                });
                break;
            }
        }

        // WSL
        let wsl_path = format!(r"{}\System32\wsl.exe", system_root);
        if Path::new(&wsl_path).exists() {
            shells.push(ShellOption {
                path: wsl_path,
                name: "WSL".to_string(),
            });
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::path::Path;

        // Try to read /etc/shells
        if let Ok(content) = std::fs::read_to_string("/etc/shells") {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if Path::new(line).exists() {
                    let name = Path::new(line)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(line)
                        .to_string();
                    // Avoid duplicates
                    if !shells.iter().any(|s: &ShellOption| s.path == line) {
                        shells.push(ShellOption {
                            path: line.to_string(),
                            name,
                        });
                    }
                }
            }
        }

        // Fallback if /etc/shells is empty or unreadable
        if shells.is_empty() {
            let common_shells = [
                ("/bin/zsh", "zsh"),
                ("/bin/bash", "bash"),
                ("/bin/sh", "sh"),
                ("/usr/bin/fish", "fish"),
            ];
            for (path, name) in &common_shells {
                if Path::new(path).exists() {
                    shells.push(ShellOption {
                        path: path.to_string(),
                        name: name.to_string(),
                    });
                }
            }
        }
    }

    shells
}

#[cfg(test)]
mod tests {
    use super::{
        decode_utf8_chunk, flush_utf8_tail, sanitize_terminal_output_for_agent,
        trim_replay_to_last_screen_reset, ReplayBuffer,
    };

    #[test]
    fn decode_utf8_chunk_preserves_split_multibyte_sequences() {
        let mut pending = Vec::new();
        let bytes = "├─ Claude Code".as_bytes();
        let split_at = 2;

        let first = decode_utf8_chunk(&mut pending, &bytes[..split_at]);
        let second = decode_utf8_chunk(&mut pending, &bytes[split_at..]);
        let tail = flush_utf8_tail(&mut pending);

        assert_eq!(first, "");
        assert_eq!(second, "├─ Claude Code");
        assert_eq!(tail, None);
    }

    #[test]
    fn decode_utf8_chunk_keeps_incomplete_suffix_until_next_read() {
        let mut pending = Vec::new();
        let bytes = "你A".as_bytes();

        let first = decode_utf8_chunk(&mut pending, &bytes[..2]);
        let second = decode_utf8_chunk(&mut pending, &bytes[2..]);

        assert_eq!(first, "");
        assert_eq!(second, "你A");
    }

    #[test]
    fn replay_buffer_discards_history_after_screen_reset_sequence() {
        let mut replay = ReplayBuffer::new();

        replay.push("before clear\r\n".to_string());
        replay.push("\u{1b}[H\u{1b}[2Jafter clear\r\n".to_string());

        assert_eq!(replay.snapshot(), "\u{1b}[H\u{1b}[2Jafter clear\r\n");
    }

    #[test]
    fn trim_replay_uses_last_screen_reset_in_chunk() {
        let (trimmed, resets_screen) = trim_replay_to_last_screen_reset(
            "prefix\u{1b}[2Jmiddle\u{1b}[H\u{1b}[2Jsuffix".to_string(),
        );

        assert!(resets_screen);
        assert_eq!(trimmed, "\u{1b}[H\u{1b}[2Jsuffix");
    }

    #[test]
    fn sanitize_terminal_output_for_agent_strips_ansi_and_osc_sequences() {
        let raw = "\u{1b}[0m\u{1b}[49mhello\u{1b}[39m\n\u{1b}]1;whoami\u{7}whoami\njorben\r\n";

        let sanitized = sanitize_terminal_output_for_agent(raw);

        assert_eq!(sanitized, "hello\nwhoami\njorben\n");
    }

    #[test]
    fn sanitize_terminal_output_for_agent_applies_backspaces() {
        let raw = "abc\u{8}\u{8}Z\n";

        let sanitized = sanitize_terminal_output_for_agent(raw);

        assert_eq!(sanitized, "aZ\n");
    }
}
