//! WeChat CdnMedia structured message types and multimedia send/receive logic.
//!
//! Implements CDN-based media upload/download for the WeChat iLink Bot API,
//! including AES-256-CBC encryption for outbound media and download URL
//! construction for inbound media items.

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use md5::{Digest, Md5};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Message item type: text
pub const MESSAGE_ITEM_TEXT: i32 = 1;
/// Message item type: image
pub const MESSAGE_ITEM_IMAGE: i32 = 2;
/// Message item type: voice
pub const MESSAGE_ITEM_VOICE: i32 = 3;
/// Message item type: file
pub const MESSAGE_ITEM_FILE: i32 = 4;
/// Message item type: video
pub const MESSAGE_ITEM_VIDEO: i32 = 5;

/// Upload media type for images
pub const UPLOAD_MEDIA_TYPE_IMAGE: i32 = 1;
/// Upload media type for videos
pub const UPLOAD_MEDIA_TYPE_VIDEO: i32 = 2;
/// Upload media type for files
pub const UPLOAD_MEDIA_TYPE_FILE: i32 = 3;

// ---------------------------------------------------------------------------
// CdnMedia — core CDN resource locator and decryption info
// ---------------------------------------------------------------------------

/// CDN media resource descriptor. Shared by all media item types.
/// Contains the encrypted query parameter for CDN URL construction and
/// the AES key for content decryption.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CdnMedia {
    /// Encrypted query parameter used in CDN download/upload URLs.
    #[serde(default)]
    pub encrypt_query_param: Option<String>,
    /// AES decryption key (base64-encoded).
    #[serde(default)]
    pub aes_key: Option<String>,
    /// Encryption type (1 = AES-256-CBC).
    #[serde(default)]
    pub encrypt_type: Option<i32>,
}

// ---------------------------------------------------------------------------
// Media Item types — correspond to WeChat message item_list entries
// ---------------------------------------------------------------------------

/// Image media item, with main image and optional thumbnail.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageItem {
    #[serde(default)]
    pub media: Option<CdnMedia>,
    #[serde(default)]
    pub thumb_media: Option<CdnMedia>,
    /// Alternative AES key field (takes priority over media.aes_key for images).
    #[serde(default)]
    pub aeskey: Option<String>,
    #[serde(default)]
    pub mid_size: Option<String>,
    #[serde(default)]
    pub thumb_size: Option<String>,
}

/// Voice media item.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VoiceItem {
    #[serde(default)]
    pub media: Option<CdnMedia>,
    /// Encoding type: 7=mp3, 8=ogg, 5=amr, 6/other=silk
    #[serde(default)]
    pub encode_type: Option<i32>,
    /// Duration in seconds.
    #[serde(default)]
    pub playtime: Option<i32>,
    /// Voice-to-text transcription.
    #[serde(default)]
    pub text: Option<String>,
}

/// File media item.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileItem {
    #[serde(default)]
    pub media: Option<CdnMedia>,
    #[serde(default)]
    pub file_name: Option<String>,
    /// File size (string in protocol, parsed to u64).
    #[serde(default)]
    pub len: Option<String>,
}

/// Video media item, with main video and thumbnail.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VideoItem {
    #[serde(default)]
    pub media: Option<CdnMedia>,
    #[serde(default)]
    pub thumb_media: Option<CdnMedia>,
    #[serde(default)]
    pub video_size: Option<String>,
    #[serde(default)]
    pub thumb_size: Option<String>,
    #[serde(default)]
    pub play_length: Option<i32>,
}

/// Text item (for completeness in MessageItem union).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextItem {
    #[serde(default)]
    pub text: Option<String>,
}

/// Union-style message item: the `type` field determines which `*_item` is populated.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageItem {
    #[serde(rename = "type", default)]
    pub item_type: i32,
    #[serde(default)]
    pub text_item: Option<TextItem>,
    #[serde(default)]
    pub image_item: Option<ImageItem>,
    #[serde(default)]
    pub voice_item: Option<VoiceItem>,
    #[serde(default)]
    pub file_item: Option<FileItem>,
    #[serde(default)]
    pub video_item: Option<VideoItem>,
}

// ---------------------------------------------------------------------------
// MediaAttachment — tiycode unified inbound media representation
// ---------------------------------------------------------------------------

/// Type of media attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Image,
    Video,
    Voice,
    File,
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaType::Image => write!(f, "image"),
            MediaType::Video => write!(f, "video"),
            MediaType::Voice => write!(f, "voice"),
            MediaType::File => write!(f, "file"),
        }
    }
}

/// Structured media attachment extracted from inbound WeChat messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachment {
    /// Type of media.
    pub media_type: MediaType,
    /// CDN download URL.
    pub url: String,
    /// Original file name (if available).
    pub file_name: Option<String>,
    /// File size in bytes (if available).
    pub size: Option<u64>,
    /// AES key for decryption (base64).
    pub aes_key: Option<String>,
    /// MIME type.
    pub mime_type: String,
    /// Voice transcription text (voice items only).
    pub transcription: Option<String>,
    /// Duration in seconds (voice/video items).
    pub duration_secs: Option<i32>,
}

// ---------------------------------------------------------------------------
// Inbound: extract media attachments from message item_list
// ---------------------------------------------------------------------------

/// Extract all media attachments from a WeChat message's item_list.
pub fn extract_media_attachments(
    item_list: &[MessageItem],
    cdn_base_url: &str,
) -> Vec<MediaAttachment> {
    let mut attachments = Vec::new();
    for item in item_list {
        match item.item_type {
            MESSAGE_ITEM_IMAGE => {
                if let Some(ref img) = item.image_item {
                    if let Some(att) = map_image_attachment(img, cdn_base_url) {
                        attachments.push(att);
                    }
                }
            }
            MESSAGE_ITEM_VOICE => {
                if let Some(ref voice) = item.voice_item {
                    if let Some(att) = map_voice_attachment(voice, cdn_base_url) {
                        attachments.push(att);
                    }
                }
            }
            MESSAGE_ITEM_FILE => {
                if let Some(ref file) = item.file_item {
                    if let Some(att) = map_file_attachment(file, cdn_base_url) {
                        attachments.push(att);
                    }
                }
            }
            MESSAGE_ITEM_VIDEO => {
                if let Some(ref video) = item.video_item {
                    if let Some(att) = map_video_attachment(video, cdn_base_url) {
                        attachments.push(att);
                    }
                }
            }
            _ => {}
        }
    }
    attachments
}

fn map_image_attachment(img: &ImageItem, cdn_base_url: &str) -> Option<MediaAttachment> {
    let media = img.media.as_ref()?;
    let param = media.encrypt_query_param.as_deref()?;
    let url = build_cdn_download_url(cdn_base_url, param);

    // Image AES key: prefer ImageItem.aeskey, fallback to CdnMedia.aes_key
    let aes_key = img
        .aeskey
        .clone()
        .or_else(|| media.aes_key.clone())
        .filter(|k| !k.is_empty());

    Some(MediaAttachment {
        media_type: MediaType::Image,
        url,
        file_name: None,
        size: img.mid_size.as_deref().and_then(|s| s.parse::<u64>().ok()),
        aes_key,
        mime_type: "image/jpeg".to_string(),
        transcription: None,
        duration_secs: None,
    })
}

fn map_voice_attachment(voice: &VoiceItem, cdn_base_url: &str) -> Option<MediaAttachment> {
    let media = voice.media.as_ref()?;
    let param = media.encrypt_query_param.as_deref()?;
    let url = build_cdn_download_url(cdn_base_url, param);
    let aes_key = media.aes_key.clone().filter(|k| !k.is_empty());
    let mime_type = infer_voice_mime(voice.encode_type).to_string();

    Some(MediaAttachment {
        media_type: MediaType::Voice,
        url,
        file_name: None,
        size: None,
        aes_key,
        mime_type,
        transcription: voice.text.clone().filter(|t| !t.is_empty()),
        duration_secs: voice.playtime,
    })
}

fn map_file_attachment(file: &FileItem, cdn_base_url: &str) -> Option<MediaAttachment> {
    let media = file.media.as_ref()?;
    let param = media.encrypt_query_param.as_deref()?;
    let url = build_cdn_download_url(cdn_base_url, param);
    let aes_key = media.aes_key.clone().filter(|k| !k.is_empty());

    let size = file.len.as_deref().and_then(|s| s.parse::<u64>().ok());
    let file_name = file.file_name.clone().filter(|n| !n.is_empty());

    // Infer MIME from file extension
    let mime_type = file_name
        .as_deref()
        .and_then(infer_mime_from_extension)
        .unwrap_or("application/octet-stream")
        .to_string();

    Some(MediaAttachment {
        media_type: MediaType::File,
        url,
        file_name,
        size,
        aes_key,
        mime_type,
        transcription: None,
        duration_secs: None,
    })
}

fn map_video_attachment(video: &VideoItem, cdn_base_url: &str) -> Option<MediaAttachment> {
    let media = video.media.as_ref()?;
    let param = media.encrypt_query_param.as_deref()?;
    let url = build_cdn_download_url(cdn_base_url, param);
    let aes_key = media.aes_key.clone().filter(|k| !k.is_empty());

    let size = video
        .video_size
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok());

    Some(MediaAttachment {
        media_type: MediaType::Video,
        url,
        file_name: None,
        size,
        aes_key,
        mime_type: "video/mp4".to_string(),
        transcription: None,
        duration_secs: video.play_length,
    })
}

// ---------------------------------------------------------------------------
// URL construction helpers
// ---------------------------------------------------------------------------

/// Build CDN download URL from base URL and encrypted query parameter.
pub fn build_cdn_download_url(cdn_base_url: &str, encrypt_query_param: &str) -> String {
    let base = cdn_base_url.trim_end_matches('/');
    format!(
        "https://{}/download?encrypted_query_param={}",
        base, encrypt_query_param
    )
}

/// Infer voice MIME type from encode_type field.
pub fn infer_voice_mime(encode_type: Option<i32>) -> &'static str {
    match encode_type {
        Some(7) => "audio/mpeg",
        Some(8) => "audio/ogg",
        Some(5) => "audio/amr",
        _ => "audio/silk", // encode_type 6 or other defaults to silk
    }
}

/// Infer MIME type from file extension.
fn infer_mime_from_extension(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit('.').next()?.to_lowercase();
    Some(match ext.as_str() {
        "pdf" => "application/pdf",
        "doc" | "docx" => "application/msword",
        "xls" | "xlsx" => "application/vnd.ms-excel",
        "ppt" | "pptx" => "application/vnd.ms-powerpoint",
        "zip" => "application/zip",
        "rar" => "application/x-rar-compressed",
        "7z" => "application/x-7z-compressed",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    })
}

// ---------------------------------------------------------------------------
// Outbound: CDN upload flow
// ---------------------------------------------------------------------------

/// Result of a successful CDN upload.
#[derive(Debug, Clone)]
pub struct UploadedMedia {
    /// Encrypted query parameter for the download URL (from CDN response header).
    pub download_encrypted_query_param: String,
    /// AES key in base64 (for the recipient to decrypt).
    pub cdn_aes_key_base64: String,
    /// Size of the encrypted ciphertext.
    pub file_size_ciphertext: u64,
    /// Original plaintext size.
    pub plaintext_size: u64,
}

/// Upload media type for the getuploadurl API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadMediaType {
    Image,
    Video,
    File,
}

impl UploadMediaType {
    pub fn api_value(self) -> i32 {
        match self {
            UploadMediaType::Image => UPLOAD_MEDIA_TYPE_IMAGE,
            UploadMediaType::Video => UPLOAD_MEDIA_TYPE_VIDEO,
            UploadMediaType::File => UPLOAD_MEDIA_TYPE_FILE,
        }
    }

    pub fn message_item_type(self) -> i32 {
        match self {
            UploadMediaType::Image => MESSAGE_ITEM_IMAGE,
            UploadMediaType::Video => MESSAGE_ITEM_VIDEO,
            UploadMediaType::File => MESSAGE_ITEM_FILE,
        }
    }
}

impl From<&MediaType> for UploadMediaType {
    fn from(mt: &MediaType) -> Self {
        match mt {
            MediaType::Image => UploadMediaType::Image,
            MediaType::Video => UploadMediaType::Video,
            _ => UploadMediaType::File,
        }
    }
}

// ---------------------------------------------------------------------------
// AES-256-CBC encryption
// ---------------------------------------------------------------------------

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

/// Encrypt media payload with AES-256-CBC + PKCS7 padding.
/// Returns (ciphertext, hex-encoded AES key).
pub fn encrypt_media_payload(data: &[u8]) -> (Vec<u8>, String) {
    let mut key = [0u8; 32];
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut key);
    rand::thread_rng().fill_bytes(&mut iv);

    let padded_size = ((data.len() / 16) + 1) * 16;
    let mut buf = vec![0u8; padded_size];
    buf[..data.len()].copy_from_slice(data);

    let ciphertext = Aes256CbcEnc::new(&key.into(), &iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
        .expect("encryption buffer is correctly sized");

    // Prepend IV to ciphertext (WeChat CDN convention)
    let mut result = Vec::with_capacity(16 + ciphertext.len());
    result.extend_from_slice(&iv);
    result.extend_from_slice(ciphertext);

    let key_hex = hex::encode(key);
    (result, key_hex)
}

/// Calculate the padded ciphertext size for a given plaintext size.
pub fn padded_ciphertext_size(plaintext_size: usize) -> usize {
    // PKCS7 padding always adds at least 1 byte, up to block_size bytes
    ((plaintext_size / 16) + 1) * 16
}

// ---------------------------------------------------------------------------
// CDN upload API interactions
// ---------------------------------------------------------------------------

/// Request body for getuploadurl API.
#[derive(Debug, Serialize)]
struct GetUploadUrlRequest {
    media_type: i32,
    file_size: u64,
    md5: String,
    no_need_thumb: bool,
}

/// Response from getuploadurl API.
#[derive(Debug, Deserialize)]
struct GetUploadUrlResponse {
    #[serde(default)]
    errcode: Option<i32>,
    #[serde(default)]
    errmsg: Option<String>,
    #[serde(default)]
    upload_param: Option<String>,
    #[serde(default)]
    filekey: Option<String>,
}

/// Full CDN upload flow:
/// 1. Encrypt the file data with AES-256-CBC
/// 2. Get upload URL from iLink API
/// 3. Upload ciphertext to CDN
/// 4. Return the download parameters
pub async fn upload_media(
    client: &Client,
    cdn_base_url: &str,
    base_url: &str,
    token: &str,
    headers: &reqwest::header::HeaderMap,
    file_data: &[u8],
    media_type: UploadMediaType,
) -> Result<UploadedMedia> {
    // 1. Encrypt
    let (ciphertext, aes_key_hex) = encrypt_media_payload(file_data);
    let raw_size = file_data.len() as u64;
    let ciphertext_size = ciphertext.len() as u64;

    // Compute MD5 of plaintext
    let raw_md5 = format!("{:x}", Md5::digest(file_data));

    // 2. Get upload URL
    let upload_url_endpoint = format!("https://{}/ilink/bot/getuploadurl", base_url);
    let req_body = GetUploadUrlRequest {
        media_type: media_type.api_value(),
        file_size: ciphertext_size,
        md5: raw_md5,
        no_need_thumb: true,
    };

    let resp = client
        .post(&upload_url_endpoint)
        .headers(headers.clone())
        .bearer_auth(token)
        .json(&req_body)
        .send()
        .await
        .context("failed to request upload URL")?;

    let upload_resp: GetUploadUrlResponse = resp
        .json()
        .await
        .context("failed to parse upload URL response")?;

    if upload_resp.errcode.unwrap_or(0) != 0 {
        bail!(
            "getuploadurl failed: errcode={}, errmsg={}",
            upload_resp.errcode.unwrap_or(-1),
            upload_resp.errmsg.unwrap_or_default()
        );
    }

    let upload_param = upload_resp
        .upload_param
        .ok_or_else(|| anyhow!("missing upload_param in response"))?;
    let filekey = upload_resp
        .filekey
        .ok_or_else(|| anyhow!("missing filekey in response"))?;

    // 3. Upload ciphertext to CDN
    let cdn_upload_url = format!(
        "https://{}/upload?encrypted_query_param={}&filekey={}",
        cdn_base_url.trim_end_matches('/'),
        upload_param,
        filekey
    );

    let cdn_resp = client
        .post(&cdn_upload_url)
        .header("Content-Type", "application/octet-stream")
        .body(ciphertext)
        .send()
        .await
        .context("CDN upload request failed")?;

    if !cdn_resp.status().is_success() {
        bail!("CDN upload failed with status: {}", cdn_resp.status());
    }

    // 4. Extract download param from response header
    let download_param = cdn_resp
        .headers()
        .get("x-encrypted-param")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("missing x-encrypted-param in CDN response"))?;

    // Convert hex key to base64 for the message
    let key_bytes = hex::decode(&aes_key_hex).context("invalid hex key")?;
    let cdn_aes_key_base64 = BASE64.encode(&key_bytes);

    Ok(UploadedMedia {
        download_encrypted_query_param: download_param,
        cdn_aes_key_base64,
        file_size_ciphertext: ciphertext_size,
        plaintext_size: raw_size,
    })
}

// ---------------------------------------------------------------------------
// Outbound message construction
// ---------------------------------------------------------------------------

/// Build an outbound MessageItem for a successfully uploaded media.
pub fn build_outbound_media_item(
    uploaded: &UploadedMedia,
    media_type: UploadMediaType,
    file_name: &str,
    raw_size: u64,
) -> MessageItem {
    let cdn_media = CdnMedia {
        encrypt_query_param: Some(uploaded.download_encrypted_query_param.clone()),
        aes_key: Some(uploaded.cdn_aes_key_base64.clone()),
        encrypt_type: Some(1),
    };

    match media_type {
        UploadMediaType::Image => MessageItem {
            item_type: MESSAGE_ITEM_IMAGE,
            image_item: Some(ImageItem {
                media: Some(cdn_media),
                aeskey: Some(uploaded.cdn_aes_key_base64.clone()),
                ..Default::default()
            }),
            ..Default::default()
        },
        UploadMediaType::Video => MessageItem {
            item_type: MESSAGE_ITEM_VIDEO,
            video_item: Some(VideoItem {
                media: Some(cdn_media),
                video_size: Some(raw_size.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        UploadMediaType::File => MessageItem {
            item_type: MESSAGE_ITEM_FILE,
            file_item: Some(FileItem {
                media: Some(cdn_media),
                file_name: Some(file_name.to_string()),
                len: Some(raw_size.to_string()),
            }),
            ..Default::default()
        },
    }
}

/// Send message request body for the sendmessage API.
/// Note: This is the inner structure. In practice, iLink requires wrapping this
/// in a `{"base_info": {...}, "msg": {...}}` envelope — see WeixinAdapter::send_media.
#[derive(Debug, Serialize)]
pub struct SendMessageRequest {
    pub to_user_id: String,
    pub client_id: String,
    pub message_type: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
    pub item_list: Vec<MessageItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_cdn_download_url() {
        let url = build_cdn_download_url("novac2c.cdn.weixin.qq.com/c2c", "abc123");
        assert_eq!(
            url,
            "https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=abc123"
        );
    }

    #[test]
    fn test_build_cdn_download_url_trailing_slash() {
        let url = build_cdn_download_url("novac2c.cdn.weixin.qq.com/c2c/", "xyz");
        assert_eq!(
            url,
            "https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=xyz"
        );
    }

    #[test]
    fn test_infer_voice_mime() {
        assert_eq!(infer_voice_mime(Some(7)), "audio/mpeg");
        assert_eq!(infer_voice_mime(Some(8)), "audio/ogg");
        assert_eq!(infer_voice_mime(Some(5)), "audio/amr");
        assert_eq!(infer_voice_mime(Some(6)), "audio/silk");
        assert_eq!(infer_voice_mime(None), "audio/silk");
    }

    #[test]
    fn test_encrypt_media_payload_size() {
        let data = b"hello world, this is test data!";
        let (ciphertext, key_hex) = encrypt_media_payload(data);
        // Ciphertext = IV (16) + padded data
        let expected_padded = ((data.len() / 16) + 1) * 16;
        assert_eq!(ciphertext.len(), 16 + expected_padded);
        assert_eq!(key_hex.len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn test_padded_ciphertext_size() {
        assert_eq!(padded_ciphertext_size(0), 16);
        assert_eq!(padded_ciphertext_size(1), 16);
        assert_eq!(padded_ciphertext_size(15), 16);
        assert_eq!(padded_ciphertext_size(16), 32);
        assert_eq!(padded_ciphertext_size(17), 32);
    }

    #[test]
    fn test_extract_image_attachment() {
        let items = vec![MessageItem {
            item_type: MESSAGE_ITEM_IMAGE,
            image_item: Some(ImageItem {
                media: Some(CdnMedia {
                    encrypt_query_param: Some("param123".to_string()),
                    aes_key: Some("base64key".to_string()),
                    encrypt_type: Some(1),
                }),
                aeskey: Some("priority_key".to_string()),
                mid_size: Some("1024".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }];

        let attachments = extract_media_attachments(&items, "novac2c.cdn.weixin.qq.com/c2c");
        assert_eq!(attachments.len(), 1);
        let att = &attachments[0];
        assert_eq!(att.media_type, MediaType::Image);
        assert_eq!(att.aes_key.as_deref(), Some("priority_key"));
        assert_eq!(att.size, Some(1024));
        assert!(att.url.contains("param123"));
    }

    #[test]
    fn test_extract_file_attachment() {
        let items = vec![MessageItem {
            item_type: MESSAGE_ITEM_FILE,
            file_item: Some(FileItem {
                media: Some(CdnMedia {
                    encrypt_query_param: Some("file_param".to_string()),
                    aes_key: Some("filekey".to_string()),
                    encrypt_type: Some(1),
                }),
                file_name: Some("report.pdf".to_string()),
                len: Some("2048".to_string()),
            }),
            ..Default::default()
        }];

        let attachments = extract_media_attachments(&items, "novac2c.cdn.weixin.qq.com/c2c");
        assert_eq!(attachments.len(), 1);
        let att = &attachments[0];
        assert_eq!(att.media_type, MediaType::File);
        assert_eq!(att.file_name.as_deref(), Some("report.pdf"));
        assert_eq!(att.mime_type, "application/pdf");
        assert_eq!(att.size, Some(2048));
    }

    #[test]
    fn test_build_outbound_media_item_image() {
        let uploaded = UploadedMedia {
            download_encrypted_query_param: "dl_param".to_string(),
            cdn_aes_key_base64: "b64key".to_string(),
            file_size_ciphertext: 1024,
            plaintext_size: 1000,
        };
        let item = build_outbound_media_item(&uploaded, UploadMediaType::Image, "photo.jpg", 1000);
        assert_eq!(item.item_type, MESSAGE_ITEM_IMAGE);
        let img = item.image_item.unwrap();
        assert_eq!(img.media.unwrap().encrypt_query_param.unwrap(), "dl_param");
        assert_eq!(img.aeskey.unwrap(), "b64key");
    }
}
