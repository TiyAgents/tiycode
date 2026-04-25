use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use tokio::fs;

use crate::model::errors::{AppError, ErrorSource};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFileDto {
    pub name: String,
    pub media_type: String,
    pub data_url: String,
}

#[tauri::command]
pub async fn attachment_read_files(
    paths: Vec<String>,
    max_bytes: Option<u64>,
) -> Result<Vec<AttachmentFileDto>, AppError> {
    let mut files = Vec::new();

    for raw_path in paths {
        let path = Path::new(&raw_path);
        let metadata = fs::metadata(path).await.map_err(|error| {
            AppError::internal(
                ErrorSource::System,
                format!("failed to read attachment metadata: {error}"),
            )
        })?;

        if !metadata.is_file() {
            return Err(AppError::validation(
                ErrorSource::System,
                "Only files can be attached.",
            ));
        }

        if let Some(limit) = max_bytes {
            if metadata.len() > limit {
                return Err(AppError::validation(
                    ErrorSource::System,
                    format!(
                        "The selected file exceeds the maximum supported size of {} bytes.",
                        limit
                    ),
                ));
            }
        }

        let Some(name) = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
        else {
            return Err(AppError::validation(
                ErrorSource::System,
                "Failed to resolve the selected attachment file name.",
            ));
        };

        let Some(media_type) = attachment_media_type(path) else {
            return Err(AppError::validation(
                ErrorSource::System,
                format!("Unsupported attachment type for '{name}'."),
            ));
        };

        let bytes = fs::read(path).await.map_err(|error| {
            AppError::internal(
                ErrorSource::System,
                format!("failed to read attachment file: {error}"),
            )
        })?;
        let data_url = format!("data:{};base64,{}", media_type, STANDARD.encode(bytes));

        files.push(AttachmentFileDto {
            name,
            media_type: media_type.to_string(),
            data_url,
        });
    }

    Ok(files)
}

fn attachment_media_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("bmp") => Some("image/bmp"),
        Some("svg") => Some("image/svg+xml"),
        Some("md") => Some("text/markdown"),
        Some("txt") => Some("text/plain"),
        Some("json") => Some("application/json"),
        Some("js" | "jsx") => Some("text/javascript"),
        Some("ts") => Some("application/typescript"),
        Some("tsx") => Some("text/tsx"),
        Some("py") => Some("text/x-python"),
        Some("go") => Some("text/x-go"),
        Some("rs") => Some("text/x-rust"),
        Some("java") => Some("text/x-java-source"),
        Some("c" | "h") => Some("text/x-c"),
        Some("cc" | "cpp" | "cxx" | "hpp" | "hh") => Some("text/x-c++"),
        Some("yaml" | "yml") => Some("application/yaml"),
        Some("toml") => Some("application/toml"),
        Some("ini" | "conf" | "cfg" | "env" | "properties") => Some("text/plain"),
        Some("xml") => Some("application/xml"),
        Some("html") => Some("text/html"),
        Some("css" | "scss" | "less") => Some("text/css"),
        Some("sql") => Some("application/sql"),
        Some("sh" | "bash" | "zsh") => Some("application/x-sh"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{attachment_media_type, attachment_read_files};
    use std::path::Path;

    #[test]
    fn attachment_media_type_maps_supported_extensions_case_insensitively() {
        for (file_name, expected) in [
            ("image.PNG", Some("image/png")),
            ("photo.jpeg", Some("image/jpeg")),
            ("anim.GIF", Some("image/gif")),
            ("doc.md", Some("text/markdown")),
            ("script.tsx", Some("text/tsx")),
            ("main.rs", Some("text/x-rust")),
            ("config.yaml", Some("application/yaml")),
            ("style.scss", Some("text/css")),
            ("run.zsh", Some("application/x-sh")),
            ("archive.zip", None),
            ("README", None),
        ] {
            assert_eq!(attachment_media_type(Path::new(file_name)), expected);
        }
    }

    #[tokio::test]
    async fn attachment_read_files_encodes_supported_files_as_data_urls() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let text_path = tempdir.path().join("note.txt");
        let json_path = tempdir.path().join("data.json");
        tokio::fs::write(&text_path, b"hello")
            .await
            .expect("text file");
        tokio::fs::write(&json_path, br#"{"ok":true}"#)
            .await
            .expect("json file");

        let files = attachment_read_files(
            vec![
                text_path.to_string_lossy().to_string(),
                json_path.to_string_lossy().to_string(),
            ],
            Some(64),
        )
        .await
        .expect("attachments should load");

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "note.txt");
        assert_eq!(files[0].media_type, "text/plain");
        assert_eq!(files[0].data_url, "data:text/plain;base64,aGVsbG8=");
        assert_eq!(files[1].name, "data.json");
        assert_eq!(files[1].media_type, "application/json");
        assert!(files[1]
            .data_url
            .starts_with("data:application/json;base64,"));
    }

    #[tokio::test]
    async fn attachment_read_files_rejects_directories_oversized_and_unsupported_files() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let directory_error =
            attachment_read_files(vec![tempdir.path().to_string_lossy().to_string()], Some(64))
                .await
                .expect_err("directories cannot be attached");
        assert_eq!(directory_error.error_code, "system.validation");
        assert_eq!(directory_error.user_message, "Only files can be attached.");

        let large_path = tempdir.path().join("large.txt");
        tokio::fs::write(&large_path, b"too large")
            .await
            .expect("large file");
        let large_error =
            attachment_read_files(vec![large_path.to_string_lossy().to_string()], Some(3))
                .await
                .expect_err("oversized files should fail");
        assert!(large_error
            .user_message
            .contains("maximum supported size of 3 bytes"));

        let unsupported_path = tempdir.path().join("archive.zip");
        tokio::fs::write(&unsupported_path, b"zip")
            .await
            .expect("unsupported file");
        let unsupported_error =
            attachment_read_files(vec![unsupported_path.to_string_lossy().to_string()], None)
                .await
                .expect_err("unsupported files should fail");
        assert_eq!(
            unsupported_error.user_message,
            "Unsupported attachment type for 'archive.zip'."
        );
    }
}
