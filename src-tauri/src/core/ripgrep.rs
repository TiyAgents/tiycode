use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::core::windows_process::configure_background_tokio_command;

/// Execute ripgrep with a resilient lookup strategy.
///
/// Search order:
/// 1. `TIY_RG_PATH` override
/// 2. bundled app resource locations near the current executable
/// 3. `rg` available on the current process PATH
/// 4. `command -v rg` / `where.exe` from a login shell
/// 5. common install locations
pub async fn run_rg(args: Vec<OsString>) -> io::Result<std::process::Output> {
    run_rg_in(args, None::<&Path>).await
}

pub async fn run_rg_in(
    args: Vec<OsString>,
    current_dir: Option<&Path>,
) -> io::Result<std::process::Output> {
    let resolved = resolve_rg_executable().await?;
    spawn_rg(&resolved, &args, current_dir).await
}

async fn spawn_rg(
    program: impl AsRef<OsStr>,
    args: &[OsString],
    current_dir: Option<&Path>,
) -> io::Result<std::process::Output> {
    let mut cmd = Command::new(program);
    configure_background_tokio_command(&mut cmd);
    if let Some(current_dir) = current_dir {
        cmd.current_dir(current_dir);
    }
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    cmd.output().await
}

async fn resolve_rg_executable() -> io::Result<PathBuf> {
    if let Some(path) = find_env_override("TIY_RG_PATH") {
        return Ok(path);
    }

    if let Some(path) = find_bundled_rg() {
        return Ok(path);
    }

    if let Some(path) = find_on_path(executable_name()) {
        return Ok(path);
    }

    if let Some(path) = find_from_login_shell().await {
        return Ok(path);
    }

    if let Some(path) = find_common_install_locations() {
        return Ok(path);
    }
    Err(io::Error::new(
        ErrorKind::NotFound,
        "ripgrep executable was not found on PATH, in a login shell, or in bundled resources",
    ))
}

fn find_env_override(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let path = PathBuf::from(value);
    is_executable_file(&path).then_some(path)
}

fn find_on_path(executable_name: &str) -> Option<PathBuf> {
    let path_value = std::env::var_os("PATH")?;
    find_on_explicit_paths(&path_value, executable_name)
}

fn find_on_explicit_paths(path_value: &OsStr, executable_name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_value)
        .map(|dir| dir.join(executable_name))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(not(target_os = "windows"))]
async fn find_from_login_shell() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let output = Command::new(shell)
        .arg("-lc")
        .arg("command -v rg")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let candidate = PathBuf::from(stdout.lines().next()?.trim());
    is_executable_file(&candidate).then_some(candidate)
}

#[cfg(target_os = "windows")]
async fn find_from_login_shell() -> Option<PathBuf> {
    let mut command = Command::new("where.exe");
    configure_background_tokio_command(&mut command);
    let output = command
        .arg("rg")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let candidate = PathBuf::from(stdout.lines().next()?.trim());
    is_executable_file(&candidate).then_some(candidate)
}

fn find_bundled_rg() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    find_bundled_rg_in_dirs(&bundled_rg_search_dirs(&current_exe))
}

pub(crate) fn find_bundled_rg_in_dirs(dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut seen = HashSet::new();

    for dir in dirs {
        for candidate in bundled_rg_candidates_in_dir(dir) {
            if !seen.insert(candidate.clone()) {
                continue;
            }
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }

    None
}

fn find_common_install_locations() -> Option<PathBuf> {
    common_install_locations()
        .into_iter()
        .find(|candidate| is_executable_file(candidate))
}

fn common_install_locations() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/rg"));
        candidates.push(PathBuf::from("/usr/local/bin/rg"));
        candidates.push(PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/rg",
        ));
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/rg",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from("/usr/local/bin/rg"));
        candidates.push(PathBuf::from("/usr/bin/rg"));
    }

    #[cfg(target_os = "windows")]
    {
        candidates.push(PathBuf::from(r"C:\Program Files\ripgrep\rg.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files (x86)\ripgrep\rg.exe"));
    }

    candidates
}

fn bundled_rg_search_dirs(current_exe: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    if let Some(exe_dir) = current_exe.parent() {
        push_candidate(&mut dirs, &mut seen, exe_dir.to_path_buf());
        push_candidate(&mut dirs, &mut seen, exe_dir.join("resources"));
        push_candidate(&mut dirs, &mut seen, exe_dir.join("Resources"));
    }

    for ancestor in current_exe.ancestors() {
        if ancestor.file_name() == Some(OsStr::new("Contents")) {
            push_candidate(&mut dirs, &mut seen, ancestor.join("Resources"));
        }
    }

    dirs
}

fn bundled_rg_candidates_in_dir(dir: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    push_bundled_rg_entries(&mut candidates, &mut seen, dir);
    push_bundled_rg_entries(&mut candidates, &mut seen, &dir.join("bin"));

    candidates
}

fn push_bundled_rg_entries(candidates: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, dir: &Path) {
    push_candidate(candidates, seen, dir.join(executable_name()));

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        if is_bundled_rg_file_name(&file_name) {
            push_candidate(candidates, seen, entry.path());
        }
    }
}

fn is_bundled_rg_file_name(file_name: &OsStr) -> bool {
    let Some(name) = file_name.to_str() else {
        return false;
    };

    #[cfg(target_os = "windows")]
    {
        name.eq_ignore_ascii_case("rg.exe")
            || (name.starts_with("rg-") && name.to_ascii_lowercase().ends_with(".exe"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        name == "rg" || name.starts_with("rg-")
    }
}

fn push_candidate(candidates: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        candidates.push(path);
    }
}

fn executable_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "rg.exe"
    }

    #[cfg(not(target_os = "windows"))]
    {
        "rg"
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };

    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{executable_name, find_bundled_rg_in_dirs, find_on_explicit_paths};
    use std::ffi::OsString;

    #[test]
    fn finds_rg_on_explicit_path_list() {
        let tmp = tempfile::tempdir().expect("should create tempdir");
        let rg_path = tmp.path().join(executable_name());
        std::fs::write(&rg_path, "#!/bin/sh\n").expect("should create fake rg");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&rg_path)
                .expect("fake rg should exist")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&rg_path, permissions).expect("should set executable bit");
        }

        let result = find_on_explicit_paths(&OsString::from(tmp.path()), executable_name())
            .expect("rg should be discoverable");

        assert_eq!(result, rg_path);
    }

    #[test]
    fn finds_target_suffixed_bundled_rg_in_directory() {
        let tmp = tempfile::tempdir().expect("should create tempdir");
        let rg_path = tmp.path().join(test_bundled_rg_name());
        std::fs::write(&rg_path, "#!/bin/sh\n").expect("should create fake bundled rg");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&rg_path)
                .expect("fake rg should exist")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&rg_path, permissions).expect("should set executable bit");
        }

        let result = find_bundled_rg_in_dirs(&[tmp.path().to_path_buf()])
            .expect("bundled rg should be discoverable");

        assert_eq!(result, rg_path);
    }

    fn test_bundled_rg_name() -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "rg-x86_64-pc-windows-msvc.exe"
        }

        #[cfg(target_os = "macos")]
        {
            "rg-aarch64-apple-darwin"
        }

        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            "rg-x86_64-unknown-linux-gnu"
        }
    }
}
