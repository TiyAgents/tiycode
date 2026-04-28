use super::*;

// --- Plugin types ---

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct InstalledPluginRecord {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct PluginConfigStore {
    #[serde(default)]
    pub(super) items: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginManifest {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) description: Option<String>,
    pub(super) author: Option<String>,
    pub(super) homepage: Option<String>,
    pub(super) default_enabled: Option<bool>,
    #[serde(default)]
    pub(super) capabilities: Vec<String>,
    #[serde(default)]
    pub(super) permissions: Vec<String>,
    pub(super) hooks: Option<PluginManifestHooks>,
    #[serde(default)]
    pub(super) tools: Vec<PluginManifestTool>,
    #[serde(default)]
    pub(super) commands: Vec<PluginManifestCommand>,
    pub(super) timeout_ms: Option<u64>,
    pub(super) skills_dir: Option<String>,
    pub(super) config_schema: Option<PluginManifestSchema>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginManifestHooks {
    pub(super) pre_tool_use: Option<Vec<String>>,
    pub(super) post_tool_use: Option<Vec<String>>,
    pub(super) on_run_start: Option<Vec<String>>,
    pub(super) on_run_complete: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginManifestTool {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) command: String,
    #[serde(default)]
    pub(super) args: Vec<String>,
    pub(super) env: Option<HashMap<String, String>>,
    pub(super) cwd: Option<String>,
    pub(super) timeout_ms: Option<u64>,
    pub(super) required_permission: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginManifestCommand {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) prompt_template: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginManifestSchema {
    #[allow(dead_code)]
    pub(super) r#type: String,
    pub(super) path: String,
}

#[derive(Debug, Clone)]
pub(super) struct InstalledPluginRuntime {
    pub(super) manifest: PluginManifest,
    pub(super) path: PathBuf,
    pub(super) enabled: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PluginCommandRegistration {
    pub(super) plugin_id: String,
    pub(super) command: PluginManifestCommand,
}

#[derive(Debug, Clone)]
pub(super) struct PluginHookRegistration {
    pub(super) plugin: InstalledPluginRuntime,
    pub(super) handler: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PluginToolInput<'a> {
    pub(super) args: &'a serde_json::Value,
    pub(super) workspace: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) thread_id: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginToolOutput {
    pub(super) success: bool,
    pub(super) result: Option<serde_json::Value>,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct HookInput<'a> {
    pub(super) event: &'a str,
    pub(super) payload: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct HookOutput {
    pub(super) action: Option<String>,
    pub(super) message: Option<String>,
    pub(super) metadata: Option<serde_json::Value>,
}

// --- Plugin impl methods ---

impl ExtensionsManager {
    pub async fn validate_plugin_dir(&self, path: &str) -> Result<PluginDetailDto, AppError> {
        let runtime = self.load_plugin_from_dir(Path::new(path), false)?;
        Ok(self.build_plugin_detail(&runtime, None))
    }

    pub async fn install_plugin_from_dir(&self, path: &str) -> Result<PluginDetailDto, AppError> {
        let runtime = self.load_plugin_from_dir(Path::new(path), false)?;
        let enabled = runtime.manifest.default_enabled.unwrap_or(true);
        let mut installed = self.load_installed_plugin_records().await?;
        installed.retain(|record| record.id != runtime.manifest.id);
        installed.push(InstalledPluginRecord {
            id: runtime.manifest.id.clone(),
            path: runtime.path.to_string_lossy().to_string(),
            enabled,
        });
        self.save_installed_plugin_records(&installed).await?;
        let installed_runtime = InstalledPluginRuntime { enabled, ..runtime };
        self.sync_plugin_managed_mcp_configs(&installed_runtime)
            .await?;
        self.write_extension_audit(
            "plugin_installed",
            "plugin",
            &installed_runtime.manifest.id,
            serde_json::json!({ "path": installed_runtime.path.to_string_lossy() }),
        )
        .await?;
        Ok(self.build_plugin_detail(&installed_runtime, None))
    }

    pub async fn update_plugin_config(
        &self,
        id: &str,
        config: serde_json::Value,
    ) -> Result<(), AppError> {
        let mut store = self.load_plugin_config_store().await?;
        store.items.insert(id.to_string(), config.clone());
        self.save_plugin_config_store(&store).await?;
        self.write_extension_audit("plugin_config_updated", "plugin", id, config)
            .await
    }

    pub(super) async fn collect_plugin_summaries(
        &self,
    ) -> Result<Vec<ExtensionSummaryDto>, AppError> {
        let installed = self.load_installed_plugin_records().await?;
        let installed_ids = installed
            .iter()
            .map(|record| record.id.clone())
            .collect::<HashSet<_>>();
        let mut items = Vec::new();

        for plugin in self.load_plugin_runtimes().await? {
            let install_state = if plugin.enabled {
                ExtensionInstallState::Enabled
            } else {
                ExtensionInstallState::Disabled
            };
            items.push(self.build_plugin_summary(&plugin, install_state, None));
        }

        for discovered_dir in self.discover_plugin_dirs()? {
            let runtime = match self.load_plugin_from_dir(&discovered_dir, true) {
                Ok(runtime) => runtime,
                Err(error) => {
                    let name = discovered_dir
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or("Unknown plugin")
                        .to_string();
                    items.push(ExtensionSummaryDto {
                        id: format!("discovered:{}", discovered_dir.display()),
                        kind: ExtensionKind::Plugin,
                        name,
                        version: "0.0.0".to_string(),
                        description: Some(error.user_message),
                        source: ExtensionSourceDto::LocalDir {
                            path: discovered_dir.to_string_lossy().to_string(),
                        },
                        install_state: ExtensionInstallState::Error,
                        health: ExtensionHealth::Error,
                        permissions: Vec::new(),
                        tags: vec!["local".to_string()],
                    });
                    continue;
                }
            };

            if installed_ids.contains(&runtime.manifest.id) {
                continue;
            }

            items.push(self.build_plugin_summary(
                &runtime,
                ExtensionInstallState::Discovered,
                None,
            ));
        }

        Ok(items)
    }

    pub(super) async fn load_plugin_runtimes(
        &self,
    ) -> Result<Vec<InstalledPluginRuntime>, AppError> {
        let installed = self.load_installed_plugin_records().await?;
        let mut items = Vec::with_capacity(installed.len());
        for record in installed {
            let mut runtime = self.load_plugin_from_dir(Path::new(&record.path), false)?;
            runtime.enabled = record.enabled;
            items.push(runtime);
        }
        items.sort_by(|left, right| {
            left.manifest
                .name
                .to_lowercase()
                .cmp(&right.manifest.name.to_lowercase())
        });
        Ok(items)
    }

    pub(super) async fn load_enabled_plugin_runtimes(
        &self,
    ) -> Result<Vec<InstalledPluginRuntime>, AppError> {
        Ok(self
            .load_plugin_runtimes()
            .await?
            .into_iter()
            .filter(|plugin| plugin.enabled)
            .collect())
    }

    pub(super) async fn load_registered_plugin_commands(
        &self,
    ) -> Result<Vec<ExtensionCommandDto>, AppError> {
        Ok(self
            .load_enabled_plugin_runtimes()
            .await?
            .into_iter()
            .flat_map(|plugin| {
                self.load_plugin_command_definitions(&plugin)
                    .into_iter()
                    .filter_map(move |command| {
                        command.prompt_template.clone().map(|prompt_template| {
                            PluginCommandRegistration {
                                plugin_id: plugin.manifest.id.clone(),
                                command: PluginManifestCommand {
                                    prompt_template: Some(prompt_template),
                                    ..command
                                },
                            }
                        })
                    })
            })
            .map(|registration| ExtensionCommandDto {
                plugin_id: registration.plugin_id,
                name: registration.command.name,
                description: registration.command.description,
                prompt_template: registration.command.prompt_template.unwrap_or_default(),
            })
            .collect())
    }

    pub(super) fn load_plugin_command_definitions(
        &self,
        plugin: &InstalledPluginRuntime,
    ) -> Vec<PluginManifestCommand> {
        let mut commands = plugin.manifest.commands.clone();
        let commands_root = plugin.path.join("commands");
        if !commands_root.is_dir() {
            return commands;
        }

        for entry in fs::read_dir(&commands_root).ok().into_iter().flatten() {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            if commands.iter().any(|command| command.name == name) {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path) else {
                tracing::warn!(path = %path.display(), "failed to read plugin command file");
                continue;
            };
            let Some(command) = parse_plugin_command_markdown(&raw, name) else {
                tracing::warn!(path = %path.display(), "failed to parse plugin command file");
                continue;
            };
            commands.push(command);
        }

        commands.sort_by(|left, right| left.name.cmp(&right.name));
        commands
    }

    pub(super) async fn load_plugin_hook_registrations(
        &self,
        event: &str,
    ) -> Result<Vec<PluginHookRegistration>, AppError> {
        Ok(self
            .load_enabled_plugin_runtimes()
            .await?
            .into_iter()
            .flat_map(|plugin| {
                self.handlers_for_event(&plugin.manifest, event)
                    .into_iter()
                    .map(move |handler| PluginHookRegistration {
                        plugin: plugin.clone(),
                        handler,
                    })
            })
            .collect())
    }

    pub(super) fn load_plugin_from_dir(
        &self,
        dir: &Path,
        discovered: bool,
    ) -> Result<InstalledPluginRuntime, AppError> {
        let plugin_dir = dunce::canonicalize(dir).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_path",
                format!(
                    "Unable to access plugin directory '{}': {error}",
                    dir.display()
                ),
            )
        })?;
        let manifest_path = if plugin_dir.join("plugin.json").is_file() {
            plugin_dir.join("plugin.json")
        } else {
            plugin_dir.join(".claude-plugin/plugin.json")
        };
        let manifest_raw = fs::read_to_string(&manifest_path).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.missing_manifest",
                format!(
                    "Unable to read '{}' for plugin '{}': {error}",
                    manifest_path.display(),
                    dir.display()
                ),
            )
        })?;
        let manifest = parse_plugin_manifest(&manifest_raw, &plugin_dir).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_manifest",
                format!(
                    "Invalid plugin manifest in '{}': {error}",
                    manifest_path.display()
                ),
            )
        })?;

        if !discovered && !plugin_dir.exists() {
            return Err(AppError::not_found(
                ErrorSource::Settings,
                format!("plugin directory '{}'", plugin_dir.display()),
            ));
        }

        Ok(InstalledPluginRuntime {
            enabled: manifest.default_enabled.unwrap_or(true),
            manifest,
            path: plugin_dir,
        })
    }

    pub(super) fn build_plugin_summary(
        &self,
        plugin: &InstalledPluginRuntime,
        install_state: ExtensionInstallState,
        last_error: Option<String>,
    ) -> ExtensionSummaryDto {
        ExtensionSummaryDto {
            id: plugin.manifest.id.clone(),
            kind: ExtensionKind::Plugin,
            name: plugin.manifest.name.clone(),
            version: plugin.manifest.version.clone(),
            description: plugin.manifest.description.clone().or(last_error.clone()),
            source: ExtensionSourceDto::LocalDir {
                path: plugin.path.to_string_lossy().to_string(),
            },
            install_state: install_state.clone(),
            health: match install_state {
                ExtensionInstallState::Enabled => ExtensionHealth::Healthy,
                ExtensionInstallState::Error => ExtensionHealth::Error,
                _ => ExtensionHealth::Unknown,
            },
            permissions: plugin.manifest.permissions.clone(),
            tags: plugin.manifest.capabilities.clone(),
        }
    }

    pub(super) fn build_plugin_detail(
        &self,
        plugin: &InstalledPluginRuntime,
        last_error: Option<String>,
    ) -> PluginDetailDto {
        let command_names = self.collect_command_bundle_names(&plugin.path, &plugin.manifest);
        let commands = command_names
            .into_iter()
            .map(|name| {
                plugin
                    .manifest
                    .commands
                    .iter()
                    .find(|command| command.name == name)
                    .cloned()
                    .unwrap_or(PluginManifestCommand {
                        name,
                        description: "Bundled command".to_string(),
                        prompt_template: None,
                    })
            })
            .map(|command| PluginCommandDto {
                name: command.name,
                description: command.description,
                prompt_template: command.prompt_template,
            })
            .collect();

        PluginDetailDto {
            id: plugin.manifest.id.clone(),
            path: plugin.path.to_string_lossy().to_string(),
            author: plugin.manifest.author.clone(),
            homepage: plugin.manifest.homepage.clone(),
            default_enabled: plugin.manifest.default_enabled.unwrap_or(true),
            enabled: plugin.enabled,
            capabilities: plugin.manifest.capabilities.clone(),
            permissions: plugin.manifest.permissions.clone(),
            hooks: self.build_plugin_hook_groups(&plugin.manifest),
            tools: plugin
                .manifest
                .tools
                .iter()
                .map(|tool| PluginToolDto {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    command: tool.command.clone(),
                    args: tool.args.clone(),
                    cwd: tool.cwd.clone(),
                    timeout_ms: tool.timeout_ms,
                    required_permission: tool.required_permission.clone(),
                })
                .collect(),
            commands,
            bundled_skills: self
                .collect_skill_bundle_names(&plugin.path, plugin.manifest.skills_dir.as_deref()),
            bundled_mcp_servers: self.collect_mcp_bundle_names(&plugin.path),
            timeout_ms: plugin.manifest.timeout_ms,
            skills_dir: plugin.manifest.skills_dir.clone(),
            config_schema_path: plugin
                .manifest
                .config_schema
                .as_ref()
                .map(|schema| schema.path.clone()),
            last_error,
        }
    }

    pub(super) fn discover_plugin_dirs(&self) -> Result<Vec<PathBuf>, AppError> {
        let base = tiy_home().join("plugins");
        let mut dirs = Vec::new();
        if !base.exists() {
            return Ok(dirs);
        }
        for entry in fs::read_dir(base)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
        Ok(dirs)
    }

    pub(super) async fn load_installed_plugin_records(
        &self,
    ) -> Result<Vec<InstalledPluginRecord>, AppError> {
        let file = global_plugins_config_path();
        let records = self
            .read_json_file_with_diagnostics::<Vec<InstalledPluginRecord>>(
                &file,
                "plugins",
                ConfigScope::Global,
            )?
            .value;
        if !records.is_empty() {
            return Ok(records);
        }

        let legacy_records = self
            .read_json_setting::<Vec<InstalledPluginRecord>>(
                LEGACY_EXTENSIONS_INSTALLED_PLUGINS_KEY,
            )
            .await?;
        if !legacy_records.is_empty() {
            self.save_installed_plugin_records(&legacy_records).await?;
            let _ =
                settings_repo::delete(&self.pool, LEGACY_EXTENSIONS_INSTALLED_PLUGINS_KEY).await;
            return Ok(legacy_records);
        }

        Ok(records)
    }

    pub(super) async fn save_installed_plugin_records(
        &self,
        records: &[InstalledPluginRecord],
    ) -> Result<(), AppError> {
        self.write_json_file(&global_plugins_config_path(), records)
    }

    pub(super) async fn load_plugin_config_store(&self) -> Result<PluginConfigStore, AppError> {
        self.read_json_setting(EXTENSIONS_PLUGIN_CONFIG_KEY).await
    }

    pub(super) async fn save_plugin_config_store(
        &self,
        store: &PluginConfigStore,
    ) -> Result<(), AppError> {
        self.write_json_setting(EXTENSIONS_PLUGIN_CONFIG_KEY, store)
            .await
    }

    pub(super) async fn update_plugin_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<bool, AppError> {
        let mut records = self.load_installed_plugin_records().await?;
        let mut found = false;
        let mut target_path = None;
        for record in &mut records {
            if record.id == id {
                record.enabled = enabled;
                target_path = Some(record.path.clone());
                found = true;
                break;
            }
        }
        if found {
            self.save_installed_plugin_records(&records).await?;
            if let Some(path) = target_path {
                let mut plugin = self.load_plugin_from_dir(Path::new(&path), false)?;
                plugin.enabled = enabled;
                self.sync_plugin_managed_mcp_configs(&plugin).await?;
            }
        }
        Ok(found)
    }

    pub(super) async fn uninstall_plugin(&self, id: &str) -> Result<bool, AppError> {
        let mut records = self.load_installed_plugin_records().await?;
        let before = records.len();
        records.retain(|record| record.id != id);
        if before == records.len() {
            return Ok(false);
        }
        self.save_installed_plugin_records(&records).await?;
        self.remove_plugin_managed_mcp_configs(id).await?;
        Ok(true)
    }

    pub(super) fn build_plugin_hook_groups(
        &self,
        manifest: &PluginManifest,
    ) -> Vec<PluginHookGroupDto> {
        let mut hook_groups = Vec::new();
        for (event, handlers) in [
            (
                "pre_tool_use",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.pre_tool_use.clone()),
            ),
            (
                "post_tool_use",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.post_tool_use.clone()),
            ),
            (
                "run_started",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.on_run_start.clone()),
            ),
            (
                "run_finished",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.on_run_complete.clone()),
            ),
        ] {
            if let Some(handlers) = handlers {
                hook_groups.push(PluginHookGroupDto {
                    event: event.to_string(),
                    handlers,
                });
            }
        }
        hook_groups
    }

    pub(super) fn collect_skill_bundle_names(
        &self,
        plugin_dir: &Path,
        skills_dir: Option<&str>,
    ) -> Vec<String> {
        let skills_root = plugin_dir.join(skills_dir.unwrap_or("skills"));
        if !skills_root.is_dir() {
            return Vec::new();
        }

        let mut items = read_child_dirs(&skills_root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|skill_dir| {
                let skill_file = skill_dir.join("SKILL.md");
                if !skill_file.is_file() {
                    return None;
                }
                let raw = fs::read_to_string(&skill_file).ok()?;
                let (record, _) = parse_skill_markdown(&raw, &skill_dir, "plugin")?;
                Some(record.name)
            })
            .collect::<Vec<_>>();
        items.sort();
        items
    }

    pub(super) fn collect_command_bundle_names(
        &self,
        plugin_dir: &Path,
        manifest: &PluginManifest,
    ) -> Vec<String> {
        let mut items = manifest
            .commands
            .iter()
            .map(|command| command.name.clone())
            .collect::<Vec<_>>();
        let commands_root = plugin_dir.join("commands");
        if commands_root.is_dir() {
            for entry in fs::read_dir(&commands_root).ok().into_iter().flatten() {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let candidate = if path.is_file() {
                    path.file_stem()
                } else if path.is_dir() {
                    path.file_name()
                } else {
                    None
                };
                let Some(name) = candidate.and_then(OsStr::to_str) else {
                    continue;
                };
                if !name.is_empty() {
                    items.push(name.to_string());
                }
            }
        }
        items.sort();
        items.dedup();
        items
    }

    pub(super) fn collect_mcp_bundle_names(&self, plugin_dir: &Path) -> Vec<String> {
        let mcp_path = plugin_dir.join(".mcp.json");
        if !mcp_path.is_file() {
            return Vec::new();
        }
        let raw = match fs::read_to_string(&mcp_path) {
            Ok(raw) => raw,
            Err(_) => return Vec::new(),
        };
        let value = match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };

        let mut items = match value.get("servers") {
            Some(serde_json::Value::Object(map)) => map.keys().cloned().collect::<Vec<_>>(),
            Some(serde_json::Value::Array(servers)) => servers
                .iter()
                .filter_map(read_named_value)
                .collect::<Vec<_>>(),
            _ => match value {
                serde_json::Value::Object(map) => map
                    .iter()
                    .filter_map(|(key, value)| {
                        if value.is_object() {
                            Some(read_named_value(value).unwrap_or_else(|| key.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            },
        };
        items.sort();
        items.dedup();
        items
    }

    pub(super) fn load_plugin_managed_mcp_configs(
        &self,
        plugin: &InstalledPluginRuntime,
    ) -> Result<Vec<McpServerConfigInput>, AppError> {
        let mcp_path = plugin.path.join(".mcp.json");
        if !mcp_path.is_file() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&mcp_path).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_mcp_bundle",
                format!("Unable to read '{}': {error}", mcp_path.display()),
            )
        })?;
        let value = serde_json::from_str::<serde_json::Value>(&raw).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_mcp_bundle",
                format!("Invalid MCP bundle '{}': {error}", mcp_path.display()),
            )
        })?;

        Ok(read_plugin_mcp_server_entries(&value)
            .into_iter()
            .filter_map(|(server_name, spec)| {
                build_plugin_managed_mcp_config(&plugin.manifest, server_name, spec)
            })
            .collect())
    }

    pub(super) async fn sync_plugin_managed_mcp_configs(
        &self,
        plugin: &InstalledPluginRuntime,
    ) -> Result<(), AppError> {
        let mut configs = self
            .load_mcp_configs_for_scope(None, ConfigScope::Global)
            .await?;
        let prefix = plugin_managed_mcp_prefix(&plugin.manifest.id);
        let existing_managed_configs = configs
            .iter()
            .filter(|config| config.id.starts_with(&prefix))
            .map(|config| (config.id.clone(), config.clone()))
            .collect::<HashMap<_, _>>();
        configs.retain(|config| !config.id.starts_with(&prefix));
        let managed_configs = if plugin.enabled {
            self.load_plugin_managed_mcp_configs(plugin)?
                .into_iter()
                .map(|config| {
                    merge_plugin_managed_mcp_config(
                        existing_managed_configs.get(&config.id),
                        config,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        if plugin.enabled {
            configs.extend(managed_configs.clone());
            configs
                .sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
        }
        self.save_mcp_configs_for_scope(&configs, None, ConfigScope::Global)
            .await?;
        self.remove_mcp_runtime_records_with_prefix(&prefix).await?;
        for config in managed_configs {
            self.refresh_mcp_runtime(&config, None, ConfigScope::Global.as_str())
                .await?;
        }
        Ok(())
    }

    pub(super) async fn remove_plugin_managed_mcp_configs(
        &self,
        plugin_id: &str,
    ) -> Result<(), AppError> {
        let mut configs = self
            .load_mcp_configs_for_scope(None, ConfigScope::Global)
            .await?;
        let prefix = plugin_managed_mcp_prefix(plugin_id);
        let before = configs.len();
        configs.retain(|config| !config.id.starts_with(&prefix));
        if before == configs.len() {
            return Ok(());
        }
        self.save_mcp_configs_for_scope(&configs, None, ConfigScope::Global)
            .await?;
        self.remove_mcp_runtime_records_with_prefix(&prefix).await
    }

    pub(super) fn handlers_for_event(&self, manifest: &PluginManifest, event: &str) -> Vec<String> {
        let hooks = match manifest.hooks.as_ref() {
            Some(hooks) => hooks,
            None => return Vec::new(),
        };

        match event {
            "pre_tool_use" => hooks.pre_tool_use.clone().unwrap_or_default(),
            "post_tool_use" => hooks.post_tool_use.clone().unwrap_or_default(),
            "run_started" => hooks.on_run_start.clone().unwrap_or_default(),
            "run_finished" => hooks.on_run_complete.clone().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub(super) async fn execute_hook(
        &self,
        plugin: &InstalledPluginRuntime,
        handler: &str,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<HookOutput, AppError> {
        let command_path = plugin.path.join(handler);
        // Prevent path traversal: ensure the resolved command stays within the plugin directory.
        let command_path = std::fs::canonicalize(&command_path).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.plugin.hook_not_found",
                format!(
                    "Hook '{}' from plugin '{}' could not be resolved: {error}",
                    handler, plugin.manifest.id
                ),
            )
        })?;
        let plugin_root = std::fs::canonicalize(&plugin.path).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.plugin.plugin_path_invalid",
                format!(
                    "Plugin '{}' path could not be resolved: {error}",
                    plugin.manifest.id
                ),
            )
        })?;
        if !command_path.starts_with(&plugin_root) {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.plugin.hook_escape",
                format!(
                    "Hook '{}' from plugin '{}' resolved outside the plugin directory",
                    handler, plugin.manifest.id
                ),
            ));
        }
        let output = self
            .execute_command_json(
                command_path.as_os_str(),
                &[],
                plugin.path.as_path(),
                DEFAULT_HOOK_TIMEOUT_MS,
                &HookInput { event, payload },
                None,
            )
            .await?;
        serde_json::from_slice::<HookOutput>(&output.stdout).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.plugin.invalid_hook_output",
                format!(
                    "Hook '{}' from plugin '{}' returned invalid JSON: {error}",
                    handler, plugin.manifest.id
                ),
            )
        })
    }

    pub(super) async fn execute_plugin_tool(
        &self,
        plugin: &InstalledPluginRuntime,
        tool: &PluginManifestTool,
        tool_input: &serde_json::Value,
        workspace_path: &str,
        thread_id: Option<&str>,
    ) -> Result<ToolOutput, AppError> {
        let timeout_ms = tool
            .timeout_ms
            .or(plugin.manifest.timeout_ms)
            .unwrap_or(DEFAULT_PLUGIN_TIMEOUT_MS)
            .min(300_000);

        let variables = build_plugin_variables(workspace_path, &plugin.path, thread_id);
        let args = tool
            .args
            .iter()
            .map(|arg| substitute_variables(arg, &variables))
            .collect::<Vec<_>>();
        let cwd = tool
            .cwd
            .as_deref()
            .map(|cwd| PathBuf::from(substitute_variables(cwd, &variables)))
            .unwrap_or_else(|| plugin.path.clone());
        let env = tool.env.as_ref().map(|env| {
            env.iter()
                .map(|(key, value)| (key.clone(), substitute_variables(value, &variables)))
                .collect::<Vec<_>>()
        });

        let output = self
            .execute_command_json(
                OsStr::new(&tool.command),
                &args,
                &cwd,
                timeout_ms,
                &PluginToolInput {
                    args: tool_input,
                    workspace: workspace_path,
                    thread_id,
                },
                env.as_deref(),
            )
            .await?;

        let parsed =
            serde_json::from_slice::<PluginToolOutput>(&output.stdout).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.plugin.invalid_tool_output",
                    format!(
                        "Plugin tool '{}' from '{}' returned invalid JSON: {error}",
                        tool.name, plugin.manifest.id
                    ),
                )
            })?;

        Ok(ToolOutput {
            success: parsed.success,
            result: match (parsed.result, parsed.error) {
                (Some(result), _) => result,
                (None, Some(error)) => serde_json::json!({ "error": error }),
                (None, None) => serde_json::json!({ "ok": parsed.success }),
            },
        })
    }

    pub(super) async fn execute_command_json<T: Serialize>(
        &self,
        program: &OsStr,
        args: &[String],
        cwd: &Path,
        timeout_ms: u64,
        stdin_payload: &T,
        env: Option<&[(String, String)]>,
    ) -> Result<std::process::Output, AppError> {
        let mut command = Command::new(program);
        command.args(args);
        command.current_dir(cwd);
        command.kill_on_drop(true);
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        if let Some(env_pairs) = env {
            for (key, value) in env_pairs {
                command.env(key, value);
            }
        }

        let mut child = command.spawn().map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.command.spawn_failed",
                format!(
                    "Failed to start '{}': {error}",
                    Path::new(program).display()
                ),
            )
        })?;

        let payload = serde_json::to_vec(stdin_payload).map_err(|error| {
            AppError::internal(
                ErrorSource::Tool,
                format!("Failed to serialize command payload: {error}"),
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                let _ = stdin.write_all(&payload).await;
            });
        }

        let wait = child.wait_with_output();
        let output = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), wait)
            .await
            .map_err(|_| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.command.timeout",
                    format!("Extension command timed out after {timeout_ms}ms"),
                )
            })??;

        if !output.status.success() {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.command.non_zero_exit",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        Ok(output)
    }
}

// --- Plugin free functions ---

pub(super) fn parse_plugin_manifest(
    raw: &str,
    plugin_dir: &Path,
) -> Result<PluginManifest, String> {
    let value = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|error| format!("manifest is not valid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "manifest root must be a JSON object".to_string())?;
    let fallback_name = plugin_dir
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("plugin")
        .to_string();

    let commands = object
        .get("commands")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let command = entry.as_object()?;
                    let name = read_string_keys(command, &["name", "id", "label"])
                        .unwrap_or_else(|| format!("command-{}", index + 1));
                    Some(PluginManifestCommand {
                        description: read_string_keys(command, &["description"])
                            .unwrap_or_else(|| name.clone()),
                        name,
                        prompt_template: read_string_keys(
                            command,
                            &["promptTemplate", "prompt_template"],
                        ),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let tools = object
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let tool = entry.as_object()?;
                    let command = read_string_keys(tool, &["command", "cmd"])?;
                    let name = read_string_keys(tool, &["name", "id", "label"])
                        .unwrap_or_else(|| format!("tool-{}", index + 1));
                    Some(PluginManifestTool {
                        name: name.clone(),
                        description: read_string_keys(tool, &["description"])
                            .unwrap_or_else(|| name.clone()),
                        command,
                        args: read_string_array_keys(tool, &["args"]),
                        env: read_string_map_keys(tool, &["env"]),
                        cwd: read_string_keys(tool, &["cwd"]),
                        timeout_ms: read_u64_keys(tool, &["timeoutMs", "timeout_ms"]),
                        required_permission: read_string_keys(
                            tool,
                            &["requiredPermission", "required_permission"],
                        )
                        .unwrap_or_else(|| "read".to_string()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(PluginManifest {
        id: read_string_keys(object, &["id"]).unwrap_or_else(|| fallback_name.clone()),
        name: read_string_keys(object, &["name", "title"]).unwrap_or_else(|| fallback_name.clone()),
        version: read_string_keys(object, &["version"]).unwrap_or_else(|| "0.0.0".to_string()),
        description: read_string_keys(object, &["description", "summary"]),
        author: read_author_name(object.get("author")),
        homepage: read_string_keys(object, &["homepage", "repository", "url"]),
        default_enabled: read_bool_keys(object, &["defaultEnabled", "default_enabled"]),
        capabilities: read_string_array_keys(object, &["capabilities"]),
        permissions: read_string_array_keys(object, &["permissions"]),
        hooks: read_plugin_hooks(object.get("hooks")),
        tools,
        commands,
        timeout_ms: read_u64_keys(object, &["timeoutMs", "timeout_ms"]),
        skills_dir: read_string_keys(object, &["skillsDir", "skills_dir"]),
        config_schema: read_config_schema(
            object
                .get("configSchema")
                .or_else(|| object.get("config_schema")),
        ),
    })
}

pub(super) fn parse_plugin_command_markdown(
    raw: &str,
    fallback_name: &str,
) -> Option<PluginManifestCommand> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (description, prompt_template) = if let Some((frontmatter, body)) = split_frontmatter(raw) {
        let meta = parse_frontmatter_map(frontmatter);
        let description = meta
            .get("description")
            .cloned()
            .unwrap_or_else(|| format!("Plugin command /{fallback_name}"));
        let prompt = body.trim().to_string();
        (description, prompt)
    } else {
        (
            format!("Plugin command /{fallback_name}"),
            trimmed.to_string(),
        )
    };

    if prompt_template.is_empty() {
        return None;
    }

    Some(PluginManifestCommand {
        name: fallback_name.to_string(),
        description,
        prompt_template: Some(prompt_template),
    })
}

pub(super) fn read_named_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("label")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("name").and_then(serde_json::Value::as_str))
        .or_else(|| value.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

pub(super) fn plugin_managed_mcp_prefix(plugin_id: &str) -> String {
    format!("plugin::{plugin_id}::")
}

pub(super) fn plugin_managed_mcp_id(plugin_id: &str, server_name: &str) -> String {
    format!("{}{}", plugin_managed_mcp_prefix(plugin_id), server_name)
}

pub(super) fn read_plugin_mcp_server_entries<'a>(
    value: &'a serde_json::Value,
) -> Vec<(String, &'a serde_json::Map<String, serde_json::Value>)> {
    let mut entries = Vec::new();
    match value.get("servers") {
        Some(serde_json::Value::Object(map)) => {
            for (server_name, spec) in map {
                if let Some(spec) = spec.as_object() {
                    entries.push((server_name.clone(), spec));
                }
            }
        }
        Some(serde_json::Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                let Some(spec) = item.as_object() else {
                    continue;
                };
                let server_name = read_string_keys(spec, &["id", "name", "label"])
                    .unwrap_or_else(|| format!("server-{}", index + 1));
                entries.push((server_name, spec));
            }
        }
        _ => {
            if let Some(map) = value.as_object() {
                for (server_name, spec) in map {
                    if let Some(spec) = spec.as_object() {
                        entries.push((server_name.clone(), spec));
                    }
                }
            }
        }
    }
    entries
}

pub(super) fn build_plugin_managed_mcp_config(
    manifest: &PluginManifest,
    server_name: String,
    spec: &serde_json::Map<String, serde_json::Value>,
) -> Option<McpServerConfigInput> {
    let raw_transport = read_string_keys(spec, &["transport", "type"]);
    let has_url = read_string_keys(spec, &["url"]).is_some();
    let has_command = read_string_keys(spec, &["command", "cmd"]).is_some();
    let transport = match raw_transport.as_deref() {
        Some("streamable-http") | Some("http") | Some("https") => "streamable-http".to_string(),
        Some("stdio") => "stdio".to_string(),
        Some(other) if other.contains("http") => "streamable-http".to_string(),
        _ if has_url => "streamable-http".to_string(),
        _ if has_command => "stdio".to_string(),
        _ => return None,
    };
    let label = read_string_keys(spec, &["label", "name"])
        .unwrap_or_else(|| build_plugin_managed_mcp_label(&manifest.name, &server_name));
    Some(McpServerConfigInput {
        id: plugin_managed_mcp_id(&manifest.id, &server_name),
        label,
        transport,
        enabled: true,
        auto_start: true,
        command: read_string_keys(spec, &["command", "cmd"]),
        args: Some(read_string_array_keys(spec, &["args"])),
        env: read_string_map_keys(spec, &["env"]),
        cwd: read_string_keys(spec, &["cwd"]),
        url: read_string_keys(spec, &["url"]),
        headers: read_string_map_keys(spec, &["headers"]),
        timeout_ms: read_u64_keys(spec, &["timeoutMs", "timeout_ms"]),
    })
}

pub(super) fn build_plugin_managed_mcp_label(plugin_name: &str, server_name: &str) -> String {
    let plugin_name = plugin_name.trim();
    let server_name = server_name.trim();

    if plugin_name.is_empty() {
        return server_name.to_string();
    }
    if server_name.is_empty() {
        return plugin_name.to_string();
    }
    if plugin_managed_mcp_names_match(plugin_name, server_name) {
        return plugin_name.to_string();
    }

    format!("{plugin_name} / {server_name}")
}

pub(super) fn plugin_managed_mcp_names_match(left: &str, right: &str) -> bool {
    normalize_plugin_managed_mcp_name(left) == normalize_plugin_managed_mcp_name(right)
}

pub(super) fn normalize_plugin_managed_mcp_name(value: &str) -> String {
    value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn merge_plugin_managed_mcp_config(
    existing: Option<&McpServerConfigInput>,
    managed: McpServerConfigInput,
) -> McpServerConfigInput {
    let Some(existing) = existing else {
        return managed;
    };

    McpServerConfigInput {
        enabled: existing.enabled,
        auto_start: existing.auto_start,
        env: existing.env.clone().or(managed.env.clone()),
        cwd: existing.cwd.clone().or(managed.cwd.clone()),
        headers: existing.headers.clone().or(managed.headers.clone()),
        timeout_ms: existing.timeout_ms.or(managed.timeout_ms),
        ..managed
    }
}

pub(super) fn read_string_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .filter_map(|key| object.get(*key))
        .find_map(read_string_value)
}

pub(super) fn read_string_value(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::trim).and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

pub(super) fn read_string_array_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Vec<String> {
    let mut items = keys
        .iter()
        .filter_map(|key| object.get(*key))
        .find_map(|value| match value {
            serde_json::Value::Array(entries) => Some(
                entries
                    .iter()
                    .filter_map(read_string_value)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    items.sort();
    items.dedup();
    items
}

pub(super) fn read_string_map_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<HashMap<String, String>> {
    let map = keys
        .iter()
        .filter_map(|key| object.get(*key))
        .find_map(|value| match value {
            serde_json::Value::Object(map) => Some(
                map.iter()
                    .filter_map(|(key, value)| {
                        read_string_value(value).map(|value| (key.clone(), value))
                    })
                    .collect::<HashMap<_, _>>(),
            ),
            _ => None,
        })?;
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

pub(super) fn read_bool_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<bool> {
    keys.iter()
        .filter_map(|key| object.get(*key))
        .find_map(serde_json::Value::as_bool)
}

pub(super) fn read_u64_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<u64> {
    keys.iter()
        .filter_map(|key| object.get(*key))
        .find_map(serde_json::Value::as_u64)
}

pub(super) fn read_author_name(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    read_string_value(value).or_else(|| {
        value
            .as_object()
            .and_then(|object| read_string_keys(object, &["name", "label", "id"]))
    })
}

pub(super) fn read_plugin_hooks(value: Option<&serde_json::Value>) -> Option<PluginManifestHooks> {
    let object = value?.as_object()?;
    let pre_tool_use = read_optional_string_array_keys(object, &["preToolUse", "pre_tool_use"]);
    let post_tool_use = read_optional_string_array_keys(object, &["postToolUse", "post_tool_use"]);
    let on_run_start = read_optional_string_array_keys(object, &["onRunStart", "on_run_start"]);
    let on_run_complete =
        read_optional_string_array_keys(object, &["onRunComplete", "on_run_complete"]);
    let hooks = PluginManifestHooks {
        pre_tool_use,
        post_tool_use,
        on_run_start,
        on_run_complete,
    };
    if hooks.pre_tool_use.is_none()
        && hooks.post_tool_use.is_none()
        && hooks.on_run_start.is_none()
        && hooks.on_run_complete.is_none()
    {
        None
    } else {
        Some(hooks)
    }
}

pub(super) fn read_optional_string_array_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<Vec<String>> {
    let items = read_string_array_keys(object, keys);
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

pub(super) fn read_config_schema(
    value: Option<&serde_json::Value>,
) -> Option<PluginManifestSchema> {
    let object = value?.as_object()?;
    let path = read_string_keys(object, &["path"])?;
    Some(PluginManifestSchema {
        r#type: read_string_keys(object, &["type"]).unwrap_or_else(|| "json-schema".to_string()),
        path,
    })
}

pub(super) fn build_plugin_variables(
    workspace_path: &str,
    plugin_dir: &Path,
    thread_id: Option<&str>,
) -> HashMap<String, String> {
    let mut values = HashMap::new();
    values.insert("workspace".to_string(), workspace_path.to_string());
    values.insert(
        "plugin_dir".to_string(),
        plugin_dir.to_string_lossy().to_string(),
    );
    if let Some(thread_id) = thread_id {
        values.insert("thread_id".to_string(), thread_id.to_string());
    }
    values
}

pub(super) fn substitute_variables(input: &str, variables: &HashMap<String, String>) -> String {
    variables
        .iter()
        .fold(input.to_string(), |current, (key, value)| {
            current.replace(&format!("${{{key}}}"), value)
        })
}

pub(super) fn mask_sensitive_value(value: &str) -> String {
    let len = value.chars().count();
    if len <= 4 {
        return "****".to_string();
    }
    let prefix = value.chars().take(4).collect::<String>();
    format!("{prefix}****")
}

pub(super) fn mask_url(value: String) -> String {
    let Some((base, _query)) = value.split_once('?') else {
        return value;
    };
    format!("{base}?****")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn extension_builders_render_plugin_mcp_skill_and_marketplace_dtos() {
        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );
        let plugin_dir = tempdir().expect("plugin dir");
        let commands_dir = plugin_dir.path().join("commands");
        let skills_dir = plugin_dir.path().join("skills/alpha-skill");
        fs::create_dir_all(&commands_dir).expect("commands dir");
        fs::create_dir_all(&skills_dir).expect("skills dir");
        fs::write(commands_dir.join("fix.md"), "Fix it").expect("command file");
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: Alpha Skill\ntags:\n- docs\n---\nSkill body",
        )
        .expect("skill file");
        fs::write(
            plugin_dir.path().join(".mcp.json"),
            serde_json::json!({
                "servers": [
                    { "name": "Docs Server" },
                    { "id": "api-server", "label": "API Server" }
                ]
            })
            .to_string(),
        )
        .expect("mcp bundle");

        let manifest = PluginManifest {
            id: "demo.plugin".to_string(),
            name: "Demo Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Demo description".to_string()),
            author: Some("Ada".to_string()),
            homepage: Some("https://example.com".to_string()),
            default_enabled: Some(false),
            capabilities: vec!["tools".to_string(), "commands".to_string()],
            permissions: vec!["shell".to_string()],
            hooks: Some(PluginManifestHooks {
                pre_tool_use: Some(vec!["pre".to_string()]),
                post_tool_use: Some(vec!["post".to_string()]),
                on_run_start: Some(vec!["start".to_string()]),
                on_run_complete: Some(vec!["done".to_string()]),
            }),
            tools: vec![PluginManifestTool {
                name: "run".to_string(),
                description: "Run a task".to_string(),
                command: "node".to_string(),
                args: vec!["tool.js".to_string()],
                env: None,
                cwd: Some("tools".to_string()),
                timeout_ms: Some(1234),
                required_permission: "shell".to_string(),
            }],
            commands: vec![PluginManifestCommand {
                name: "review".to_string(),
                description: "Review code".to_string(),
                prompt_template: Some("Review this".to_string()),
            }],
            timeout_ms: Some(30_000),
            skills_dir: Some("skills".to_string()),
            config_schema: Some(PluginManifestSchema {
                r#type: "json-schema".to_string(),
                path: "schema.json".to_string(),
            }),
        };
        let runtime = InstalledPluginRuntime {
            manifest: manifest.clone(),
            path: plugin_dir.path().to_path_buf(),
            enabled: true,
        };

        let summary = manager.build_plugin_summary(
            &runtime,
            ExtensionInstallState::Enabled,
            Some("ignored because description exists".to_string()),
        );
        assert_eq!(summary.kind, ExtensionKind::Plugin);
        assert_eq!(summary.health, ExtensionHealth::Healthy);
        assert_eq!(summary.description.as_deref(), Some("Demo description"));
        assert_eq!(summary.permissions, vec!["shell".to_string()]);
        assert_eq!(
            summary.tags,
            vec!["tools".to_string(), "commands".to_string()]
        );
        assert!(matches!(
            summary.source,
            ExtensionSourceDto::LocalDir { .. }
        ));

        let error_summary = manager.build_plugin_summary(
            &InstalledPluginRuntime {
                manifest: PluginManifest {
                    description: None,
                    ..manifest.clone()
                },
                ..runtime.clone()
            },
            ExtensionInstallState::Error,
            Some("load failed".to_string()),
        );
        assert_eq!(error_summary.health, ExtensionHealth::Error);
        assert_eq!(error_summary.description.as_deref(), Some("load failed"));

        let detail = manager.build_plugin_detail(&runtime, Some("previous error".to_string()));
        assert_eq!(detail.author.as_deref(), Some("Ada"));
        assert!(!detail.default_enabled);
        assert!(detail.enabled);
        assert_eq!(detail.hooks.len(), 4);
        assert_eq!(detail.tools[0].required_permission, "shell");
        assert_eq!(
            detail
                .commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["fix", "review"]
        );
        assert_eq!(detail.bundled_skills, vec!["Alpha Skill".to_string()]);
        assert_eq!(
            detail.bundled_mcp_servers,
            vec!["API Server".to_string(), "Docs Server".to_string()]
        );
        assert_eq!(detail.config_schema_path.as_deref(), Some("schema.json"));
        assert_eq!(detail.last_error.as_deref(), Some("previous error"));

        let mcp_state = |status: &str, enabled: bool, stale_snapshot: bool, transport: &str| {
            McpServerStateDto {
                id: format!("docs-{status}"),
                label: "Docs".to_string(),
                scope: "global".to_string(),
                status: status.to_string(),
                phase: "ready".to_string(),
                tools: Vec::new(),
                resources: Vec::new(),
                stale_snapshot,
                last_error: Some("runtime note".to_string()),
                updated_at: "now".to_string(),
                config: McpServerConfigDto {
                    id: format!("docs-{status}"),
                    label: "Docs".to_string(),
                    transport: transport.to_string(),
                    enabled,
                    auto_start: true,
                    command: None,
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: Some("https://example.com/mcp".to_string()),
                    headers: HashMap::new(),
                    timeout_ms: None,
                },
            }
        };
        let connected =
            manager.build_mcp_summary(&mcp_state("connected", true, false, "streamable-http"));
        assert_eq!(connected.install_state, ExtensionInstallState::Enabled);
        assert_eq!(connected.health, ExtensionHealth::Healthy);
        assert_eq!(connected.permissions, vec!["network-access".to_string()]);
        let degraded = manager.build_mcp_summary(&mcp_state("degraded", true, true, "stdio"));
        assert_eq!(degraded.health, ExtensionHealth::Degraded);
        assert!(degraded.tags.contains(&"stale-snapshot".to_string()));
        assert_eq!(degraded.permissions, vec!["shell-exec".to_string()]);
        let config_error =
            manager.build_mcp_summary(&mcp_state("config_error", true, false, "stdio"));
        assert_eq!(config_error.install_state, ExtensionInstallState::Error);
        assert_eq!(config_error.health, ExtensionHealth::Error);
        let disabled = manager.build_mcp_summary(&mcp_state("disconnected", false, false, "stdio"));
        assert_eq!(disabled.install_state, ExtensionInstallState::Disabled);
        assert_eq!(disabled.health, ExtensionHealth::Unknown);

        let skill = SkillRecordDto {
            id: "skill-alpha".to_string(),
            name: "Alpha Skill".to_string(),
            description: Some("Helps with docs".to_string()),
            tags: vec!["docs".to_string()],
            triggers: Vec::new(),
            tools: Vec::new(),
            priority: None,
            source: "workspace".to_string(),
            path: "/workspace/.tiy/skills/alpha".to_string(),
            enabled: false,
            scope: "workspace".to_string(),
            content_preview: "preview".to_string(),
            prompt_budget_chars: 7,
        };
        let skill_summary = manager.build_skill_summary(&skill);
        assert_eq!(skill_summary.kind, ExtensionKind::Skill);
        assert_eq!(skill_summary.install_state, ExtensionInstallState::Disabled);
        assert_eq!(skill_summary.health, ExtensionHealth::Healthy);
        assert!(matches!(
            skill_summary.source,
            ExtensionSourceDto::LocalDir { .. }
        ));
        assert_eq!(skill_summary.tags, vec!["docs".to_string()]);

        let marketplace = manager.build_marketplace_source_dto(
            &MarketplaceSourceRecord {
                id: "custom-source".to_string(),
                name: "Custom Source".to_string(),
                url: "https://example.com/catalog.git".to_string(),
                kind: "git".to_string(),
                last_synced_at: Some("2026-04-25T00:00:00Z".to_string()),
                last_error: None,
            },
            Some(3),
        );
        assert_eq!(marketplace.status, "ready");
        assert_eq!(marketplace.plugin_count, 3);
        assert!(!marketplace.builtin);
        let marketplace_error = manager.build_marketplace_source_dto(
            &MarketplaceSourceRecord {
                id: "custom-source".to_string(),
                name: "Custom Source".to_string(),
                url: "https://example.com/catalog.git".to_string(),
                kind: "git".to_string(),
                last_synced_at: None,
                last_error: Some("git failed".to_string()),
            },
            None,
        );
        assert_eq!(marketplace_error.status, "error");
        assert_eq!(marketplace_error.plugin_count, 0);
    }

    #[test]
    fn plugin_manifest_helpers_parse_aliases_and_optional_sections() {
        let plugin_dir = Path::new("/plugins/demo");
        let manifest = parse_plugin_manifest(
            r#"{
              "id": "demo.plugin",
              "name": "Demo Plugin",
              "version": "1.2.3",
              "summary": "Demo summary",
              "author": { "name": "Ada" },
              "repository": "https://example.com/repo.git",
              "default_enabled": false,
              "capabilities": ["tools", "tools", "commands"],
              "permissions": ["shell", "network"],
              "hooks": { "pre_tool_use": ["pre"], "postToolUse": ["post"] },
              "tools": [
                { "name": "run", "description": "Run command", "command": "node", "args": ["tool.js"], "permission": "shell" }
              ],
              "commands": [
                { "name": "review", "description": "Review code", "prompt": "Review this" }
              ],
              "timeout_ms": 42,
              "skills_dir": "skills",
              "config_schema": { "path": "schema.json", "type": "json-schema" }
            }"#,
            plugin_dir,
        )
        .expect("manifest");

        assert_eq!(manifest.id, "demo.plugin");
        assert_eq!(manifest.description.as_deref(), Some("Demo summary"));
        assert_eq!(manifest.author.as_deref(), Some("Ada"));
        assert_eq!(
            manifest.homepage.as_deref(),
            Some("https://example.com/repo.git")
        );
        assert_eq!(manifest.default_enabled, Some(false));
        assert_eq!(
            manifest.capabilities,
            vec!["commands".to_string(), "tools".to_string()]
        );
        assert_eq!(
            manifest.permissions,
            vec!["network".to_string(), "shell".to_string()]
        );
        assert!(manifest
            .hooks
            .as_ref()
            .and_then(|hooks| hooks.pre_tool_use.as_ref())
            .is_some());
        assert_eq!(manifest.tools[0].required_permission, "read");
        assert_eq!(manifest.commands[0].description, "Review code");
        assert_eq!(manifest.timeout_ms, Some(42));
        assert_eq!(manifest.skills_dir.as_deref(), Some("skills"));
        assert_eq!(
            manifest
                .config_schema
                .as_ref()
                .map(|schema| schema.path.as_str()),
            Some("schema.json")
        );

        let invalid = parse_plugin_manifest("{not json", plugin_dir).unwrap_err();
        assert!(invalid.contains("manifest is not valid JSON"));
    }

    #[test]
    fn low_level_manifest_readers_trim_deduplicate_and_ignore_invalid_values() {
        let object = serde_json::json!({
            "name": "  Demo  ",
            "empty": "   ",
            "args": [" b ", 12, "a", "a", ""],
            "env": { "TOKEN": " secret ", "EMPTY": " ", "NUMBER": 12 },
            "enabled": true,
            "timeout_ms": 120,
            "author": { "label": "Grace" },
            "hooks": { "onRunStart": ["start", "start"], "on_run_complete": ["done"] },
            "schema": { "path": "schema.json" }
        })
        .as_object()
        .unwrap()
        .clone();

        assert_eq!(
            read_string_keys(&object, &["missing", "name"]).as_deref(),
            Some("Demo")
        );
        assert_eq!(read_string_keys(&object, &["empty"]), None);
        assert_eq!(
            read_string_array_keys(&object, &["args"]),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            read_string_map_keys(&object, &["env"])
                .unwrap()
                .get("TOKEN")
                .map(String::as_str),
            Some("secret")
        );
        assert_eq!(read_bool_keys(&object, &["enabled"]), Some(true));
        assert_eq!(read_u64_keys(&object, &["timeout_ms"]), Some(120));
        assert_eq!(
            read_author_name(object.get("author")).as_deref(),
            Some("Grace")
        );
        assert_eq!(
            read_plugin_hooks(object.get("hooks"))
                .and_then(|hooks| hooks.on_run_complete)
                .unwrap(),
            vec!["done".to_string()]
        );
        assert_eq!(
            read_config_schema(object.get("schema")).map(|schema| schema.r#type),
            Some("json-schema".to_string())
        );
    }

    #[test]
    fn compare_extension_summaries_sorts_enabled_then_installed_then_name() {
        let mut items = vec![
            ExtensionSummaryDto {
                id: "skill-zeta".to_string(),
                kind: ExtensionKind::Skill,
                name: "Zeta".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                source: ExtensionSourceDto::Builtin,
                install_state: ExtensionInstallState::Discovered,
                health: ExtensionHealth::Unknown,
                permissions: Vec::new(),
                tags: Vec::new(),
            },
            ExtensionSummaryDto {
                id: "skill-bravo".to_string(),
                kind: ExtensionKind::Skill,
                name: "bravo".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                source: ExtensionSourceDto::Builtin,
                install_state: ExtensionInstallState::Installed,
                health: ExtensionHealth::Unknown,
                permissions: Vec::new(),
                tags: Vec::new(),
            },
            ExtensionSummaryDto {
                id: "skill-alpha".to_string(),
                kind: ExtensionKind::Skill,
                name: "Alpha".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                source: ExtensionSourceDto::Builtin,
                install_state: ExtensionInstallState::Enabled,
                health: ExtensionHealth::Unknown,
                permissions: Vec::new(),
                tags: Vec::new(),
            },
        ];

        items.sort_by(compare_extension_summaries);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["skill-alpha", "skill-bravo", "skill-zeta"]
        );
    }

    #[test]
    fn parse_plugin_manifest_supports_minimal_marketplace_shape() {
        let plugin_dir = tempdir().expect("tempdir");
        fs::create_dir_all(plugin_dir.path().join("commands")).expect("create commands dir");
        fs::write(
            plugin_dir.path().join("commands").join("feature-dev.md"),
            "# feature-dev",
        )
        .expect("write command file");

        let manifest = parse_plugin_manifest(
            r#"{
              "name": "feature-dev",
              "description": "Comprehensive feature development workflow",
              "author": {
                "name": "Anthropic",
                "email": "support@anthropic.com"
              }
            }"#,
            plugin_dir.path(),
        )
        .expect("parse manifest");

        assert_eq!(
            manifest.id,
            plugin_dir
                .path()
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("plugin")
        );
        assert_eq!(manifest.name, "feature-dev");
        assert_eq!(manifest.version, "0.0.0");
        assert_eq!(manifest.author.as_deref(), Some("Anthropic"));
        assert!(manifest.commands.is_empty());
    }

    #[test]
    fn build_plugin_managed_mcp_config_supports_bundle_specs() {
        let manifest = PluginManifest {
            id: "bundle.plugin".to_string(),
            name: "Bundle Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            default_enabled: Some(true),
            capabilities: Vec::new(),
            permissions: Vec::new(),
            hooks: None,
            tools: Vec::new(),
            commands: Vec::new(),
            timeout_ms: None,
            skills_dir: None,
            config_schema: None,
        };

        let http_value = serde_json::json!({
            "type": "http",
            "url": "https://mcp.example.com/api"
        });
        let http_config = build_plugin_managed_mcp_config(
            &manifest,
            "example-server".to_string(),
            http_value.as_object().expect("http object"),
        )
        .expect("http config");
        assert_eq!(http_config.id, "plugin::bundle.plugin::example-server");
        assert_eq!(http_config.transport, "streamable-http");
        assert_eq!(
            http_config.url.as_deref(),
            Some("https://mcp.example.com/api")
        );

        let stdio_value = serde_json::json!({
            "command": "uvx",
            "args": ["context7-mcp"],
            "env": { "TOKEN": "secret" }
        });
        let stdio_config = build_plugin_managed_mcp_config(
            &manifest,
            "context7".to_string(),
            stdio_value.as_object().expect("stdio object"),
        )
        .expect("stdio config");
        assert_eq!(stdio_config.label, "Bundle Plugin / context7");
        assert_eq!(stdio_config.transport, "stdio");
        assert_eq!(stdio_config.command.as_deref(), Some("uvx"));
        assert_eq!(
            stdio_config.args.as_ref().expect("args"),
            &vec!["context7-mcp".to_string()]
        );
        assert_eq!(
            stdio_config
                .env
                .as_ref()
                .and_then(|env| env.get("TOKEN"))
                .map(String::as_str),
            Some("secret")
        );
    }

    #[test]
    fn merge_plugin_managed_mcp_config_preserves_user_enabled_state() {
        let existing = McpServerConfigInput {
            id: "plugin::bundle.plugin::context7".to_string(),
            label: "Bundle Plugin / context7".to_string(),
            transport: "stdio".to_string(),
            enabled: false,
            auto_start: false,
            command: Some("old-command".to_string()),
            args: Some(vec!["old-arg".to_string()]),
            env: Some(HashMap::from([("TOKEN".to_string(), "secret".to_string())])),
            cwd: Some("/tmp/context7".to_string()),
            url: None,
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer test".to_string(),
            )])),
            timeout_ms: Some(45_000),
        };

        let managed = McpServerConfigInput {
            id: existing.id.clone(),
            label: existing.label.clone(),
            transport: "stdio".to_string(),
            enabled: true,
            auto_start: true,
            command: Some("uvx".to_string()),
            args: Some(vec!["context7-mcp".to_string()]),
            env: None,
            cwd: None,
            url: None,
            headers: None,
            timeout_ms: Some(15_000),
        };

        let merged = merge_plugin_managed_mcp_config(Some(&existing), managed);

        assert!(!merged.enabled);
        assert!(!merged.auto_start);
        assert_eq!(merged.command.as_deref(), Some("uvx"));
        assert_eq!(
            merged.args.as_ref().expect("args"),
            &vec!["context7-mcp".to_string()]
        );
        assert_eq!(
            merged
                .env
                .as_ref()
                .and_then(|env| env.get("TOKEN"))
                .map(String::as_str),
            Some("secret")
        );
        assert_eq!(merged.cwd.as_deref(), Some("/tmp/context7"));
        assert_eq!(
            merged
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization"))
                .map(String::as_str),
            Some("Bearer test")
        );
        assert_eq!(merged.timeout_ms, Some(45_000));
    }

    #[test]
    fn build_plugin_managed_mcp_label_deduplicates_equivalent_names() {
        assert_eq!(
            build_plugin_managed_mcp_label("context7", "context7"),
            "context7"
        );
        assert_eq!(
            build_plugin_managed_mcp_label("Context7", "context-7"),
            "Context7"
        );
        assert_eq!(
            build_plugin_managed_mcp_label("Anthropic Tools", "filesystem"),
            "Anthropic Tools / filesystem"
        );
    }

    #[test]
    fn build_plugin_managed_mcp_config_deduplicates_matching_plugin_and_server_names() {
        let manifest = PluginManifest {
            id: "context7".to_string(),
            name: "context7".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            default_enabled: Some(true),
            capabilities: Vec::new(),
            permissions: Vec::new(),
            hooks: None,
            tools: Vec::new(),
            commands: Vec::new(),
            timeout_ms: None,
            skills_dir: None,
            config_schema: None,
        };

        let value = serde_json::json!({
            "command": "uvx",
            "args": ["context7-mcp"]
        });
        let config = build_plugin_managed_mcp_config(
            &manifest,
            "context7".to_string(),
            value.as_object().expect("stdio object"),
        )
        .expect("stdio config");

        assert_eq!(config.label, "context7");
    }

    #[test]
    fn parse_plugin_command_markdown_supports_command_files() {
        let command = parse_plugin_command_markdown(
            r#"---
description: Code review a pull request
disable-model-invocation: false
---

Provide a code review for the given pull request."#,
            "code-review",
        )
        .expect("command");

        assert_eq!(command.name, "code-review");
        assert_eq!(command.description, "Code review a pull request");
        assert_eq!(
            command.prompt_template.as_deref(),
            Some("Provide a code review for the given pull request.")
        );
    }

    #[tokio::test]
    async fn load_plugin_from_dir_supports_claude_plugin_manifest_layout() {
        let plugin_dir = tempdir().expect("tempdir");
        let manifest_dir = plugin_dir.path().join(".claude-plugin");
        fs::create_dir_all(&manifest_dir).expect("create manifest dir");
        fs::write(
            manifest_dir.join("plugin.json"),
            r#"{
              "id": "market.plugin",
              "name": "Marketplace Plugin",
              "version": "1.0.0",
              "description": "plugin from marketplace",
              "tools": [],
              "commands": []
            }"#,
        )
        .expect("write manifest");

        let runtime = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        )
        .load_plugin_from_dir(plugin_dir.path(), false)
        .expect("load plugin");

        assert_eq!(runtime.manifest.id, "market.plugin");
        assert_eq!(runtime.manifest.name, "Marketplace Plugin");
        assert_eq!(
            runtime.path,
            dunce::canonicalize(plugin_dir.path()).expect("canonical plugin path")
        );
    }
}
