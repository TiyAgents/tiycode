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
