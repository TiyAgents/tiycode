//! Gateway process supervisor — manages the lifecycle of the IM gateway child process.
//!
//! Handles:
//! - Detecting if gateway is already running (PID file + process liveness check)
//! - Starting the gateway as a child process of the current binary
//! - Stopping a running gateway (SIGTERM on Unix, TerminateProcess on Windows)
//! - Auto-restarting on version upgrade
//! - Reporting status to the frontend

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::gateway::config::default_config_path;

/// Current binary version, embedded at compile time.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Gateway process status reported to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub version: Option<String>,
    pub config_exists: bool,
}

/// Manages the gateway child process lifecycle.
pub struct GatewaySupervisor {
    /// Path to the PID file: `~/.tiy/gateway.pid`
    pid_file: PathBuf,
    /// Path to the version file: `~/.tiy/gateway.version`
    version_file: PathBuf,
    /// Path to the gateway config: `~/.tiy/gateway/config.toml`
    config_path: PathBuf,
    /// Path to the gateway log file: `~/.tiy/logs/gateway.log`
    log_path: PathBuf,
    /// Reference to the spawned child process (if we spawned it this session).
    child: Option<Child>,
}

impl GatewaySupervisor {
    /// Create a new supervisor using standard tiy home paths.
    pub fn new() -> Self {
        let tiy_home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".tiy");
        Self {
            pid_file: tiy_home.join("gateway.pid"),
            version_file: tiy_home.join("gateway.version"),
            config_path: default_config_path(),
            log_path: tiy_home.join("logs/gateway.log"),
            child: None,
        }
    }

    /// Check whether the gateway config file exists.
    pub fn config_exists(&self) -> bool {
        self.config_path.exists()
    }

    /// Check whether a gateway process is currently alive.
    pub fn is_running(&self) -> bool {
        match self.read_pid() {
            Some(pid) => is_process_alive(pid),
            None => false,
        }
    }

    /// Get the current status.
    pub fn status(&self) -> GatewayStatus {
        let pid = self.read_pid();
        let running = pid.map_or(false, is_process_alive);
        let version = self.read_version();
        GatewayStatus {
            running,
            pid: if running { pid } else { None },
            version: if running { version } else { None },
            config_exists: self.config_exists(),
        }
    }

    /// Start the gateway process if not already running.
    /// Returns Ok(true) if started, Ok(false) if already running.
    pub fn ensure_started(&mut self) -> anyhow::Result<bool> {
        if !self.config_exists() {
            anyhow::bail!("gateway config not found at {}", self.config_path.display());
        }

        if self.is_running() {
            // Check if version upgrade requires restart.
            if self.needs_upgrade_restart() {
                tracing::info!("gateway binary version mismatch, restarting");
                self.stop()?;
                // Fall through to start.
            } else {
                return Ok(false);
            }
        }

        self.start_process()
    }

    /// Stop the running gateway process.
    pub fn stop(&mut self) -> anyhow::Result<()> {
        // First try to kill our owned child.
        if let Some(ref mut child) = self.child {
            tracing::info!(pid = child.id(), "stopping owned gateway child process");
            let _ = child.kill();
            let _ = child.wait();
            self.child = None;
        } else if let Some(pid) = self.read_pid() {
            // Kill by PID (process started in a previous app session).
            if is_process_alive(pid) {
                tracing::info!(pid, "stopping gateway process by PID");
                kill_process(pid);
                // Wait briefly for it to exit.
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        self.cleanup_files();
        Ok(())
    }

    /// Restart the gateway: stop + start.
    pub fn restart(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        self.start_process()?;
        Ok(())
    }

    /// Check if the running gateway was launched with a different version.
    fn needs_upgrade_restart(&self) -> bool {
        match self.read_version() {
            Some(v) => v != CURRENT_VERSION,
            None => false, // No version file — can't determine, don't restart.
        }
    }

    /// Spawn the gateway child process.
    fn start_process(&mut self) -> anyhow::Result<bool> {
        let exe = std::env::current_exe()
            .map_err(|e| anyhow::anyhow!("failed to resolve current executable: {e}"))?;

        // Ensure log directory exists.
        if let Some(parent) = self.log_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        // Truncate log file if it exceeds 10 MB (tracing logs go to the rolling
        // appender; this file only captures panic/non-tracing stderr output).
        if self.log_path.exists() {
            if fs::metadata(&self.log_path).map(|m| m.len()).unwrap_or(0) > 10 * 1024 * 1024 {
                let _ = fs::write(&self.log_path, "");
            }
        }

        // Open log file for stdout/stderr redirection.
        let log_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to open gateway log file {}: {e}",
                    self.log_path.display()
                )
            })?;
        let log_stderr = log_file
            .try_clone()
            .map_err(|e| anyhow::anyhow!("failed to clone log file handle: {e}"))?;

        let child = Command::new(&exe)
            .args([
                "gateway",
                "--config",
                self.config_path.to_str().unwrap_or(""),
            ])
            .stdout(log_file)
            .stderr(log_stderr)
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn gateway process: {e}"))?;

        let pid = child.id();
        tracing::info!(pid, exe = %exe.display(), "gateway process started");

        // Write PID and version files.
        self.write_pid(pid)?;
        self.write_version()?;

        self.child = Some(child);
        Ok(true)
    }

    // --- File operations ---

    fn read_pid(&self) -> Option<u32> {
        fs::read_to_string(&self.pid_file)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
    }

    fn write_pid(&self, pid: u32) -> anyhow::Result<()> {
        let mut f = fs::File::create(&self.pid_file)
            .map_err(|e| anyhow::anyhow!("failed to write PID file: {e}"))?;
        write!(f, "{pid}")?;
        Ok(())
    }

    fn read_version(&self) -> Option<String> {
        fs::read_to_string(&self.version_file)
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn write_version(&self) -> anyhow::Result<()> {
        fs::write(&self.version_file, CURRENT_VERSION)
            .map_err(|e| anyhow::anyhow!("failed to write version file: {e}"))?;
        Ok(())
    }

    fn cleanup_files(&self) {
        let _ = fs::remove_file(&self.pid_file);
        let _ = fs::remove_file(&self.version_file);
    }
}

impl Drop for GatewaySupervisor {
    fn drop(&mut self) {
        // Note: we intentionally do NOT stop the gateway on drop.
        // The gateway should keep running after the GUI exits (7×24 operation).
        // We only stop if explicitly requested.
    }
}

/// Thread-safe wrapper for use in AppState / Tauri managed state.
#[derive(Clone)]
pub struct GatewaySupervisorHandle(pub Arc<Mutex<GatewaySupervisor>>);

impl GatewaySupervisorHandle {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(GatewaySupervisor::new())))
    }

    pub async fn ensure_started(&self) -> anyhow::Result<bool> {
        self.0.lock().await.ensure_started()
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        self.0.lock().await.stop()
    }

    pub async fn restart(&self) -> anyhow::Result<()> {
        self.0.lock().await.restart()
    }

    pub async fn status(&self) -> GatewayStatus {
        self.0.lock().await.status()
    }
}

// --- Platform-specific process utilities ---

#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    // kill(pid, 0) checks if process exists without sending a signal.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(unix)]
fn kill_process(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
}

#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if handle == ptr::null_mut() {
            return false;
        }
        let mut exit_code: u32 = 0;
        let result = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        result != 0 && exit_code == STILL_ACTIVE
    }
}

#[cfg(windows)]
fn kill_process(pid: u32) {
    use std::ptr;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle != ptr::null_mut() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}
