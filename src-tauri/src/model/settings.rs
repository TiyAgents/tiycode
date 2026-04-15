use serde::Serialize;

// ---------------------------------------------------------------------------
// Settings (KV)
// ---------------------------------------------------------------------------

/// A single setting row from the `settings` table.
#[derive(Debug, Clone)]
pub struct SettingRecord {
    pub key: String,
    pub value_json: String,
    pub updated_at: String,
}

/// DTO for frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingDto {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCommandDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub argument_hint: String,
    pub description: String,
    pub prompt: String,
    pub source: String,
    pub enabled: bool,
    pub version: u32,
    pub file_name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCommandInput {
    pub id: Option<String>,
    pub name: String,
    pub path: String,
    pub argument_hint: Option<String>,
    pub description: Option<String>,
    pub prompt: String,
    pub source: Option<String>,
    pub enabled: Option<bool>,
    pub version: Option<u32>,
}

impl SettingRecord {
    pub fn into_dto(self) -> SettingDto {
        let value = serde_json::from_str(&self.value_json).unwrap_or(serde_json::Value::Null);
        SettingDto {
            key: self.key,
            value,
            updated_at: self.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Policy (KV, same shape but separate table)
// ---------------------------------------------------------------------------

pub type PolicyRecord = SettingRecord;
pub type PolicyDto = SettingDto;
