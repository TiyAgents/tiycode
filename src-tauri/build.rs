use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PATH");
    println!("cargo:rerun-if-env-changed=SHELL");
    println!("cargo:rerun-if-env-changed=TIY_BUILD_RG_PATH");

    bundle_ripgrep().expect("failed to prepare bundled ripgrep");
    ensure_bundled_catalog_placeholder();
    tauri_build::build()
}

/// Ensure the `bundled-catalog/` directory exists with at least placeholder
/// files so that the Tauri resource bundler does not fail during local dev
/// builds. In CI the real catalog files are downloaded before the build step
/// and will already be present, so the placeholders are never created there.
fn ensure_bundled_catalog_placeholder() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let catalog_dir = manifest_dir.join("bundled-catalog");
    fs::create_dir_all(&catalog_dir).ok();

    let manifest_file = catalog_dir.join("catalog.manifest.json");
    if !manifest_file.exists() {
        let _ = fs::write(&manifest_file, r#"{"version":"0"}"#);
    }
    let catalog_file = catalog_dir.join("catalog.json");
    if !catalog_file.exists() {
        let _ = fs::write(&catalog_file, "{}");
    }
}

fn bundle_ripgrep() -> io::Result<()> {
    let target = env::var("TARGET")
        .map_err(|error| io::Error::other(format!("missing TARGET env var: {error}")))?;
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| io::Error::other(format!("missing CARGO_MANIFEST_DIR: {error}")))?,
    );

    let rg_source = locate_rg().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "ripgrep ('rg') is required at build time so TiyCode can bundle search support",
        )
    })?;

    let destination = manifest_dir
        .join("target")
        .join("tiy-rg")
        .join(format!("rg-{target}{}", executable_suffix()));

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    prepare_destination_for_copy(&destination)?;
    fs::copy(&rg_source, &destination)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::metadata(&rg_source)?.permissions().mode();
        fs::set_permissions(&destination, fs::Permissions::from_mode(permissions))?;
    }

    println!("cargo:rerun-if-changed={}", rg_source.display());
    Ok(())
}

fn prepare_destination_for_copy(destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return Ok(());
    }

    make_writable_if_needed(destination)?;
    fs::remove_file(destination)
}

fn make_writable_if_needed(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    let mut permissions = metadata.permissions();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = permissions.mode();
        if mode & 0o200 == 0 {
            permissions.set_mode(mode | 0o200);
            fs::set_permissions(path, permissions)?;
        }
    }

    #[cfg(windows)]
    {
        if permissions.readonly() {
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
        }
    }

    Ok(())
}

fn locate_rg() -> Option<PathBuf> {
    find_env_override("TIY_BUILD_RG_PATH")
        .or_else(find_on_path)
        .or_else(find_common_locations)
}

fn find_env_override(name: &str) -> Option<PathBuf> {
    let value = env::var_os(name)?;
    let path = PathBuf::from(value);
    is_executable_file(&path).then_some(path)
}

fn find_on_path() -> Option<PathBuf> {
    let path_value = env::var_os("PATH")?;
    env::split_paths(&path_value)
        .map(|dir| dir.join(executable_name()))
        .find(|candidate| is_executable_file(candidate))
}

fn find_common_locations() -> Option<PathBuf> {
    common_rg_locations()
        .into_iter()
        .find(|candidate| is_executable_file(candidate))
}

fn common_rg_locations() -> Vec<PathBuf> {
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

fn executable_suffix() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        ".exe"
    }

    #[cfg(not(target_os = "windows"))]
    {
        ""
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
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
