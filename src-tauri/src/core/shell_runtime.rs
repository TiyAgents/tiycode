use std::ffi::OsString;
#[cfg(target_os = "windows")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(not(target_os = "windows"))]
use tokio::process::Command;

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
    let mut args = Vec::with_capacity(3);
    if mode == UnixShellMode::Login {
        args.push("-l".to_string());
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
    let candidates = executable_candidates(command);

    for directory in std::env::split_paths(&path_value) {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unix_shell_command_args_include_login_flag_when_requested() {
        assert_eq!(
            unix_shell_command_args(UnixShellMode::Login, "echo hi"),
            vec!["-l", "-c", "echo hi"]
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
}
