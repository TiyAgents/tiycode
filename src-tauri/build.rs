use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
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
