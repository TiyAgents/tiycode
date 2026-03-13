use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

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
    SystemMetadata {
        app_name: "Tiy Agent".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        runtime: "Tauri 2".into(),
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
pub fn open_workspace_in_app(target_path: String, app_path: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return open_workspace_in_app_macos(&target_path, app_path.as_deref());
    }

    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "windows")]
        {
            return open_workspace_in_app_windows(&target_path, app_path.as_deref());
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (target_path, app_path);
            Err(
                "Opening workspace folders in external apps is only supported on macOS and Windows right now."
                    .into(),
            )
        }
    }
}

#[cfg(target_os = "macos")]
struct WorkspaceOpenAppSpec {
    id: &'static str,
    name: &'static str,
    bundle_name: &'static str,
    preferred_paths: &'static [&'static str],
}

#[cfg(target_os = "macos")]
const WORKSPACE_OPEN_APP_SPECS: [WorkspaceOpenAppSpec; 5] = [
    WorkspaceOpenAppSpec {
        id: "finder",
        name: "Finder",
        bundle_name: "Finder.app",
        preferred_paths: &["/System/Library/CoreServices/Finder.app"],
    },
    WorkspaceOpenAppSpec {
        id: "vscode",
        name: "VS Code",
        bundle_name: "Visual Studio Code.app",
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "cursor",
        name: "Cursor",
        bundle_name: "Cursor.app",
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "windsurf",
        name: "Windsurf",
        bundle_name: "Windsurf.app",
        preferred_paths: &[],
    },
    WorkspaceOpenAppSpec {
        id: "zed",
        name: "Zed",
        bundle_name: "Zed.app",
        preferred_paths: &[],
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
fn open_workspace_in_app_macos(target_path: &str, app_path: Option<&str>) -> Result<(), String> {
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

#[cfg(target_os = "windows")]
struct WindowsWorkspaceOpenAppSpec {
    id: &'static str,
    name: &'static str,
    executable_name: Option<&'static str>,
    relative_paths: &'static [&'static str],
    icon_relative_paths: &'static [&'static str],
}

#[cfg(target_os = "windows")]
const WINDOWS_WORKSPACE_OPEN_APP_SPECS: [WindowsWorkspaceOpenAppSpec; 5] = [
    WindowsWorkspaceOpenAppSpec {
        id: "explorer",
        name: "Explorer",
        executable_name: None,
        relative_paths: &[],
        icon_relative_paths: &[],
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
    },
    WindowsWorkspaceOpenAppSpec {
        id: "cursor",
        name: "Cursor",
        executable_name: Some("Cursor.exe"),
        relative_paths: &["Cursor\\Cursor.exe", "Programs\\Cursor\\Cursor.exe"],
        icon_relative_paths: &["Cursor\\Cursor.exe", "Programs\\Cursor\\Cursor.exe"],
    },
    WindowsWorkspaceOpenAppSpec {
        id: "windsurf",
        name: "Windsurf",
        executable_name: Some("Windsurf.exe"),
        relative_paths: &["Windsurf\\Windsurf.exe", "Programs\\Windsurf\\Windsurf.exe"],
        icon_relative_paths: &["Windsurf\\Windsurf.exe", "Programs\\Windsurf\\Windsurf.exe"],
    },
    WindowsWorkspaceOpenAppSpec {
        id: "zed",
        name: "Zed",
        executable_name: Some("Zed.exe"),
        relative_paths: &["Zed\\Zed.exe", "Programs\\Zed\\Zed.exe"],
        icon_relative_paths: &["Zed\\Zed.exe", "Programs\\Zed\\Zed.exe"],
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

    let candidate_roots = windows_search_roots();

    for root in candidate_roots {
        for relative_path in spec.relative_paths {
            let candidate = root.join(relative_path);
            if candidate.exists() {
                return Some(Some(candidate.to_string_lossy().into_owned()));
            }
        }
    }

    let executable_name = spec.executable_name?;
    find_executable_on_path(executable_name).map(Some)
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

    let candidate_roots = windows_search_roots();

    for root in candidate_roots {
        for relative_path in spec.icon_relative_paths {
            let candidate = root.join(relative_path);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let executable_name = spec.executable_name?;
    find_executable_on_path(executable_name).map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn windows_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    for env_name in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
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
fn open_workspace_in_app_windows(target_path: &str, app_path: Option<&str>) -> Result<(), String> {
    let normalized_target_path = normalize_windows_target_path(target_path);
    let mut command = if let Some(app_path) = app_path {
        Command::new(app_path)
    } else {
        Command::new("explorer.exe")
    };

    command.arg(&normalized_target_path);
    command.spawn().map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn normalize_windows_target_path(target_path: &str) -> String {
    let normalized = PathBuf::from(target_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(target_path));

    let mut value = normalized.to_string_lossy().replace('/', "\\");

    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_string();
    }

    value
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

    let status = Command::new("powershell.exe")
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
        .find_map(|root| find_bundle_in_dir(&root, spec.bundle_name, 3))
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
fn find_bundle_in_dir(root: &Path, bundle_name: &str, depth: usize) -> Option<PathBuf> {
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

        if path.file_name().and_then(|value| value.to_str()) == Some(bundle_name) {
            return Some(path);
        }

        if path.extension().and_then(|value| value.to_str()) == Some("app") {
            continue;
        }

        if let Some(found) = find_bundle_in_dir(&path, bundle_name, depth.saturating_sub(1)) {
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
