use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::subagent::{CustomSubagentInput, CustomSubagentModelRole, CustomSubagentRecord};

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
    model_role: String,
    is_enabled: i32,
    can_delegate: i32,
    max_delegation_depth: i32,
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
            model_role: CustomSubagentModelRole::from_db(&self.model_role),
            is_enabled: self.is_enabled != 0,
            can_delegate: self.can_delegate != 0,
            max_delegation_depth: self.max_delegation_depth as u32,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

const SUBAGENT_COLUMNS: &str = "id, name, slug, system_prompt, invocation_description, allowed_tools, model_role, is_enabled, can_delegate, max_delegation_depth, created_at, updated_at";

/// Same column list as `SUBAGENT_COLUMNS` but qualified with the `s.` table
/// alias, for queries that JOIN `custom_subagents AS s`. Kept beside the base
/// constant so adding a column only requires updating both in one place.
const SUBAGENT_COLUMNS_PREFIXED: &str = "s.id, s.name, s.slug, s.system_prompt, s.invocation_description, s.allowed_tools, s.model_role, s.is_enabled, s.can_delegate, s.max_delegation_depth, s.created_at, s.updated_at";

// ---------------------------------------------------------------------------
// CRUD operations
// ---------------------------------------------------------------------------

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<CustomSubagentRecord>, AppError> {
    let rows = sqlx::query_as::<_, SubagentRow>(&format!(
        "SELECT {SUBAGENT_COLUMNS} FROM custom_subagents ORDER BY name ASC"
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(rows.into_iter().map(SubagentRow::into_record).collect())
}

pub async fn get_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<CustomSubagentRecord>, AppError> {
    let row = sqlx::query_as::<_, SubagentRow>(&format!(
        "SELECT {SUBAGENT_COLUMNS} FROM custom_subagents WHERE id = ?"
    ))
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
    let row = sqlx::query_as::<_, SubagentRow>(&format!(
        "SELECT {SUBAGENT_COLUMNS} FROM custom_subagents WHERE slug = ?"
    ))
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
    crate::model::subagent::validate_slug(&input.slug).map_err(|msg| {
        AppError::recoverable(ErrorSource::Settings, "custom_subagent.invalid_slug", msg)
    })?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let is_enabled: i32 = if input.is_enabled.unwrap_or(true) {
        1
    } else {
        0
    };
    let can_delegate: i32 = if input.can_delegate.unwrap_or(false) {
        1
    } else {
        0
    };
    let max_depth = input.max_delegation_depth.unwrap_or(3);
    let max_depth_val: i32 = max_depth.clamp(1, 5) as i32;
    let tools_json =
        serde_json::to_string(&input.allowed_tools).unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "INSERT INTO custom_subagents (id, name, slug, system_prompt, invocation_description, allowed_tools, model_role, is_enabled, can_delegate, max_delegation_depth, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.name)
    .bind(&input.slug)
    .bind(&input.system_prompt)
    .bind(&input.invocation_description)
    .bind(&tools_json)
    .bind(input.model_role.as_str())
    .bind(is_enabled)
    .bind(can_delegate)
    .bind(max_depth_val)
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
        model_role: input.model_role,
        is_enabled: is_enabled != 0,
        can_delegate: can_delegate != 0,
        max_delegation_depth: max_depth_val as u32,
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
    // Preserve the existing is_enabled value when the input omits it,
    // preventing accidental re-enabling of disabled subagents.
    let existing = get_by_id(pool, id).await?.ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "custom_subagent.not_found",
            "custom subagent not found",
        )
    })?;

    crate::model::subagent::validate_slug(&input.slug).map_err(|msg| {
        AppError::recoverable(ErrorSource::Settings, "custom_subagent.invalid_slug", msg)
    })?;

    let is_enabled_val = input.is_enabled.unwrap_or_else(|| existing.is_enabled);
    let is_enabled: i32 = if is_enabled_val { 1 } else { 0 };
    let can_delegate_val = input.can_delegate.unwrap_or_else(|| existing.can_delegate);
    let can_delegate: i32 = if can_delegate_val { 1 } else { 0 };
    let max_depth_val = input
        .max_delegation_depth
        .unwrap_or_else(|| existing.max_delegation_depth);
    let max_depth_val: i32 = max_depth_val.clamp(1, 5) as i32;
    let tools_json =
        serde_json::to_string(&input.allowed_tools).unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "UPDATE custom_subagents SET name = ?, slug = ?, system_prompt = ?, invocation_description = ?, allowed_tools = ?, model_role = ?, is_enabled = ?, can_delegate = ?, max_delegation_depth = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&input.name)
    .bind(&input.slug)
    .bind(&input.system_prompt)
    .bind(&input.invocation_description)
    .bind(&tools_json)
    .bind(input.model_role.as_str())
    .bind(is_enabled)
    .bind(can_delegate)
    .bind(max_depth_val)
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

    // Construct the updated record from known inputs and cached created_at
    Ok(CustomSubagentRecord {
        id: id.to_string(),
        name: input.name.clone(),
        slug: input.slug.clone(),
        system_prompt: input.system_prompt.clone(),
        invocation_description: input.invocation_description.clone(),
        allowed_tools: tools_json,
        model_role: input.model_role,
        is_enabled: is_enabled_val,
        can_delegate: can_delegate_val,
        max_delegation_depth: max_depth_val as u32,
        created_at: existing.created_at.clone(),
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
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    // Delete existing access records for this profile
    sqlx::query("DELETE FROM profile_subagent_access WHERE profile_id = ?")
        .bind(profile_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    // Insert new access records
    for subagent_id in subagent_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO profile_subagent_access (profile_id, subagent_id) VALUES (?, ?)",
        )
        .bind(profile_id)
        .bind(subagent_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(())
}

pub async fn list_for_profile(
    pool: &SqlitePool,
    profile_id: &str,
) -> Result<Vec<CustomSubagentRecord>, AppError> {
    let rows = sqlx::query_as::<_, SubagentRow>(&format!(
        "SELECT {SUBAGENT_COLUMNS_PREFIXED} \
         FROM custom_subagents s \
         INNER JOIN profile_subagent_access a ON s.id = a.subagent_id \
         WHERE a.profile_id = ? AND s.is_enabled = 1 \
         ORDER BY s.name ASC"
    ))
    .bind(profile_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::internal(ErrorSource::Database, e.to_string()))?;

    Ok(rows.into_iter().map(SubagentRow::into_record).collect())
}
