#[cfg(any(target_os = "macos", target_os = "windows"))]
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::fs;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
use crate::core::windows_process::configure_background_std_command;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMetadata {
    app_name: String,
    version: String,
    platform: String,
    arch: String,
    runtime: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOpenApp {
    id: String,
    name: String,
    open_with: Option<String>,
    icon_data_url: Option<String>,
}

#[tauri::command]
pub fn get_system_metadata() -> SystemMetadata {
    let version = match option_env!("BUILD_COMMIT_HASH") {
        Some(hash) => format!("{}-dev.{}", env!("CARGO_PKG_VERSION"), hash),
        None => env!("CARGO_PKG_VERSION").to_string(),
    };
    SystemMetadata {
        app_name: "TiyCode".into(),
        version,
        platform: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        runtime: "Tauri 2".into(),
    }
}

#[tauri::command]
pub fn is_homebrew_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        // Check if the running binary lives under a Homebrew Cellar / Caskroom prefix.
        // Homebrew cask installs symlink into /opt/homebrew/Caskroom (Apple Silicon)
        // or /usr/local/Caskroom (Intel).  The actual .app bundle is placed inside
        // the Caskroom directory tree, so checking the current executable path is
        // the most reliable indicator.
        if let Ok(exe_path) = std::env::current_exe() {
            let path_str = exe_path.to_string_lossy();
            return path_str.contains("/Caskroom/")
                || path_str.contains("/Cellar/")
                || path_str.contains("/homebrew/");
        }
        false
    }

    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[tauri::command]
pub fn get_workspace_open_apps() -> Vec<WorkspaceOpenApp> {
    #[cfg(target_os = "macos")]
    {
        return macos_workspace_open_apps();
    }

    #[cfg(target_os = "windows")]
    {
        return windows_workspace_open_apps();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[tauri::command]
pub fn open_workspace_in_app(
    target_path: String,
    app_id: String,
    app_path: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return open_workspace_in_app_macos(&target_path, &app_id, app_path.as_deref());
    }

    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "windows")]
        {
            return open_workspace_in_app_windows(&target_path, &app_id, app_path.as_deref());
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (target_path, app_id, app_path);
            Err(
                "Opening workspace folders in external apps is only supported on macOS and Windows right now."
                    .into(),
            )
        }
    }
}

#[tauri::command]
pub fn open_tree_path_in_app(
    target_path: String,
    is_directory: bool,
    app_id: String,
    app_path: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return open_tree_path_in_app_macos(
            &target_path,
            is_directory,
            &app_id,
            app_path.as_deref(),
        );
    }

    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "windows")]
        {
            return open_tree_path_in_app_windows(
                &target_path,
                is_directory,
                &app_id,
                app_path.as_deref(),
            );
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (target_path, is_directory, app_id, app_path);
            Err("Opening tree paths in external apps is only supported on macOS and Windows right now.".into())
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreePathOpenBehavior {
    OpenTarget,
    RevealTarget,
    OpenContainingFolder,
}

#[cfg(target_os = "macos")]
struct WorkspaceOpenAppSpec {
    id: &'static str,
    name: &'static str,
    bundle_names: &'static [&'static str],
    preferred_paths: &'static [&'static str],
}

#[cfg(target_os = "macos")]
const WORKSPACE_OPEN_APP_SPECS: [WorkspaceOpenAppSpec; 13] = [
    WorkspaceOpenAppSpec {
        id: "finder",
        name: "Finder",
        bundle_names: &["Finder.app"],
        preferred_paths: &["/System/Library/CoreServices/Finder.app"],
    },
    WorkspaceOpenAppSpec {
        id: "terminal",
        name: "Terminal",
        bundle_names: &["Terminal.app"],
        preferred_paths: &[
            "/System/Applications/Utilities/Terminal.app",
            "/Applications/Utilities/Terminal.app",
        ],
    },
    WorkspaceOpenAppSpec {
        id: "iterm2",
        name: "iTerm2",
        bundle_names: &["iTerm.app"],
        preferred_paths: &["/Applications/iTerm.app"],
    },
    WorkspaceOpenAppSpec {
        id: "warp",
        name: "Warp",
        bundle_names: &["Warp.app"],
        preferred_paths: &["/Applications/Warp.app"],
    },
    WorkspaceOpenAppSpec {
        id: "vscode",
        name: "VS Code",
        bundle_names: &["Visual Studio Code.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "cursor",
        name: "Cursor",
        bundle_names: &["Cursor.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "windsurf",
        name: "Windsurf",
        bundle_names: &["Windsurf.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "zed",
        name: "Zed",
        bundle_names: &["Zed.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "intellij-idea",
        name: "IntelliJ IDEA",
        bundle_names: &["IntelliJ IDEA.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "pycharm",
        name: "PyCharm",
        bundle_names: &["PyCharm.app", "PyCharm CE.app", "PyCharm Professional.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "goland",
        name: "GoLand",
        bundle_names: &["GoLand.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "android-studio",
        name: "Android Studio",
        bundle_names: &["Android Studio.app", "Android Studio Preview.app"],
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "ghostty",
        name: "Ghostty",
        bundle_names: &["Ghostty.app"],
        preferred_paths: &["/Applications/Ghostty.app"],
    },
];

#[cfg(target_os = "macos")]
fn macos_workspace_open_apps() -> Vec<WorkspaceOpenApp> {
    WORKSPACE_OPEN_APP_SPECS
        .iter()
        .filter_map(|spec| {
            let app_path = find_app_bundle(spec)?;
            let icon_path = resolve_icon_path(&app_path)?;
            let icon_data_url = build_icon_data_url(&icon_path, spec.id)?;

            Some(WorkspaceOpenApp {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                open_with: build_open_with(spec, &app_path),
                icon_data_url: Some(icon_data_url),
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn build_open_with(spec: &WorkspaceOpenAppSpec, app_path: &Path) -> Option<String> {
    if spec.id == "finder" {
        None
    } else {
        Some(app_path.to_string_lossy().into_owned())
    }
}

#[cfg(target_os = "macos")]
fn find_workspace_open_app_spec(app_id: &str) -> Option<&'static WorkspaceOpenAppSpec> {
    WORKSPACE_OPEN_APP_SPECS
        .iter()
        .find(|spec| spec.id == app_id)
}

#[cfg(target_os = "macos")]
fn open_workspace_in_app_macos(
    target_path: &str,
    app_id: &str,
    app_path: Option<&str>,
) -> Result<(), String> {
    let resolved_app_path = app_path
        .map(PathBuf::from)
        .or_else(|| find_workspace_open_app_spec(app_id).and_then(find_app_bundle))
        .map(|path| path.to_string_lossy().into_owned());

    match app_id {
        "warp" => open_workspace_in_warp_macos(target_path),
        _ => open_workspace_with_open_command_macos(target_path, resolved_app_path.as_deref()),
    }
}

#[cfg(target_os = "macos")]
fn open_tree_path_in_app_macos(
    target_path: &str,
    is_directory: bool,
    app_id: &str,
    app_path: Option<&str>,
) -> Result<(), String> {
    let resolved_app_path = app_path
        .map(PathBuf::from)
        .or_else(|| find_workspace_open_app_spec(app_id).and_then(find_app_bundle))
        .map(|path| path.to_string_lossy().into_owned());

    let behavior = tree_path_open_behavior_macos(app_id, is_directory);
    match behavior {
        TreePathOpenBehavior::OpenTarget => {
            if app_id == "warp" {
                open_workspace_in_warp_macos(target_path)
            } else {
                open_workspace_with_open_command_macos(target_path, resolved_app_path.as_deref())
            }
        }
        TreePathOpenBehavior::RevealTarget => reveal_tree_path_in_finder_macos(target_path),
        TreePathOpenBehavior::OpenContainingFolder => {
            let folder_path = containing_directory_path(target_path)?;
            if app_id == "warp" {
                open_workspace_in_warp_macos(&folder_path)
            } else {
                open_workspace_with_open_command_macos(&folder_path, resolved_app_path.as_deref())
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn tree_path_open_behavior_macos(app_id: &str, is_directory: bool) -> TreePathOpenBehavior {
    if is_directory {
        return TreePathOpenBehavior::OpenTarget;
    }

    match app_id {
        "finder" => TreePathOpenBehavior::RevealTarget,
        "terminal" | "iterm2" | "warp" | "ghostty" => TreePathOpenBehavior::OpenContainingFolder,
        _ => TreePathOpenBehavior::OpenTarget,
    }
}

#[cfg(target_os = "macos")]
fn open_workspace_with_open_command_macos(
    target_path: &str,
    app_path: Option<&str>,
) -> Result<(), String> {
    let mut command = Command::new("open");

    if let Some(app_path) = app_path {
        command.arg("-a").arg(app_path);
    }

    let output = command
        .arg(target_path)
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("`open` exited with status {}.", output.status)
        };

        Err(message)
    }
}

#[cfg(target_os = "macos")]
fn reveal_tree_path_in_finder_macos(target_path: &str) -> Result<(), String> {
    let output = Command::new("open")
        .arg("-R")
        .arg(target_path)
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("`open -R` exited with status {}.", output.status)
        };

        Err(message)
    }
}

#[cfg(target_os = "macos")]
fn open_workspace_in_warp_macos(target_path: &str) -> Result<(), String> {
    let uri = format!(
        "warp://action/new_window?path={}",
        percent_encode_uri_component(target_path)
    );
    open_workspace_with_open_command_macos(&uri, None)
}

#[cfg(target_os = "windows")]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowsLaunchBehavior {
    Explorer,
    PathArgument,
    WorkingDirectoryOnly,
}

#[cfg(target_os = "windows")]
struct WindowsWorkspaceOpenAppSpec {
    id: &'static str,
    name: &'static str,
    executable_name: Option<&'static str>,
    relative_paths: &'static [&'static str],
    icon_relative_paths: &'static [&'static str],
    versioned_directory_roots: &'static [&'static str],
    versioned_directory_prefixes: &'static [&'static str],
    versioned_executable_suffix: Option<&'static str>,
    toolbox_product_dirs: &'static [&'static str],
    toolbox_channel_dirs: &'static [&'static str],
    launch_behavior: WindowsLaunchBehavior,
    force_new_console: bool,
}

#[cfg(target_os = "windows")]
const WINDOWS_WORKSPACE_OPEN_APP_SPECS: [WindowsWorkspaceOpenAppSpec; 11] = [
    WindowsWorkspaceOpenAppSpec {
        id: "explorer",
        name: "Explorer",
        executable_name: None,
        relative_paths: &[],
        icon_relative_paths: &[],
        versioned_directory_roots: &[],
        versioned_directory_prefixes: &[],
        versioned_executable_suffix: None,
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::Explorer,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "powershell",
        name: "PowerShell",
        executable_name: Some("powershell.exe"),
        relative_paths: &[
            "System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            "SysWOW64\\WindowsPowerShell\\v1.0\\powershell.exe",
        ],
        icon_relative_paths: &[
            "System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            "SysWOW64\\WindowsPowerShell\\v1.0\\powershell.exe",
        ],
        versioned_directory_roots: &[],
        versioned_directory_prefixes: &[],
        versioned_executable_suffix: None,
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::WorkingDirectoryOnly,
        force_new_console: true,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "git-bash",
        name: "Git Bash",
        executable_name: Some("git-bash.exe"),
        relative_paths: &[
            "Git\\git-bash.exe",
            "Programs\\Git\\git-bash.exe",
            "GitHub\\PortableGit\\git-bash.exe",
        ],
        icon_relative_paths: &[
            "Git\\git-bash.exe",
            "Programs\\Git\\git-bash.exe",
            "Git\\mingw64\\share\\git\\git-for-windows.ico",
            "Programs\\Git\\mingw64\\share\\git\\git-for-windows.ico",
        ],
        versioned_directory_roots: &[],
        versioned_directory_prefixes: &[],
        versioned_executable_suffix: None,
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::WorkingDirectoryOnly,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "vscode",
        name: "VS Code",
        executable_name: Some("Code.exe"),
        relative_paths: &[
            "Microsoft VS Code\\Code.exe",
            "Programs\\Microsoft VS Code\\Code.exe",
            "Microsoft VS Code Insiders\\Code - Insiders.exe",
            "Programs\\Microsoft VS Code Insiders\\Code - Insiders.exe",
        ],
        icon_relative_paths: &[
            "Microsoft VS Code\\Code.exe",
            "Programs\\Microsoft VS Code\\Code.exe",
            "Microsoft VS Code Insiders\\Code - Insiders.exe",
            "Programs\\Microsoft VS Code Insiders\\Code - Insiders.exe",
        ],
        versioned_directory_roots: &[],
        versioned_directory_prefixes: &[],
        versioned_executable_suffix: None,
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "cursor",
        name: "Cursor",
        executable_name: Some("Cursor.exe"),
        relative_paths: &["Cursor\\Cursor.exe", "Programs\\Cursor\\Cursor.exe"],
        icon_relative_paths: &["Cursor\\Cursor.exe", "Programs\\Cursor\\Cursor.exe"],
        versioned_directory_roots: &[],
        versioned_directory_prefixes: &[],
        versioned_executable_suffix: None,
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "windsurf",
        name: "Windsurf",
        executable_name: Some("Windsurf.exe"),
        relative_paths: &["Windsurf\\Windsurf.exe", "Programs\\Windsurf\\Windsurf.exe"],
        icon_relative_paths: &["Windsurf\\Windsurf.exe", "Programs\\Windsurf\\Windsurf.exe"],
        versioned_directory_roots: &[],
        versioned_directory_prefixes: &[],
        versioned_executable_suffix: None,
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "zed",
        name: "Zed",
        executable_name: Some("Zed.exe"),
        relative_paths: &["Zed\\Zed.exe", "Programs\\Zed\\Zed.exe"],
        icon_relative_paths: &["Zed\\Zed.exe", "Programs\\Zed\\Zed.exe"],
        versioned_directory_roots: &[],
        versioned_directory_prefixes: &[],
        versioned_executable_suffix: None,
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "intellij-idea",
        name: "IntelliJ IDEA",
        executable_name: Some("idea64.exe"),
        relative_paths: &[
            "Programs\\IntelliJ IDEA\\bin\\idea64.exe",
            "JetBrains\\IntelliJ IDEA\\bin\\idea64.exe",
        ],
        icon_relative_paths: &[
            "Programs\\IntelliJ IDEA\\bin\\idea64.exe",
            "JetBrains\\IntelliJ IDEA\\bin\\idea64.exe",
        ],
        versioned_directory_roots: &["JetBrains"],
        versioned_directory_prefixes: &["IntelliJ IDEA"],
        versioned_executable_suffix: Some("bin\\idea64.exe"),
        toolbox_product_dirs: &["IDEA-U", "IDEA-C"],
        toolbox_channel_dirs: &["ch-0", "ch-1"],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "pycharm",
        name: "PyCharm",
        executable_name: Some("pycharm64.exe"),
        relative_paths: &[
            "Programs\\PyCharm\\bin\\pycharm64.exe",
            "JetBrains\\PyCharm\\bin\\pycharm64.exe",
            "Programs\\PyCharm CE\\bin\\pycharm64.exe",
            "JetBrains\\PyCharm CE\\bin\\pycharm64.exe",
        ],
        icon_relative_paths: &[
            "Programs\\PyCharm\\bin\\pycharm64.exe",
            "JetBrains\\PyCharm\\bin\\pycharm64.exe",
            "Programs\\PyCharm CE\\bin\\pycharm64.exe",
            "JetBrains\\PyCharm CE\\bin\\pycharm64.exe",
        ],
        versioned_directory_roots: &["JetBrains"],
        versioned_directory_prefixes: &["PyCharm", "PyCharm Community", "PyCharm Professional"],
        versioned_executable_suffix: Some("bin\\pycharm64.exe"),
        toolbox_product_dirs: &["PyCharm-P", "PyCharm-C"],
        toolbox_channel_dirs: &["ch-0", "ch-1"],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "goland",
        name: "GoLand",
        executable_name: Some("goland64.exe"),
        relative_paths: &[
            "Programs\\GoLand\\bin\\goland64.exe",
            "JetBrains\\GoLand\\bin\\goland64.exe",
        ],
        icon_relative_paths: &[
            "Programs\\GoLand\\bin\\goland64.exe",
            "JetBrains\\GoLand\\bin\\goland64.exe",
        ],
        versioned_directory_roots: &["JetBrains"],
        versioned_directory_prefixes: &["GoLand"],
        versioned_executable_suffix: Some("bin\\goland64.exe"),
        toolbox_product_dirs: &["GoLand"],
        toolbox_channel_dirs: &["ch-0", "ch-1"],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
    WindowsWorkspaceOpenAppSpec {
        id: "android-studio",
        name: "Android Studio",
        executable_name: Some("studio64.exe"),
        relative_paths: &[
            "Android\\Android Studio\\bin\\studio64.exe",
            "Programs\\Android Studio\\bin\\studio64.exe",
            "android-studio\\bin\\studio64.exe",
        ],
        icon_relative_paths: &[
            "Android\\Android Studio\\bin\\studio64.exe",
            "Programs\\Android Studio\\bin\\studio64.exe",
            "android-studio\\bin\\studio64.exe",
        ],
        versioned_directory_roots: &["Android"],
        versioned_directory_prefixes: &["Android Studio", "Android Studio Preview"],
        versioned_executable_suffix: Some("bin\\studio64.exe"),
        toolbox_product_dirs: &[],
        toolbox_channel_dirs: &[],
        launch_behavior: WindowsLaunchBehavior::PathArgument,
        force_new_console: false,
    },
];

#[cfg(target_os = "windows")]
fn windows_workspace_open_apps() -> Vec<WorkspaceOpenApp> {
    WINDOWS_WORKSPACE_OPEN_APP_SPECS
        .iter()
        .filter_map(|spec| {
            let open_with = find_windows_app_launcher(spec)?;
            let icon_data_url = find_windows_icon_source(spec, open_with.as_deref())
                .and_then(|path| build_windows_icon_data_url(&path, spec.id));

            Some(WorkspaceOpenApp {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                open_with,
                icon_data_url,
            })
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn find_windows_app_launcher(spec: &WindowsWorkspaceOpenAppSpec) -> Option<Option<String>> {
    if spec.id == "explorer" {
        return Some(None);
    }

    resolve_windows_app_path(spec).map(|path| Some(path.to_string_lossy().into_owned()))
}

#[cfg(target_os = "windows")]
fn find_windows_icon_source(
    spec: &WindowsWorkspaceOpenAppSpec,
    open_with: Option<&str>,
) -> Option<PathBuf> {
    if spec.id == "explorer" {
        return windows_system_root().map(|root| root.join("explorer.exe"));
    }

    if let Some(open_with) = open_with {
        let candidate = PathBuf::from(open_with);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    resolve_windows_icon_path(spec)
}

#[cfg(target_os = "windows")]
fn resolve_windows_app_path(spec: &WindowsWorkspaceOpenAppSpec) -> Option<PathBuf> {
    if let Some(path) = find_windows_relative_candidate(windows_search_roots(), spec.relative_paths)
    {
        return Some(path);
    }

    if let Some(path) = find_windows_versioned_candidate(
        windows_search_roots(),
        spec.versioned_directory_roots,
        spec.versioned_directory_prefixes,
        spec.versioned_executable_suffix,
    ) {
        return Some(path);
    }

    if let Some(path) = find_windows_toolbox_candidate(
        spec.toolbox_product_dirs,
        spec.toolbox_channel_dirs,
        spec.versioned_executable_suffix,
    ) {
        return Some(path);
    }

    spec.executable_name
        .and_then(find_executable_on_path)
        .map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn resolve_windows_icon_path(spec: &WindowsWorkspaceOpenAppSpec) -> Option<PathBuf> {
    if let Some(path) =
        find_windows_relative_candidate(windows_search_roots(), spec.icon_relative_paths)
    {
        return Some(path);
    }

    resolve_windows_app_path(spec)
}

#[cfg(target_os = "windows")]
fn find_windows_relative_candidate(
    roots: Vec<PathBuf>,
    relative_paths: &'static [&'static str],
) -> Option<PathBuf> {
    for root in roots {
        for relative_path in relative_paths {
            let candidate = root.join(relative_path);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn find_windows_versioned_candidate(
    roots: Vec<PathBuf>,
    directory_roots: &'static [&'static str],
    directory_prefixes: &'static [&'static str],
    executable_suffix: Option<&'static str>,
) -> Option<PathBuf> {
    let executable_suffix = executable_suffix?;

    for root in roots {
        for directory_root in directory_roots {
            let search_root = root.join(directory_root);
            let entries = match fs::read_dir(&search_root) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries.filter_map(Result::ok) {
                let candidate_root = entry.path();
                let Some(entry_name) = candidate_root.file_name().and_then(|value| value.to_str())
                else {
                    continue;
                };
                if !directory_prefixes
                    .iter()
                    .any(|prefix| entry_name.starts_with(prefix))
                {
                    continue;
                }

                let candidate = candidate_root.join(executable_suffix);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn find_windows_toolbox_candidate(
    product_dirs: &'static [&'static str],
    channel_dirs: &'static [&'static str],
    executable_suffix: Option<&'static str>,
) -> Option<PathBuf> {
    let executable_suffix = executable_suffix?;
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let toolbox_root = PathBuf::from(local_app_data)
        .join("JetBrains")
        .join("Toolbox")
        .join("apps");

    for product_dir in product_dirs {
        for channel_dir in channel_dirs {
            let channel_root = toolbox_root.join(product_dir).join(channel_dir);
            let entries = match fs::read_dir(&channel_root) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries.filter_map(Result::ok) {
                let candidate = entry.path().join(executable_suffix);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn windows_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    for env_name in [
        "WINDIR",
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ] {
        if let Some(value) = std::env::var_os(env_name) {
            roots.push(PathBuf::from(value));
        }
    }

    roots
}

#[cfg(target_os = "windows")]
fn windows_system_root() -> Option<PathBuf> {
    std::env::var_os("WINDIR").map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn find_executable_on_path(executable_name: &str) -> Option<String> {
    let path_value = std::env::var_os("PATH")?;

    for path in std::env::split_paths(&path_value) {
        let candidate = path.join(executable_name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn open_workspace_in_app_windows(
    target_path: &str,
    app_id: &str,
    app_path: Option<&str>,
) -> Result<(), String> {
    let normalized_target_path = normalize_windows_target_path(target_path);
    if app_id == "powershell" {
        return open_workspace_in_powershell_windows(&normalized_target_path, app_path);
    }

    let mut command = if app_id == "explorer" {
        Command::new("explorer.exe")
    } else if let Some(app_path) = app_path {
        Command::new(app_path)
    } else {
        return Err("The selected app is not available on this system.".into());
    };

    if app_id != "explorer" {
        command.current_dir(&normalized_target_path);
    }

    let (launch_behavior, force_new_console) = WINDOWS_WORKSPACE_OPEN_APP_SPECS
        .iter()
        .find(|spec| spec.id == app_id)
        .map(|spec| (spec.launch_behavior, spec.force_new_console))
        .unwrap_or((WindowsLaunchBehavior::PathArgument, false));

    if app_id == "powershell" {
        let escaped_target_path = powershell_single_quote(&normalized_target_path);
        let command_text = format!("Set-Location -LiteralPath '{escaped_target_path}'");
        command.args(["-NoExit", "-Command", &command_text]);
    } else if launch_behavior != WindowsLaunchBehavior::WorkingDirectoryOnly {
        command.arg(&normalized_target_path);
    }

    if force_new_console {
        command.creation_flags(CREATE_NEW_CONSOLE);
    }

    command.spawn().map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn open_tree_path_in_app_windows(
    target_path: &str,
    is_directory: bool,
    app_id: &str,
    app_path: Option<&str>,
) -> Result<(), String> {
    let behavior = tree_path_open_behavior_windows(app_id, is_directory);

    match behavior {
        TreePathOpenBehavior::OpenTarget => {
            open_workspace_in_app_windows(target_path, app_id, app_path)
        }
        TreePathOpenBehavior::RevealTarget => reveal_tree_path_in_explorer_windows(target_path),
        TreePathOpenBehavior::OpenContainingFolder => {
            let folder_path = containing_directory_path(target_path)?;
            open_workspace_in_app_windows(&folder_path, app_id, app_path)
        }
    }
}

#[cfg(target_os = "windows")]
fn tree_path_open_behavior_windows(app_id: &str, is_directory: bool) -> TreePathOpenBehavior {
    if is_directory {
        return TreePathOpenBehavior::OpenTarget;
    }

    match app_id {
        "explorer" => TreePathOpenBehavior::RevealTarget,
        "powershell" | "git-bash" => TreePathOpenBehavior::OpenContainingFolder,
        _ => TreePathOpenBehavior::OpenTarget,
    }
}

#[cfg(target_os = "windows")]
fn reveal_tree_path_in_explorer_windows(target_path: &str) -> Result<(), String> {
    let normalized_target_path = normalize_windows_target_path(target_path);
    Command::new("explorer.exe")
        .arg(format!("/select,{normalized_target_path}"))
        .spawn()
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn open_workspace_in_powershell_windows(
    normalized_target_path: &str,
    app_path: Option<&str>,
) -> Result<(), String> {
    let escaped_target_path = powershell_single_quote(normalized_target_path);
    let command_text = format!("Set-Location -LiteralPath '{escaped_target_path}'");
    let power_shell_path = app_path.unwrap_or("powershell.exe");
    let mut command = Command::new("cmd.exe");
    command.current_dir(normalized_target_path);
    command.args(["/c", "start", "", "/D", normalized_target_path]);
    command.arg(power_shell_path);
    command.args(["-NoExit", "-Command", &command_text]);
    command.spawn().map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn normalize_windows_target_path(target_path: &str) -> String {
    // Use dunce to canonicalize without the \\?\ UNC prefix
    let normalized =
        dunce::canonicalize(target_path).unwrap_or_else(|_| PathBuf::from(target_path));

    normalized.to_string_lossy().replace('/', "\\")
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn containing_directory_path(target_path: &str) -> Result<String, String> {
    let path = PathBuf::from(target_path);

    let directory = if path.is_dir() {
        path
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "The selected file does not have a containing folder.".to_string())?
    };

    Ok(directory.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::containing_directory_path;
    use std::fs;

    #[test]
    fn containing_directory_uses_parent_for_files() {
        let tempdir = tempfile::tempdir().expect("should create tempdir");
        let src_dir = tempdir.path().join("src");
        fs::create_dir_all(&src_dir).expect("should create src dir");
        let file_path = src_dir.join("main.rs");
        fs::write(&file_path, "fn main() {}\n").expect("should write file");

        let directory = containing_directory_path(&file_path.to_string_lossy())
            .expect("file path should resolve to its parent directory");
        assert_eq!(directory, src_dir.to_string_lossy());
    }

    #[test]
    fn containing_directory_keeps_directories() {
        let tempdir = tempfile::tempdir().expect("should create tempdir");
        let src_dir = tempdir.path().join("src");
        fs::create_dir_all(&src_dir).expect("should create src dir");

        let directory = containing_directory_path(&src_dir.to_string_lossy())
            .expect("directory path should remain unchanged");
        assert_eq!(directory, src_dir.to_string_lossy());
    }

    #[test]
    fn containing_directory_returns_empty_parent_for_bare_relative_file_names() {
        let directory = containing_directory_path("main.rs")
            .expect("bare relative file names resolve to an empty parent path");

        assert_eq!(directory, "");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn build_open_with_omits_finder_and_keeps_other_app_paths() {
        use super::{build_open_with, WorkspaceOpenAppSpec};
        use std::path::Path;

        let finder = WorkspaceOpenAppSpec {
            id: "finder",
            name: "Finder",
            bundle_names: &["Finder.app"],
            preferred_paths: &[],
        };
        let cursor = WorkspaceOpenAppSpec {
            id: "cursor",
            name: "Cursor",
            bundle_names: &["Cursor.app"],
            preferred_paths: &[],
        };

        assert_eq!(
            build_open_with(&finder, Path::new("/System/Finder.app")),
            None
        );
        assert_eq!(
            build_open_with(&cursor, Path::new("/Applications/Cursor.app")),
            Some("/Applications/Cursor.app".to_string())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn find_workspace_open_app_spec_returns_known_apps_only() {
        use super::find_workspace_open_app_spec;

        let spec = find_workspace_open_app_spec("vscode").expect("vscode spec should exist");
        assert_eq!(spec.name, "VS Code");
        assert_eq!(spec.bundle_names, &["Visual Studio Code.app"]);
        assert!(find_workspace_open_app_spec("missing-editor").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn percent_encode_uri_component_keeps_path_separators_and_unreserved_bytes() {
        use super::percent_encode_uri_component;

        assert_eq!(
            percent_encode_uri_component("/Users/me/My Project/你好?x=1&y=two"),
            "/Users/me/My%20Project/%E4%BD%A0%E5%A5%BD%3Fx%3D1%26y%3Dtwo"
        );
        assert_eq!(
            percent_encode_uri_component("AZaz09-_.~/nested"),
            "AZaz09-_.~/nested"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn find_bundle_in_dir_recurses_but_skips_nested_app_bundles() {
        use super::find_bundle_in_dir;

        let tempdir = tempfile::tempdir().expect("should create tempdir");
        let root = tempdir.path();
        let setapp_dir = root.join("Setapp");
        let nested_app = root.join("Container.app").join("Contents");
        let expected = setapp_dir.join("Cursor.app");
        let ignored_inside_app = nested_app.join("Cursor.app");
        fs::create_dir_all(&expected).expect("should create expected app bundle");
        fs::create_dir_all(&ignored_inside_app).expect("should create ignored nested app bundle");

        assert_eq!(find_bundle_in_dir(root, &["Cursor.app"], 3), Some(expected));
        assert_eq!(find_bundle_in_dir(root, &["Missing.app"], 3), None);
        assert_eq!(find_bundle_in_dir(root, &["Cursor.app"], 0), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolve_icon_path_prefers_plist_icon_then_falls_back_to_any_icns() {
        use super::resolve_icon_path;

        let tempdir = tempfile::tempdir().expect("should create tempdir");
        let app = tempdir.path().join("Demo.app");
        let contents = app.join("Contents");
        let resources = contents.join("Resources");
        fs::create_dir_all(&resources).expect("should create resources");
        fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleIconFile</key><string>PrimaryIcon</string></dict></plist>"#,
        )
        .expect("should write plist");
        let primary = resources.join("PrimaryIcon.icns");
        fs::write(&primary, "primary").expect("should write primary icon");
        let fallback = resources.join("Fallback.icns");
        fs::write(&fallback, "fallback").expect("should write fallback icon");

        assert_eq!(resolve_icon_path(&app), Some(primary.clone()));
        fs::remove_file(&primary).expect("should remove primary icon");
        assert_eq!(resolve_icon_path(&app), Some(fallback));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_tree_path_behavior_matches_app_capabilities() {
        use super::{tree_path_open_behavior_macos, TreePathOpenBehavior};

        assert_eq!(
            tree_path_open_behavior_macos("finder", false),
            TreePathOpenBehavior::RevealTarget
        );
        assert_eq!(
            tree_path_open_behavior_macos("terminal", false),
            TreePathOpenBehavior::OpenContainingFolder
        );
        assert_eq!(
            tree_path_open_behavior_macos("cursor", false),
            TreePathOpenBehavior::OpenTarget
        );
        assert_eq!(
            tree_path_open_behavior_macos("finder", true),
            TreePathOpenBehavior::OpenTarget
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_tree_path_behavior_matches_app_capabilities() {
        use super::{tree_path_open_behavior_windows, TreePathOpenBehavior};

        assert_eq!(
            tree_path_open_behavior_windows("explorer", false),
            TreePathOpenBehavior::RevealTarget
        );
        assert_eq!(
            tree_path_open_behavior_windows("powershell", false),
            TreePathOpenBehavior::OpenContainingFolder
        );
        assert_eq!(
            tree_path_open_behavior_windows("cursor", false),
            TreePathOpenBehavior::OpenTarget
        );
        assert_eq!(
            tree_path_open_behavior_windows("explorer", true),
            TreePathOpenBehavior::OpenTarget
        );
    }
}

#[cfg(target_os = "windows")]
fn build_windows_icon_data_url(executable_path: &Path, app_id: &str) -> Option<String> {
    let file_name = format!(
        "tiy-workspace-open-app-{app_id}-{}.png",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis()
    );
    let output_path = std::env::temp_dir().join(file_name);
    let escaped_input = powershell_single_quote(&executable_path.to_string_lossy());
    let escaped_output = powershell_single_quote(&output_path.to_string_lossy());
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         Add-Type -AssemblyName System.Drawing; \
         $icon=[System.Drawing.Icon]::ExtractAssociatedIcon('{escaped_input}'); \
         if ($null -eq $icon) {{ throw 'No icon found.' }}; \
         $bitmap=$icon.ToBitmap(); \
         try {{ $bitmap.Save('{escaped_output}', [System.Drawing.Imaging.ImageFormat]::Png) }} \
         finally {{ $bitmap.Dispose(); $icon.Dispose() }}"
    );

    let mut command = Command::new("powershell.exe");
    configure_background_std_command(&mut command);
    let status = command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()
        .ok()?;

    if !status.success() {
        let _ = fs::remove_file(&output_path);
        return None;
    }

    let icon_bytes = fs::read(&output_path).ok()?;
    let _ = fs::remove_file(&output_path);

    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(icon_bytes)
    ))
}

#[cfg(target_os = "windows")]
fn powershell_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(target_os = "macos")]
fn find_app_bundle(spec: &WorkspaceOpenAppSpec) -> Option<PathBuf> {
    for preferred_path in spec.preferred_paths {
        let path = PathBuf::from(preferred_path);
        if path.exists() {
            return Some(path);
        }
    }

    macos_search_roots()
        .into_iter()
        .find_map(|root| find_bundle_in_dir(&root, spec.bundle_names, 3))
}

#[cfg(target_os = "macos")]
fn macos_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Setapp"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Library/CoreServices"),
    ];

    if let Some(home_dir) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home_dir).join("Applications"));
    }

    roots
}

#[cfg(target_os = "macos")]
fn find_bundle_in_dir(
    root: &Path,
    bundle_names: &'static [&'static str],
    depth: usize,
) -> Option<PathBuf> {
    if depth == 0 || !root.exists() {
        return None;
    }

    for entry in fs::read_dir(root).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        let file_type = entry.file_type().ok()?;

        if !file_type.is_dir() {
            continue;
        }

        let file_name = path.file_name().and_then(|value| value.to_str());
        if bundle_names
            .iter()
            .any(|bundle_name| file_name == Some(*bundle_name))
        {
            return Some(path);
        }

        if path.extension().and_then(|value| value.to_str()) == Some("app") {
            continue;
        }

        if let Some(found) = find_bundle_in_dir(&path, bundle_names, depth.saturating_sub(1)) {
            return Some(found);
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn resolve_icon_path(app_path: &Path) -> Option<PathBuf> {
    let plist_path = app_path.join("Contents/Info.plist");
    let resources_path = app_path.join("Contents/Resources");

    let icon_name = read_plist_value(&plist_path, "CFBundleIconFile")
        .or_else(|| read_plist_value(&plist_path, "CFBundleIconName"))?;

    let normalized_icon_name = if Path::new(&icon_name).extension().is_some() {
        icon_name.clone()
    } else {
        format!("{icon_name}.icns")
    };

    let direct_icon_path = resources_path.join(&normalized_icon_name);
    if direct_icon_path.exists() {
        return Some(direct_icon_path);
    }

    let fallback_icon_path = resources_path.join(format!("{icon_name}.icns"));
    if fallback_icon_path.exists() {
        return Some(fallback_icon_path);
    }

    fs::read_dir(resources_path)
        .ok()?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("icns"))
}

#[cfg(target_os = "macos")]
fn read_plist_value(plist_path: &Path, key: &str) -> Option<String> {
    let output = Command::new("plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(target_os = "macos")]
fn build_icon_data_url(icon_path: &Path, app_id: &str) -> Option<String> {
    let file_name = format!(
        "tiy-workspace-open-app-{app_id}-{}.png",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis()
    );
    let output_path = std::env::temp_dir().join(file_name);

    let conversion_status = Command::new("sips")
        .args(["-z", "64", "64", "-s", "format", "png"])
        .arg(icon_path)
        .args(["--out"])
        .arg(&output_path)
        .status()
        .ok()?;

    if !conversion_status.success() {
        let _ = fs::remove_file(&output_path);
        return None;
    }

    let icon_bytes = fs::read(&output_path).ok()?;
    let _ = fs::remove_file(&output_path);

    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(icon_bytes)
    ))
}

#[cfg(target_os = "macos")]
fn percent_encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());

    for byte in value.bytes() {
        let is_unreserved = matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/');
        if is_unreserved {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }

    encoded
}
