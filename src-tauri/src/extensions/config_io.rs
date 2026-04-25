use super::*;

#[derive(Debug, Clone)]
pub(super) struct ConfigLoadOutcome<T> {
    pub(super) value: T,
}

impl ExtensionsManager {
    pub(super) async fn read_json_setting<T>(&self, key: &str) -> Result<T, AppError>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        let value = settings_repo::get(&self.pool, key).await?;
        match value {
            Some(record) => serde_json::from_str(&record.value_json).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Settings,
                    "extensions.settings.invalid_json",
                    format!("Invalid extension setting payload for '{key}': {error}"),
                )
            }),
            None => Ok(T::default()),
        }
    }

    pub(super) async fn write_json_setting<T>(&self, key: &str, value: &T) -> Result<(), AppError>
    where
        T: Serialize + ?Sized,
    {
        let encoded = serde_json::to_string(value).map_err(|error| {
            AppError::internal(
                ErrorSource::Settings,
                format!("Failed to serialize extension setting '{key}': {error}"),
            )
        })?;
        settings_repo::set(&self.pool, key, &encoded).await
    }

    pub(super) fn read_json_file<T>(&self, path: &Path) -> Result<T, AppError>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        Ok(self
            .read_json_file_with_diagnostics(path, "config", ConfigScope::Global)?
            .value)
    }

    pub(super) fn read_json_file_with_diagnostics<T>(
        &self,
        path: &Path,
        area: &str,
        scope: ConfigScope,
    ) -> Result<ConfigLoadOutcome<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        if !path.exists() {
            self.clear_diagnostic(path, area, scope);
            return Ok(ConfigLoadOutcome {
                value: T::default(),
            });
        }
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) => {
                let diagnostic = self.make_config_diagnostic(
                    path,
                    area,
                    scope,
                    ConfigDiagnosticKind::ReadFailed,
                    format!("Unable to read {area} config"),
                    format!("Failed to read '{}': {error}", path.display()),
                    format!(
                        "Check that '{}' is readable and not locked by another process.",
                        path.display()
                    ),
                );
                self.record_diagnostic(diagnostic.clone());
                return Ok(ConfigLoadOutcome {
                    value: T::default(),
                });
            }
        };

        match serde_json::from_str(&raw) {
            Ok(value) => {
                self.clear_diagnostic(path, area, scope);
                Ok(ConfigLoadOutcome { value })
            }
            Err(error) => {
                let diagnostic = self.make_config_diagnostic(
                    path,
                    area,
                    scope,
                    ConfigDiagnosticKind::InvalidJson,
                    format!("{area} config is not valid JSON"),
                    format!("Invalid JSON in '{}': {error}", path.display()),
                    format!(
                        "Fix the JSON syntax in '{}' or replace it with a valid backup.",
                        path.display()
                    ),
                );
                self.record_diagnostic(diagnostic.clone());
                Ok(ConfigLoadOutcome {
                    value: T::default(),
                })
            }
        }
    }

    pub(super) fn write_json_file<T>(&self, path: &Path, value: &T) -> Result<(), AppError>
    where
        T: Serialize + ?Sized,
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = serde_json::to_string_pretty(value).map_err(|error| {
            AppError::internal(
                ErrorSource::Settings,
                format!(
                    "Failed to serialize extension config '{}': {error}",
                    path.display()
                ),
            )
        })?;
        fs::write(path, encoded)?;
        Ok(())
    }

    pub(super) fn record_diagnostic(&self, diagnostic: ConfigDiagnosticDto) {
        if let Ok(mut items) = self.diagnostics.lock() {
            items.retain(|item| item.id != diagnostic.id);
            items.push(diagnostic);
            items.sort_by(|left, right| left.file_path.cmp(&right.file_path));
        }
    }

    pub(super) fn clear_diagnostic(&self, path: &Path, area: &str, scope: ConfigScope) {
        let id = config_diagnostic_id(path, area, scope);
        if let Ok(mut items) = self.diagnostics.lock() {
            items.retain(|item| item.id != id);
        }
    }

    pub(super) fn make_config_diagnostic(
        &self,
        path: &Path,
        area: &str,
        scope: ConfigScope,
        kind: ConfigDiagnosticKind,
        summary: String,
        detail: String,
        suggestion: String,
    ) -> ConfigDiagnosticDto {
        ConfigDiagnosticDto {
            id: config_diagnostic_id(path, area, scope),
            scope: scope.as_str().to_string(),
            area: area.to_string(),
            file_path: display_config_path(path),
            severity: ConfigDiagnosticSeverity::Error,
            kind,
            summary,
            detail,
            suggestion,
        }
    }

    pub(super) async fn write_extension_audit(
        &self,
        action: &str,
        target_type: &str,
        target_id: &str,
        result: serde_json::Value,
    ) -> Result<(), AppError> {
        audit_repo::insert(
            &self.pool,
            &audit_repo::AuditInsert {
                actor_type: "user".to_string(),
                actor_id: None,
                source: "extensions".to_string(),
                workspace_id: None,
                thread_id: None,
                run_id: None,
                tool_call_id: None,
                action: action.to_string(),
                target_type: Some(target_type.to_string()),
                target_id: Some(target_id.to_string()),
                policy_check_json: None,
                result_json: Some(result.to_string()),
            },
        )
        .await
    }

    pub(super) async fn write_tool_hook_audit(
        &self,
        plugin_id: &str,
        event: &str,
        tool_call_id: &str,
        run_id: &str,
        thread_id: &str,
        output: &HookOutput,
    ) -> Result<(), AppError> {
        audit_repo::insert(
            &self.pool,
            &audit_repo::AuditInsert {
                actor_type: "agent".to_string(),
                actor_id: Some(run_id.to_string()),
                source: format!("plugin:{plugin_id}"),
                workspace_id: None,
                thread_id: Some(thread_id.to_string()),
                run_id: Some(run_id.to_string()),
                tool_call_id: Some(tool_call_id.to_string()),
                action: format!("hook_{event}"),
                target_type: Some("plugin_hook".to_string()),
                target_id: Some(plugin_id.to_string()),
                policy_check_json: None,
                result_json: Some(serde_json::to_string(output).unwrap_or_default()),
            },
        )
        .await
    }
}

pub(super) fn display_config_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }

        if let Ok(canonical_home) = dunce::canonicalize(&home) {
            if let Ok(relative) = path.strip_prefix(&canonical_home) {
                return format!("~/{}", relative.display());
            }
        }
    }

    path.display().to_string()
}

pub(super) fn config_diagnostic_id(path: &Path, area: &str, scope: ConfigScope) -> String {
    format!("{}:{}:{}", scope.as_str(), area, path.display())
}
