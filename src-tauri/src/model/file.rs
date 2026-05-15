use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContentDto {
    pub content: String,
    pub size_bytes: u64,
    pub is_binary: bool,
}
