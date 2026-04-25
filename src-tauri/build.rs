use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    inject_build_commit_hash();
    ensure_bundled_catalog_placeholder();
    tauri_build::build()
}

/// When building locally (outside CI), embed the short git commit hash as a
/// compile-time environment variable so the app can display a dev version like
/// `0.1.0-dev.705f212`. In CI the `CI` env var is always set, so this step is
/// skipped and the version stays exactly as written in Cargo.toml (which CI
/// patches to the release tag before building).
fn inject_build_commit_hash() {
    // Re-run this build script if the HEAD ref changes (new commit / checkout).
    println!("cargo:rerun-if-changed=.git/HEAD");
    // Also track the ref file so branch switches trigger a rebuild.
    if let Ok(head) = std::fs::read_to_string(
        std::path::Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
            .parent()
            .unwrap()
            .join(".git/HEAD"),
    ) {
        if let Some(ref_path) = head.strip_prefix("ref: ") {
            let ref_path = ref_path.trim();
            println!("cargo:rerun-if-changed=.git/{ref_path}");
        }
    }

    // Skip in CI — release builds set the version via sed before invoking cargo.
    if env::var("CI").is_ok() {
        return;
    }

    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !hash.is_empty() {
                println!("cargo:rustc-env=BUILD_COMMIT_HASH={hash}");
            }
        }
    }
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
