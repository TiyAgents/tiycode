use super::*;

// --- Skill types ---

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct SkillStateStore {
    #[serde(default)]
    pub(super) enabled: Vec<String>,
    #[serde(default)]
    pub(super) disabled: Vec<String>,
    #[serde(default, alias = "pinned", skip_serializing)]
    #[allow(dead_code)]
    pub(super) legacy_pinned: Vec<String>,
}

pub(super) struct SkillRuntime {
    pub(super) record: SkillRecordDto,
    pub(super) content: String,
}

// --- Skill impl methods ---

impl ExtensionsManager {
    pub(super) async fn resolve_skill_scope(
        &self,
        id: &str,
        workspace_path: Option<&str>,
    ) -> ConfigScope {
        if self
            .skill_exists(id, None, ConfigScope::Global)
            .await
            .unwrap_or(false)
        {
            return ConfigScope::Global;
        }
        if workspace_path.is_some()
            && self
                .skill_exists(id, workspace_path, ConfigScope::Workspace)
                .await
                .unwrap_or(false)
        {
            return ConfigScope::Workspace;
        }
        ConfigScope::Global
    }

    pub async fn list_skills(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<SkillRecordDto>, AppError> {
        Ok(self
            .load_skills(workspace_path, scope)
            .await?
            .into_iter()
            .map(|skill| skill.record)
            .collect())
    }

    pub async fn rescan_skills(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<SkillRecordDto>, AppError> {
        self.list_skills(workspace_path, scope).await
    }

    pub async fn set_skill_enabled(
        &self,
        id: &str,
        enabled: bool,
        workspace_path: Option<&str>,
        scope: Option<ConfigScope>,
    ) -> Result<(), AppError> {
        // Determine the skill's true installation scope from its on-disk location
        // (built-in / plugin → global, workspace `.tiy/skills` → workspace). The
        // caller-supplied scope is treated as a hint only; if it conflicts with
        // where the skill actually lives we still write to the correct config
        // file so user-level installs never get shadowed into a workspace file.
        let actual_scope = self
            .lookup_skill_actual_scope(id, workspace_path)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, format!("skill '{id}'")))?;

        if let Some(requested) = scope {
            if requested != actual_scope {
                tracing::debug!(
                    skill_id = %id,
                    requested = %requested.as_str(),
                    actual = %actual_scope.as_str(),
                    "skill enable/disable scope hint differs from installation scope; using installation scope",
                );
            }
        }

        let effective_workspace_path = if actual_scope == ConfigScope::Workspace {
            workspace_path
        } else {
            None
        };

        let mut store = self
            .load_skill_state_store(effective_workspace_path, actual_scope)
            .await?;
        update_named_membership(&mut store.enabled, id, enabled);
        update_named_membership(&mut store.disabled, id, !enabled);
        self.save_skill_state_store(&store, effective_workspace_path, actual_scope)
            .await?;
        self.write_extension_audit(
            if enabled {
                "skill_enabled"
            } else {
                "skill_disabled"
            },
            "skill",
            id,
            serde_json::json!({ "enabled": enabled }),
        )
        .await
    }

    pub async fn preview_skill(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<SkillPreviewDto, AppError> {
        let skill = self
            .load_skills(workspace_path, scope)
            .await?
            .into_iter()
            .find(|skill| skill.record.id == id)
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, format!("skill '{id}'")))?;
        Ok(SkillPreviewDto {
            record: skill.record,
            content: skill.content,
        })
    }

    pub(super) async fn collect_skill_summaries(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<ExtensionSummaryDto>, AppError> {
        Ok(self
            .load_skills(workspace_path, scope)
            .await?
            .into_iter()
            .map(|skill| self.build_skill_summary(&skill.record))
            .collect())
    }

    pub(super) async fn load_skills(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<SkillRuntime>, AppError> {
        let global_state = self
            .load_skill_state_store(None, ConfigScope::Global)
            .await?;
        let workspace_state = if scope == ConfigScope::Workspace && workspace_path.is_some() {
            Some(
                self.load_skill_state_store(workspace_path, ConfigScope::Workspace)
                    .await?,
            )
        } else {
            None
        };
        let max_prompt_chars = self.load_skill_prompt_budget().await?;
        let mut results = Vec::new();
        let mut visited = HashSet::new();

        for (source_label, path) in self.skill_source_roots(workspace_path, scope).await? {
            if !path.exists() {
                continue;
            }

            for skill_dir in read_child_dirs(&path)? {
                let skill_file = skill_dir.join("SKILL.md");
                if !skill_file.is_file() {
                    continue;
                }
                let raw = match fs::read_to_string(&skill_file) {
                    Ok(raw) => raw,
                    Err(error) => {
                        tracing::warn!(path = %skill_file.display(), error = %error, "failed to read skill file");
                        continue;
                    }
                };
                let parsed = parse_skill_markdown(&raw, &skill_dir, &source_label);
                let Some((mut record, content)) = parsed else {
                    continue;
                };
                if visited.contains(&record.id) {
                    continue;
                }
                visited.insert(record.id.clone());

                // Determine the true installation scope based on where the skill
                // was discovered. Built-in and plugin-provided skills live under
                // the user's home and are always global; only skills discovered
                // under the workspace's `.tiy/skills` directory are workspace
                // scoped. Applying state must follow the same rule so that
                // enable/disable decisions persist in the matching config file.
                let effective_scope = match source_label.as_str() {
                    "workspace" => ConfigScope::Workspace,
                    _ => ConfigScope::Global,
                };

                apply_skill_state(&mut record, &global_state);
                if effective_scope == ConfigScope::Workspace {
                    if let Some(workspace_state) = workspace_state.as_ref() {
                        apply_skill_state(&mut record, workspace_state);
                    }
                }
                record.scope = effective_scope.as_str().to_string();

                record.prompt_budget_chars = max_prompt_chars.min(record.content_preview.len());
                results.push(SkillRuntime { record, content });
            }
        }

        results.sort_by(|left, right| compare_skill_records(&left.record, &right.record));
        Ok(results)
    }

    pub(super) async fn load_skill_state_store(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<SkillStateStore, AppError> {
        let file = match scope {
            ConfigScope::Global => global_skills_config_path(),
            ConfigScope::Workspace => workspace_skills_read_path(workspace_path)?,
        };
        Ok(self
            .read_json_file_with_diagnostics(&file, "skills", scope)?
            .value)
    }

    pub(super) async fn save_skill_state_store(
        &self,
        store: &SkillStateStore,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<(), AppError> {
        let file = match scope {
            ConfigScope::Global => global_skills_config_path(),
            ConfigScope::Workspace => workspace_skills_config_path(workspace_path)?,
        };
        self.write_json_file(&file, store)
    }

    pub(super) async fn load_skill_prompt_budget(&self) -> Result<usize, AppError> {
        let max_chars_record =
            settings_repo::get(&self.pool, EXTENSIONS_SKILLS_MAX_PROMPT_CHARS_KEY).await?;
        let max_count_record =
            settings_repo::get(&self.pool, EXTENSIONS_SKILLS_MAX_SELECTED_COUNT_KEY).await?;
        let max_chars = max_chars_record
            .and_then(|record| serde_json::from_str::<usize>(&record.value_json).ok())
            .unwrap_or(4_000);
        let max_count = max_count_record
            .and_then(|record| serde_json::from_str::<usize>(&record.value_json).ok())
            .unwrap_or(4);
        Ok(max_chars.saturating_mul(max_count.max(1)))
    }

    pub(super) async fn skill_source_roots(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<(String, PathBuf)>, AppError> {
        let mut roots = vec![
            ("builtin".to_string(), agents_home().join("skills")),
            ("builtin".to_string(), tiy_home().join("skills")),
        ];
        if scope == ConfigScope::Workspace {
            if let Some(workspace_path) = workspace_path {
                roots.push((
                    "workspace".to_string(),
                    PathBuf::from(workspace_path).join(".tiy/skills"),
                ));
            }
        }
        for plugin in self.load_enabled_plugin_runtimes().await? {
            let skills_dir = plugin
                .manifest
                .skills_dir
                .clone()
                .unwrap_or_else(|| "skills".to_string());
            roots.push(("plugin".to_string(), plugin.path.join(skills_dir)));
        }
        Ok(roots)
    }

    pub(super) fn build_skill_summary(&self, record: &SkillRecordDto) -> ExtensionSummaryDto {
        ExtensionSummaryDto {
            id: record.id.clone(),
            kind: ExtensionKind::Skill,
            name: record.name.clone(),
            version: "content".to_string(),
            description: record.description.clone(),
            source: match record.source.as_str() {
                "builtin" => ExtensionSourceDto::Builtin,
                _ => ExtensionSourceDto::LocalDir {
                    path: record.path.clone(),
                },
            },
            install_state: if record.enabled {
                ExtensionInstallState::Enabled
            } else {
                ExtensionInstallState::Disabled
            },
            health: ExtensionHealth::Healthy,
            permissions: Vec::new(),
            tags: record.tags.clone(),
        }
    }

    pub(super) async fn skill_exists(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<bool, AppError> {
        Ok(self
            .load_skills(workspace_path, scope)
            .await?
            .iter()
            .any(|skill| skill.record.id == id))
    }

    pub(super) async fn update_skill_enabled(
        &self,
        id: &str,
        enabled: bool,
        workspace_path: Option<&str>,
        _scope: ConfigScope,
    ) -> Result<bool, AppError> {
        // The caller-supplied `scope` is intentionally ignored here: the scope
        // that matters is the skill's on-disk installation location, and
        // `set_skill_enabled` resolves that itself. We still return `Ok(false)`
        // when the id doesn't match any installed skill so `enable_extension` /
        // `disable_extension` can continue trying other extension kinds.
        if self
            .lookup_skill_actual_scope(id, workspace_path)
            .await?
            .is_none()
        {
            return Ok(false);
        }
        self.set_skill_enabled(id, enabled, workspace_path, None)
            .await?;
        Ok(true)
    }

    /// Find the actual installation scope of a skill by scanning both the
    /// global skill roots and (when available) the workspace skill root. The
    /// scope is read from the `SkillRecordDto.scope` field, which `load_skills`
    /// now derives from the discovered source label rather than the query
    /// parameter.
    pub(super) async fn lookup_skill_actual_scope(
        &self,
        id: &str,
        workspace_path: Option<&str>,
    ) -> Result<Option<ConfigScope>, AppError> {
        if workspace_path.is_some() {
            for skill in self
                .load_skills(workspace_path, ConfigScope::Workspace)
                .await?
            {
                if skill.record.id == id {
                    let resolved = match skill.record.scope.as_str() {
                        "workspace" => ConfigScope::Workspace,
                        _ => ConfigScope::Global,
                    };
                    return Ok(Some(resolved));
                }
            }
        }

        for skill in self.load_skills(None, ConfigScope::Global).await? {
            if skill.record.id == id {
                return Ok(Some(ConfigScope::Global));
            }
        }

        Ok(None)
    }
}

// --- Skill free functions ---

pub(super) fn global_skills_config_path() -> PathBuf {
    tiy_home().join("skills.json")
}

pub(super) fn workspace_skills_config_path(
    workspace_path: Option<&str>,
) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped skill config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/skills.local.json"))
}

pub(super) fn legacy_workspace_skills_config_path(
    workspace_path: Option<&str>,
) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped skill config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/skills.json"))
}

pub(super) fn workspace_skills_read_path(
    workspace_path: Option<&str>,
) -> Result<PathBuf, AppError> {
    let path = workspace_skills_config_path(workspace_path)?;
    if path.exists() {
        return Ok(path);
    }
    let legacy_path = legacy_workspace_skills_config_path(workspace_path)?;
    if legacy_path.exists() {
        return Ok(legacy_path);
    }
    Ok(path)
}

pub(super) fn parse_skill_markdown(
    raw: &str,
    skill_dir: &Path,
    source: &str,
) -> Option<(SkillRecordDto, String)> {
    let (frontmatter, body) = split_frontmatter(raw)?;
    let meta = parse_frontmatter_map(frontmatter);
    let base_id = meta.get("id").cloned().or_else(|| {
        skill_dir
            .file_name()
            .and_then(OsStr::to_str)
            .map(str::to_string)
    })?;
    let name = meta.get("name").cloned().unwrap_or_else(|| base_id.clone());
    let description = meta.get("description").cloned();
    let tags = parse_array_field(meta.get("tags"));
    let triggers = parse_array_field(meta.get("triggers"));
    let tools = parse_array_field(meta.get("tools"));
    let priority = meta.get("priority").cloned();
    let trimmed_body = body.trim();
    let preview = trimmed_body.chars().take(320).collect::<String>();

    let namespaced_id = if source == "builtin" {
        base_id
    } else {
        format!("{source}:{base_id}")
    };

    Some((
        SkillRecordDto {
            id: namespaced_id,
            name,
            description,
            tags,
            triggers,
            tools,
            priority,
            source: source.to_string(),
            path: skill_dir.to_string_lossy().to_string(),
            enabled: true,
            scope: "global".to_string(),
            content_preview: preview.clone(),
            prompt_budget_chars: preview.len(),
        },
        raw.to_string(),
    ))
}

pub(super) fn update_named_membership(values: &mut Vec<String>, id: &str, enabled: bool) {
    values.retain(|value| value != id);
    if enabled {
        values.push(id.to_string());
        values.sort();
    }
}

pub(super) fn compare_skill_records(left: &SkillRecordDto, right: &SkillRecordDto) -> Ordering {
    right
        .enabled
        .cmp(&left.enabled)
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn apply_skill_state(record: &mut SkillRecordDto, state: &SkillStateStore) {
    if state.disabled.iter().any(|value| value == &record.id) {
        record.enabled = false;
    }
    if state.enabled.iter().any(|value| value == &record.id) {
        record.enabled = true;
    }
}

pub(super) fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start();
    let rest = trimmed.strip_prefix("---")?;
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let end = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let (frontmatter, body_with_sep) = rest.split_at(end);
    let body = body_with_sep
        .strip_prefix("\n---\n")
        .or_else(|| body_with_sep.strip_prefix("\r\n---\r\n"))
        .or_else(|| body_with_sep.strip_prefix("\n---"))
        .unwrap_or_default();
    Some((frontmatter, body))
}

pub(super) fn parse_frontmatter_map(frontmatter: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let lines = frontmatter.lines().collect::<Vec<_>>();
    let mut index = 0usize;

    while index < lines.len() {
        let raw_line = lines[index];
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            index += 1;
            continue;
        }
        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            index += 1;
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            index += 1;
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();

        if matches!(value, ">" | ">-" | ">+" | "|" | "|-" | "|+") {
            let folded = value.starts_with('>');
            index += 1;
            let mut block_lines = Vec::new();
            while index < lines.len() {
                let next_line = lines[index];
                if next_line.starts_with(' ') || next_line.starts_with('\t') {
                    block_lines.push(next_line.trim().to_string());
                    index += 1;
                    continue;
                }
                if next_line.trim().is_empty() {
                    block_lines.push(String::new());
                    index += 1;
                    continue;
                }
                break;
            }

            let parsed = if folded {
                fold_yaml_block_scalar(&block_lines)
            } else {
                block_lines.join("\n").trim().to_string()
            };
            values.insert(key, parsed);
            continue;
        }

        if value.is_empty() {
            index += 1;
            let mut list_items = Vec::new();
            while index < lines.len() {
                let next_line = lines[index];
                let trimmed = next_line.trim();
                if trimmed.is_empty() {
                    index += 1;
                    continue;
                }
                if next_line.starts_with(' ') || next_line.starts_with('\t') {
                    if let Some(item) = trimmed.strip_prefix("- ") {
                        list_items.push(trim_yaml_scalar(item));
                        index += 1;
                        continue;
                    }
                }
                break;
            }

            values.insert(key, list_items.join("\n"));
            continue;
        }

        values.insert(key, trim_yaml_scalar(value));
        index += 1;
    }
    values
}

pub(super) fn parse_array_field(value: Option<&String>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let trimmed = value.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        if trimmed.contains('\n') {
            return trimmed
                .lines()
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(trim_yaml_scalar)
                .collect();
        }
        return if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trim_yaml_scalar(trimmed)]
        };
    }
    trimmed[1..trimmed.len() - 1]
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(trim_yaml_scalar)
        .collect()
}

pub(super) fn trim_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

pub(super) fn fold_yaml_block_scalar(lines: &[String]) -> String {
    let mut result = String::new();

    for line in lines {
        if line.is_empty() {
            if !result.ends_with("\n\n") {
                if result.ends_with(' ') {
                    result.pop();
                }
                result.push_str("\n\n");
            }
            continue;
        }

        if result.is_empty() || result.ends_with("\n\n") {
            result.push_str(line);
        } else {
            result.push(' ');
            result.push_str(line);
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn compare_skill_records_sorts_enabled_then_name() {
        let mut items = vec![
            SkillRecordDto {
                id: "skill-zeta".to_string(),
                name: "Zeta".to_string(),
                description: None,
                tags: Vec::new(),
                triggers: Vec::new(),
                tools: Vec::new(),
                priority: None,
                source: "builtin".to_string(),
                path: "/tmp/zeta".to_string(),
                enabled: false,
                scope: "global".to_string(),
                content_preview: String::new(),
                prompt_budget_chars: 100,
            },
            SkillRecordDto {
                id: "skill-alpha".to_string(),
                name: "Alpha".to_string(),
                description: None,
                tags: Vec::new(),
                triggers: Vec::new(),
                tools: Vec::new(),
                priority: None,
                source: "builtin".to_string(),
                path: "/tmp/alpha".to_string(),
                enabled: true,
                scope: "global".to_string(),
                content_preview: String::new(),
                prompt_budget_chars: 100,
            },
            SkillRecordDto {
                id: "skill-bravo".to_string(),
                name: "bravo".to_string(),
                description: None,
                tags: Vec::new(),
                triggers: Vec::new(),
                tools: Vec::new(),
                priority: None,
                source: "builtin".to_string(),
                path: "/tmp/bravo".to_string(),
                enabled: true,
                scope: "global".to_string(),
                content_preview: String::new(),
                prompt_budget_chars: 100,
            },
        ];

        items.sort_by(compare_skill_records);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["skill-alpha", "skill-bravo", "skill-zeta"]
        );
    }

    #[test]
    fn skill_state_store_deserializes_legacy_pinned_alias_without_serializing_it() {
        let store: SkillStateStore = serde_json::from_value(serde_json::json!({
            "enabled": ["skill-a"],
            "disabled": ["skill-b"],
            "pinned": ["skill-legacy"]
        }))
        .expect("deserialize skill state");

        assert_eq!(store.enabled, vec!["skill-a"]);
        assert_eq!(store.disabled, vec!["skill-b"]);
        assert_eq!(store.legacy_pinned, vec!["skill-legacy"]);

        let serialized = serde_json::to_value(&store).expect("serialize skill state");
        assert_eq!(
            serialized.get("enabled"),
            Some(&serde_json::json!(["skill-a"]))
        );
        assert_eq!(
            serialized.get("disabled"),
            Some(&serde_json::json!(["skill-b"]))
        );
        assert!(serialized.get("pinned").is_none());
        assert!(serialized.get("legacyPinned").is_none());
    }

    #[test]
    fn parse_skill_markdown_namespaces_non_builtin_sources() {
        let skill_dir = tempdir().expect("tempdir");
        let raw = r#"---
name: Skill Alpha
description: Example skill
---

Body text
"#;

        let (builtin_record, _) =
            parse_skill_markdown(raw, skill_dir.path(), "builtin").expect("builtin skill");
        let (workspace_record, _) =
            parse_skill_markdown(raw, skill_dir.path(), "workspace").expect("workspace skill");

        assert!(!builtin_record.id.is_empty());
        assert_eq!(
            workspace_record.id,
            format!("workspace:{}", builtin_record.id)
        );
    }

    #[test]
    fn parse_skill_markdown_supports_folded_descriptions_and_yaml_lists() {
        let skill_dir = tempdir().expect("tempdir");
        let raw = r#"---
name: project-docs-sync
description: >-
  Proactively detect when project documentation files need updating.
  Automatically applies targeted edits to keep docs in sync.
tags:
  - documentation
  - automation
triggers:
  - README.md
  - AGENTS.md
tools:
  - git
  - rg
---

Body text
"#;

        let (record, _) = parse_skill_markdown(raw, skill_dir.path(), "builtin")
            .expect("folded description skill");

        assert_eq!(
            record.description.as_deref(),
            Some(
                "Proactively detect when project documentation files need updating. Automatically applies targeted edits to keep docs in sync."
            )
        );
        assert_eq!(record.tags, vec!["documentation", "automation"]);
        assert_eq!(record.triggers, vec!["README.md", "AGENTS.md"]);
        assert_eq!(record.tools, vec!["git", "rg"]);
    }
}
