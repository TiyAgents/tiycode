//! Manages the TS Agent Sidecar child process lifecycle.
//!
//! The sidecar communicates via stdio using NDJSON (newline-delimited JSON).
//! Rust sends requests, sidecar sends back responses and events.

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};

use crate::ipc::sidecar_protocol::{RustToSidecar, SidecarEvent, SidecarToRust};
use crate::model::errors::{AppError, ErrorSource};

/// Channel capacity for sidecar events.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Manages the sidecar process and provides message-level communication.
pub struct SidecarManager {
    /// The sidecar child process handle — `None` when not started.
    child: Arc<Mutex<Option<Child>>>,
    /// Sender to write messages to sidecar stdin.
    stdin_tx: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    /// Receiver for parsed sidecar events.
    event_rx: Arc<Mutex<Option<mpsc::Receiver<SidecarEvent>>>>,
    /// Sender side kept to clone for internal writer task.
    event_tx: mpsc::Sender<SidecarEvent>,
    /// Externally cloneable receiver handle.
    event_tx_clone: mpsc::Sender<SidecarEvent>,
    /// Sidecar binary path.
    sidecar_path: String,
}

impl SidecarManager {
    pub fn new(sidecar_path: String) -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            child: Arc::new(Mutex::new(None)),
            stdin_tx: Arc::new(Mutex::new(None)),
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            event_tx: event_tx.clone(),
            event_tx_clone: event_tx,
            sidecar_path,
        }
    }

    /// Take the event receiver (can only be called once).
    /// The caller (AgentRunManager) owns the receive loop.
    pub fn take_event_receiver(&self) -> Option<mpsc::Receiver<SidecarEvent>> {
        // try_lock is fine here — only called once at init
        self.event_rx.try_lock().ok()?.take()
    }

    /// Start the sidecar process.
    pub async fn start(&self) -> Result<(), AppError> {
        let mut child_lock = self.child.lock().await;
        if child_lock.is_some() {
            tracing::warn!("sidecar already running, skipping start");
            return Ok(());
        }

        tracing::info!(path = %self.sidecar_path, "starting sidecar process");

        let mut child = Command::new(&self.sidecar_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                AppError::internal(
                    ErrorSource::Sidecar,
                    format!("Failed to spawn sidecar: {e}"),
                )
            })?;

        // Take ownership of stdin/stdout
        let stdin = child.stdin.take().ok_or_else(|| {
            AppError::internal(ErrorSource::Sidecar, "Failed to capture sidecar stdin")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AppError::internal(ErrorSource::Sidecar, "Failed to capture sidecar stdout")
        })?;

        // Spawn stdin writer task
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(64);
        tokio::spawn(async move {
            let mut writer = stdin;
            while let Some(msg) = stdin_rx.recv().await {
                if let Err(e) = writer.write_all(msg.as_bytes()).await {
                    tracing::error!(error = %e, "sidecar stdin write failed");
                    break;
                }
                if let Err(e) = writer.write_all(b"\n").await {
                    tracing::error!(error = %e, "sidecar stdin newline write failed");
                    break;
                }
                let _ = writer.flush().await;
            }
            tracing::debug!("sidecar stdin writer exited");
        });

        // Spawn stdout reader task (NDJSON parsing)
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }

                match serde_json::from_str::<SidecarToRust>(&line) {
                    Ok(SidecarToRust::Event { event, payload }) => {
                        if let Some(parsed) = SidecarEvent::parse(&event, payload) {
                            if event_tx.send(parsed).await.is_err() {
                                tracing::warn!("event receiver dropped, stopping reader");
                                break;
                            }
                        }
                    }
                    Ok(SidecarToRust::Response { id, ok, payload }) => {
                        // For now, log responses. Full request-response correlation
                        // will be added when auxiliary tasks are implemented.
                        if ok {
                            tracing::debug!(id, "sidecar response ok");
                        } else {
                            tracing::warn!(id, ?payload, "sidecar response error");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, line = %line, "failed to parse sidecar message");
                    }
                }
            }

            tracing::info!("sidecar stdout reader exited");
        });

        *self.stdin_tx.lock().await = Some(stdin_tx);
        *child_lock = Some(child);

        tracing::info!("sidecar started");
        Ok(())
    }

    /// Send a JSON-RPC request to the sidecar.
    pub async fn send_request(
        &self,
        id: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<(), AppError> {
        let msg = RustToSidecar::Request {
            id: id.to_string(),
            method: method.to_string(),
            payload,
        };

        let json = serde_json::to_string(&msg).map_err(|e| {
            AppError::internal(
                ErrorSource::Sidecar,
                format!("Failed to serialize request: {e}"),
            )
        })?;

        let tx = self.stdin_tx.lock().await;
        let tx = tx
            .as_ref()
            .ok_or_else(|| AppError::internal(ErrorSource::Sidecar, "Sidecar not started"))?;

        tx.send(json).await.map_err(|e| {
            AppError::internal(
                ErrorSource::Sidecar,
                format!("Failed to send to sidecar: {e}"),
            )
        })?;

        tracing::debug!(id, method, "sent request to sidecar");
        Ok(())
    }

    /// Stop the sidecar process gracefully.
    pub async fn stop(&self) -> Result<(), AppError> {
        // Drop stdin sender to signal EOF
        *self.stdin_tx.lock().await = None;

        let mut child_lock = self.child.lock().await;
        if let Some(mut child) = child_lock.take() {
            // Give it a moment to exit gracefully, then kill
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => {
                    tracing::info!(?status, "sidecar exited");
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "error waiting for sidecar");
                }
                Err(_) => {
                    tracing::warn!("sidecar did not exit in time, killing");
                    let _ = child.kill().await;
                }
            }
        }

        Ok(())
    }

    /// Check if sidecar process is running.
    pub async fn is_running(&self) -> bool {
        let lock = self.child.lock().await;
        lock.is_some()
    }
}
