use std::ffi::{OsStr, OsString};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(not(target_os = "windows"))]
use tokio::process::Command;

#[cfg(target_os = "macos")]
pub(crate) const PATH_HELPER_COMMAND_RESOLUTION_TIMEOUT: Duration = Duration::from_millis(1500);

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnixShellMode {
    Login,
    NonLogin,
}

pub(crate) fn current_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMSPEC")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string())
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn unix_shell_command_args(mode: UnixShellMode, command: &str) -> Vec<String> {
    let mut args = Vec::with_capacity(4);
    if mode == UnixShellMode::Login {
        args.push("-l".to_string());
        args.push("-i".to_string());
    }
    args.push("-c".to_string());
    args.push(command.to_string());
    args
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn build_unix_shell_command(command: &str, mode: UnixShellMode) -> Command {
    let mut process = Command::new(current_shell());
    process.args(unix_shell_command_args(mode, command));
    process
}

pub(crate) fn find_command_on_path(command: &str) -> Option<PathBuf> {
    let path_value = std::env::var_os("PATH")?;
    find_command_on_path_value(command, &path_value)
}

fn find_command_on_path_value(command: &str, path_value: &OsStr) -> Option<PathBuf> {
    let candidates = executable_candidates(command);

    for directory in std::env::split_paths(path_value) {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
pub(crate) async fn resolve_command_path(command: &str) -> Option<PathBuf> {
    if command.trim().is_empty() {
        return None;
    }

    if let Some(path) = explicit_command_path(command) {
        return path.is_file().then_some(path);
    }

    if let Some(path) = find_command_on_path(command) {
        return Some(path);
    }

    discover_command_path_from_platform_defaults(command).await
}

#[cfg(target_os = "windows")]
pub(crate) async fn resolve_command_path(command: &str) -> Option<PathBuf> {
    if command.trim().is_empty() {
        return None;
    }

    if let Some(path) = explicit_command_path(command) {
        return path.is_file().then_some(path);
    }

    find_command_on_path(command)
}

pub(crate) fn explicit_command_path(command: &str) -> Option<PathBuf> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() || trimmed.contains(std::path::MAIN_SEPARATOR) {
        return Some(path);
    }

    #[cfg(target_os = "windows")]
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Some(path);
    }

    None
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
async fn discover_command_path_from_platform_defaults(command: &str) -> Option<PathBuf> {
    resolve_command_via_login_shell(command).await
}

#[cfg(target_os = "macos")]
async fn discover_command_path_from_platform_defaults(command: &str) -> Option<PathBuf> {
    if let Some(path) = resolve_command_via_path_helper(command).await {
        return Some(path);
    }

    resolve_command_via_login_shell(command).await
}

#[cfg(target_os = "macos")]
async fn resolve_command_via_path_helper(command: &str) -> Option<PathBuf> {
    let helper_path = Path::new("/usr/libexec/path_helper");
    if !helper_path.is_file() {
        return None;
    }

    let mut process = Command::new(helper_path);
    process.arg("-s");
    process
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let output = tokio::time::timeout(PATH_HELPER_COMMAND_RESOLUTION_TIMEOUT, process.output())
        .await
        .ok()?
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let path_value = parse_path_helper_path(&stdout)?;
    find_command_on_path_value(command, path_value.as_os_str())
}

/// Resolve a bare command name by asking the user's login shell.
///
/// This covers tools whose PATH entries are injected by shell startup files
/// (e.g. nvm, fnm, pyenv, rustup) and would not appear in the process PATH
/// or macOS `path_helper` output.
#[cfg(not(target_os = "windows"))]
async fn resolve_command_via_login_shell(command: &str) -> Option<PathBuf> {
    use std::time::Duration;

    const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_millis(3000);

    // Sanitize: only allow simple command names to avoid shell injection.
    if command.contains(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ';' | '&' | '|' | '$' | '`' | '(' | ')' | '{' | '}' | '<' | '>' | '\'' | '"' | '\\'
            )
    }) {
        return None;
    }

    // Use `builtin command -v` on bash/zsh to bypass user-defined `command`
    // functions.  Fall back to plain `command -v` for other shells (e.g. dash,
    // which is /bin/sh on Debian/Ubuntu and does not recognise `builtin`).
    let shell = current_shell();
    let shell_basename = shell.rsplit('/').next().unwrap_or("");
    let shell_command = if matches!(shell_basename, "bash" | "zsh") {
        format!("builtin command -v {}", command)
    } else {
        format!("command -v {}", command)
    };
    let mut process = build_unix_shell_command(&shell_command, UnixShellMode::Login);
    process
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    let output = tokio::time::timeout(LOGIN_SHELL_TIMEOUT, process.output())
        .await
        .ok()?
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resolved = stdout.trim();
    if resolved.is_empty() {
        return None;
    }

    let path = PathBuf::from(resolved);
    if path.is_absolute() && path.is_file() {
        Some(path)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn parse_path_helper_path(output: &str) -> Option<OsString> {
    for statement in output.split(';') {
        let trimmed = statement.trim();
        let Some(value) = trimmed.strip_prefix("PATH=") else {
            continue;
        };
        let unquoted = value
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|inner| inner.strip_suffix('\''))
            })
            .unwrap_or(value)
            .trim();
        if !unquoted.is_empty() {
            return Some(OsString::from(unquoted));
        }
    }

    None
}

fn executable_candidates(command: &str) -> Vec<OsString> {
    #[cfg(target_os = "windows")]
    {
        if Path::new(command).extension().is_some() {
            return vec![OsString::from(command)];
        }

        let pathext =
            std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let mut candidates = vec![OsString::from(command)];

        for ext in pathext.to_string_lossy().split(';') {
            let trimmed = ext.trim();
            if trimmed.is_empty() {
                continue;
            }
            candidates.push(OsString::from(format!("{command}{trimmed}")));
        }

        candidates
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![OsString::from(command)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn path_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct PathEnvGuard {
        original: Option<OsString>,
    }

    impl PathEnvGuard {
        fn set(path: &std::path::Path) -> Self {
            let original = std::env::var_os("PATH");
            // SAFETY: tests serialize PATH mutation with a process-wide mutex.
            unsafe {
                std::env::set_var("PATH", path.as_os_str());
            }
            Self { original }
        }
    }

    impl Drop for PathEnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => {
                    // SAFETY: tests serialize PATH mutation with a process-wide mutex.
                    unsafe {
                        std::env::set_var("PATH", value);
                    }
                }
                None => {
                    // SAFETY: tests serialize PATH mutation with a process-wide mutex.
                    unsafe {
                        std::env::remove_var("PATH");
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unix_shell_command_args_include_login_flag_when_requested() {
        assert_eq!(
            unix_shell_command_args(UnixShellMode::Login, "echo hi"),
            vec!["-l", "-i", "-c", "echo hi"]
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unix_shell_command_args_omit_login_flag_for_non_login_shells() {
        assert_eq!(
            unix_shell_command_args(UnixShellMode::NonLogin, "echo hi"),
            vec!["-c", "echo hi"]
        );
    }

    #[test]
    fn explicit_command_path_detects_absolute_and_relative_paths() {
        let absolute = explicit_command_path("/usr/bin/env");
        assert_eq!(absolute, Some(PathBuf::from("/usr/bin/env")));

        let relative = explicit_command_path("./node_modules/.bin/foo");
        assert_eq!(relative, Some(PathBuf::from("./node_modules/.bin/foo")));

        let bare = explicit_command_path("npx");
        assert_eq!(bare, None);
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn resolve_command_path_returns_explicit_existing_path_without_path_lookup() {
        let _lock = path_env_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _guard = PathEnvGuard::set(std::path::Path::new(""));

        let resolved = resolve_command_path("/bin/sh").await;
        assert_eq!(resolved, Some(PathBuf::from("/bin/sh")));
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn resolve_command_path_finds_bare_command_on_process_path() {
        let _lock = path_env_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let command_path = temp_dir.path().join("test-shell-runtime-command");
        std::fs::write(&command_path, "#!/bin/sh\nexit 0\n").expect("write command");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&command_path)
                .expect("metadata")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&command_path, permissions).expect("set permissions");
        }

        let _guard = PathEnvGuard::set(temp_dir.path());

        let resolved = resolve_command_path("test-shell-runtime-command").await;
        assert_eq!(resolved, Some(command_path));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_path_helper_path_reads_exported_path_assignment() {
        let parsed = parse_path_helper_path(
            r#"PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"; export PATH;"#,
        );
        assert_eq!(
            parsed,
            Some(OsString::from(
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
            ))
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn resolve_command_via_login_shell_finds_common_command() {
        // In CI / minimal Linux containers the login shell may not be
        // configured properly (missing dotfiles, no interactive tty, etc.),
        // which causes the login-shell invocation to fail or time out.
        // Try several ubiquitous commands; if *none* resolve we skip instead
        // of failing, because the feature under test is environment-dependent.
        let candidates = ["sh", "ls", "cat"];
        let mut found = false;
        for cmd in &candidates {
            if let Some(path) = resolve_command_via_login_shell(cmd).await {
                assert!(
                    path.is_absolute(),
                    "resolved path for '{}' should be absolute, got {:?}",
                    cmd,
                    path
                );
                assert!(
                    path.is_file(),
                    "resolved path for '{}' should be a file, got {:?}",
                    cmd,
                    path
                );
                found = true;
                break;
            }
        }
        if !found {
            eprintln!(
                "SKIP: login shell could not resolve any of {:?} — \
                 likely a minimal CI environment without a proper login shell",
                candidates
            );
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn resolve_command_via_login_shell_rejects_shell_metacharacters() {
        let resolved = resolve_command_via_login_shell("echo; rm -rf /").await;
        assert_eq!(resolved, None);
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn resolve_command_via_login_shell_returns_none_for_nonexistent_command() {
        let resolved =
            resolve_command_via_login_shell("this-command-definitely-does-not-exist-xyz").await;
        assert_eq!(resolved, None);
    }
}
