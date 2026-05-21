use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::subagent::{CustomSubagentInput, CustomSubagentRecord};

// ---------------------------------------------------------------------------
// Internal row type for sqlx mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct SubagentRow {
    id: String,
    name: String,
    slug: String,
    system_prompt: String,
    invocation_description: String,
    allowed_tools: String,
    is_enabled: i32,
    created_at: String,
    updated_at: String,
}

impl SubagentRow {
    fn into_record(self) -> CustomSubagentRecord {
        CustomSubagentRecord {
            id: self.id,
            name: self.name,
            slug: self.slug,
            system_prompt: self.system_prompt,
            invocation_description: self.invocation_description,
            allowed_tools: self.allowed_tools,
            is_enabled: self.is_enabled != 0,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// CRUD operations
// ---------------------------------------------------------------------------

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<CustomSubagentRecord>, AppError> {
    let rows = sqlx::query_as::<_, SubagentRow>(
        "SELECT id, name, slug, system_prompt, invocation_description, allowed_tools, is_enabled, created_at, updated_at FROM custom_subagents ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(rows.into_iter().map(SubagentRow::into_record).collect())
}

pub async fn get_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<CustomSubagentRecord>, AppError> {
    let row = sqlx::query_as::<_, SubagentRow>(
        "SELECT id, name, slug, system_prompt, invocation_description, allowed_tools, is_enabled, created_at, updated_at FROM custom_subagents WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(row.map(SubagentRow::into_record))
}

pub async fn get_by_slug(
    pool: &SqlitePool,
    slug: &str,
) -> Result<Option<CustomSubagentRecord>, AppError> {
    let row = sqlx::query_as::<_, SubagentRow>(
        "SELECT id, name, slug, system_prompt, invocation_description, allowed_tools, is_enabled, created_at, updated_at FROM custom_subagents WHERE slug = ?",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(row.map(SubagentRow::into_record))
}

pub async fn create(
    pool: &SqlitePool,
    input: &CustomSubagentInput,
) -> Result<CustomSubagentRecord, AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let is_enabled: i32 = if input.is_enabled.unwrap_or(true) {
        1
    } else {
        0
    };
    let tools_json =
        serde_json::to_string(&input.allowed_tools).unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "INSERT INTO custom_subagents (id, name, slug, system_prompt, invocation_description, allowed_tools, is_enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.name)
    .bind(&input.slug)
    .bind(&input.system_prompt)
    .bind(&input.invocation_description)
    .bind(&tools_json)
    .bind(is_enabled)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE constraint failed") {
            AppError::recoverable(
                ErrorSource::Settings,
                "custom_subagent.slug_conflict",
                format!("A subagent with slug '{}' already exists", input.slug),
            )
        } else {
            AppError::internal(ErrorSource::Database, e.to_string())
        }
    })?;

    Ok(CustomSubagentRecord {
        id,
        name: input.name.clone(),
        slug: input.slug.clone(),
        system_prompt: input.system_prompt.clone(),
        invocation_description: input.invocation_description.clone(),
        allowed_tools: tools_json,
        is_enabled: is_enabled != 0,
        created_at: now.clone(),
        updated_at: now,
    })
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    input: &CustomSubagentInput,
) -> Result<CustomSubagentRecord, AppError> {
    let now = Utc::now().to_rfc3339();
    let is_enabled: i32 = if input.is_enabled.unwrap_or(true) {
        1
    } else {
        0
    };
    let tools_json =
        serde_json::to_string(&input.allowed_tools).unwrap_or_else(|_| "[]".to_string());

    let result = sqlx::query(
        "UPDATE custom_subagents SET name = ?, slug = ?, system_prompt = ?, invocation_description = ?, allowed_tools = ?, is_enabled = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&input.name)
    .bind(&input.slug)
    .bind(&input.system_prompt)
    .bind(&input.invocation_description)
    .bind(&tools_json)
    .bind(is_enabled)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE constraint failed") {
            AppError::recoverable(
                ErrorSource::Settings,
                "custom_subagent.slug_conflict",
                format!("A subagent with slug '{}' already exists", input.slug),
            )
        } else {
            AppError::internal(ErrorSource::Database, e.to_string())
        }
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::recoverable(
            ErrorSource::Settings,
            "custom_subagent.not_found",
            "custom subagent not found",
        ));
    }

    Ok(CustomSubagentRecord {
        id: id.to_string(),
        name: input.name.clone(),
        slug: input.slug.clone(),
        system_prompt: input.system_prompt.clone(),
        invocation_description: input.invocation_description.clone(),
        allowed_tools: tools_json,
        is_enabled: is_enabled != 0,
        created_at: String::new(), // caller can re-fetch if needed
        updated_at: now,
    })
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM custom_subagents WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Profile ↔ Subagent access
// ---------------------------------------------------------------------------

pub async fn get_profile_access(
    pool: &SqlitePool,
    profile_id: &str,
) -> Result<Vec<String>, AppError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT subagent_id FROM profile_subagent_access WHERE profile_id = ?")
            .bind(profile_id)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub async fn set_profile_access(
    pool: &SqlitePool,
    profile_id: &str,
    subagent_ids: &[String],
) -> Result<(), AppError> {
    // Delete existing access records for this profile
    sqlx::query("DELETE FROM profile_subagent_access WHERE profile_id = ?")
        .bind(profile_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    // Insert new access records
    for subagent_id in subagent_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO profile_subagent_access (profile_id, subagent_id) VALUES (?, ?)",
        )
        .bind(profile_id)
        .bind(subagent_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;
    }

    Ok(())
}

pub async fn list_for_profile(
    pool: &SqlitePool,
    profile_id: &str,
) -> Result<Vec<CustomSubagentRecord>, AppError> {
    let rows = sqlx::query_as::<_, SubagentRow>(
        "SELECT s.id, s.name, s.slug, s.system_prompt, s.invocation_description, s.allowed_tools, s.is_enabled, s.created_at, s.updated_at \
         FROM custom_subagents s \
         INNER JOIN profile_subagent_access a ON s.id = a.subagent_id \
         WHERE a.profile_id = ? AND s.is_enabled = 1 \
         ORDER BY s.name ASC",
    )
    .bind(profile_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(rows.into_iter().map(SubagentRow::into_record).collect())
}
