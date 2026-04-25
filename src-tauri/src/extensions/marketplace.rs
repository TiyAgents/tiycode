use super::*;

// --- Marketplace types ---

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct MarketplaceSourceStore {
    #[serde(default)]
    pub(super) sources: Vec<MarketplaceSourceRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct MarketplaceSourceRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) url: String,
    pub(super) kind: String,
    pub(super) last_synced_at: Option<String>,
    pub(super) last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct MarketplaceSourceManifest {
    pub(super) name: Option<String>,
    pub(super) description: Option<String>,
}

// --- Marketplace impl methods ---

impl ExtensionsManager {
    pub async fn marketplace_list_sources(&self) -> Result<Vec<MarketplaceSourceDto>, AppError> {
        let store = self.load_marketplace_sources()?;
        Ok(store
            .sources
            .into_iter()
            .map(|source| self.build_marketplace_source_dto(&source, None))
            .collect())
    }

    pub async fn marketplace_add_source(
        &self,
        input: MarketplaceSourceInputDto,
    ) -> Result<MarketplaceSourceDto, AppError> {
        let mut store = self.load_marketplace_sources()?;
        let id = marketplace_source_id(&input.url);
        store.sources.retain(|source| source.id != id);
        store.sources.push(MarketplaceSourceRecord {
            id: id.clone(),
            name: input.name.clone(),
            url: input.url.clone(),
            kind: DEFAULT_MARKETPLACE_SOURCE_KIND.to_string(),
            last_synced_at: None,
            last_error: None,
        });
        self.save_marketplace_sources(&store)?;
        self.marketplace_refresh_source(&id).await
    }

    pub async fn marketplace_get_remove_source_plan(
        &self,
        id: &str,
    ) -> Result<MarketplaceRemoveSourcePlanDto, AppError> {
        self.build_marketplace_remove_source_plan(id).await
    }

    pub async fn marketplace_remove_source(&self, id: &str) -> Result<(), AppError> {
        let plan = self.build_marketplace_remove_source_plan(id).await?;
        if !plan.can_remove {
            return Err(AppError::validation(
                ErrorSource::Settings,
                plan.summary.clone(),
            ));
        }

        for plugin in &plan.removable_installed_plugins {
            self.uninstall_plugin(&plugin.id).await?;
        }

        let mut store = self.load_marketplace_sources()?;
        let before = store.sources.len();
        store.sources.retain(|source| source.id != id);
        if before == store.sources.len() {
            return Err(AppError::not_found(
                ErrorSource::Settings,
                format!("marketplace source '{id}'"),
            ));
        }
        self.save_marketplace_sources(&store)?;
        let cache_dir = marketplace_cache_root().join(id);
        if cache_dir.exists() {
            if let Err(error) = fs::remove_dir_all(&cache_dir) {
                tracing::warn!(
                    source_id = %id,
                    path = %cache_dir.display(),
                    error = %error,
                    "failed to remove marketplace source cache"
                );
            }
        }
        self.write_extension_audit(
            "marketplace_source_removed",
            "marketplace_source",
            id,
            serde_json::json!({
                "removedPluginIds": plan
                    .removable_installed_plugins
                    .iter()
                    .map(|plugin| plugin.id.clone())
                    .collect::<Vec<_>>(),
            }),
        )
        .await?;
        Ok(())
    }

    pub async fn marketplace_refresh_source(
        &self,
        id: &str,
    ) -> Result<MarketplaceSourceDto, AppError> {
        let mut store = self.load_marketplace_sources()?;
        let source_index = store
            .sources
            .iter()
            .position(|source| source.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("marketplace source '{id}'"))
            })?;
        let source_snapshot = store.sources[source_index].clone();

        match self.sync_marketplace_source_repo(&source_snapshot).await {
            Ok(_) => {
                store.sources[source_index].last_synced_at = Some(Utc::now().to_rfc3339());
                store.sources[source_index].last_error = None;
            }
            Err(error) => {
                store.sources[source_index].last_error = Some(error.user_message.clone());
            }
        }

        self.save_marketplace_sources(&store)?;
        let source = store.sources[source_index].clone();
        let items = self.marketplace_items_for_source(&source).await?;
        Ok(self.build_marketplace_source_dto(&source, Some(items.len())))
    }

    pub async fn marketplace_list_items(&self) -> Result<Vec<MarketplaceItemDto>, AppError> {
        let mut store = self.load_marketplace_sources()?;
        let mut did_update_sources = false;
        for index in 0..store.sources.len() {
            if !is_builtin_marketplace_source_id(&store.sources[index].id) {
                continue;
            }
            let cache_dir = marketplace_cache_root().join(&store.sources[index].id);
            if cache_dir.exists() {
                continue;
            }
            let source_snapshot = store.sources[index].clone();
            match self.sync_marketplace_source_repo(&source_snapshot).await {
                Ok(_) => {
                    store.sources[index].last_synced_at = Some(Utc::now().to_rfc3339());
                    store.sources[index].last_error = None;
                }
                Err(error) => {
                    store.sources[index].last_error = Some(error.user_message.clone());
                }
            }
            did_update_sources = true;
        }
        if did_update_sources {
            self.save_marketplace_sources(&store)?;
        }
        let installed = self.load_installed_plugin_records().await?;
        let installed_by_path = installed
            .iter()
            .map(|record| (record.path.clone(), record.enabled))
            .collect::<HashMap<_, _>>();
        let mut items = Vec::new();

        for source in &store.sources {
            match self.marketplace_items_for_source(source).await {
                Ok(source_items) => {
                    items.extend(source_items.into_iter().map(|mut item| {
                        if let Some(enabled) = installed_by_path.get(&item.path) {
                            item.installed = true;
                            item.enabled = *enabled;
                        }
                        item
                    }));
                }
                Err(error) => {
                    tracing::warn!(
                        source_id = %source.id,
                        source_name = %source.name,
                        error = %error.user_message,
                        "failed to load marketplace source items"
                    );
                }
            }
        }

        items.sort_by(compare_marketplace_items);
        Ok(items)
    }

    pub async fn marketplace_install_item(&self, id: &str) -> Result<PluginDetailDto, AppError> {
        let item = self
            .marketplace_list_items()
            .await?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("marketplace item '{id}'"))
            })?;
        self.install_plugin_from_dir(&item.path).await
    }

    pub(super) fn load_marketplace_sources(&self) -> Result<MarketplaceSourceStore, AppError> {
        let mut store = self
            .read_json_file_with_diagnostics::<MarketplaceSourceStore>(
                &global_marketplace_sources_path(),
                "marketplaces",
                ConfigScope::Global,
            )?
            .value;
        let mut by_id = store
            .sources
            .into_iter()
            .map(|source| (source.id.clone(), source))
            .collect::<HashMap<_, _>>();
        for source in builtin_marketplace_sources() {
            by_id.entry(source.id.clone()).or_insert(source);
        }
        let mut sources = by_id.into_values().collect::<Vec<_>>();
        sources.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        store.sources = sources;
        Ok(store)
    }

    pub(super) fn save_marketplace_sources(
        &self,
        store: &MarketplaceSourceStore,
    ) -> Result<(), AppError> {
        let persisted = MarketplaceSourceStore {
            sources: store
                .sources
                .iter()
                .filter(|source| !is_builtin_marketplace_source_id(&source.id))
                .cloned()
                .collect(),
        };
        self.write_json_file(&global_marketplace_sources_path(), &persisted)
    }

    pub(super) async fn sync_marketplace_source_repo(
        &self,
        source: &MarketplaceSourceRecord,
    ) -> Result<(), AppError> {
        let cache_dir = marketplace_cache_root().join(&source.id);
        if let Some(parent) = cache_dir.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = Command::new("git");
        configure_background_tokio_command(&mut command);
        if cache_dir.exists() {
            command
                .arg("-C")
                .arg(&cache_dir)
                .arg("pull")
                .arg("--ff-only");
        } else {
            command
                .arg("clone")
                .arg("--depth")
                .arg("1")
                .arg(&source.url)
                .arg(&cache_dir);
        }
        let output = command.output().await.map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.marketplace.git_failed",
                format!(
                    "Failed to sync marketplace source '{}': {error}",
                    source.name
                ),
            )
        })?;
        if !output.status.success() {
            return Err(AppError::recoverable(
                ErrorSource::Settings,
                "extensions.marketplace.git_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
    }

    pub(super) async fn marketplace_items_for_source(
        &self,
        source: &MarketplaceSourceRecord,
    ) -> Result<Vec<MarketplaceItemDto>, AppError> {
        let cache_dir = marketplace_cache_root().join(&source.id);
        if !cache_dir.exists() {
            return Ok(Vec::new());
        }

        let source_manifest = self.read_json_file::<MarketplaceSourceManifest>(
            &cache_dir.join(".claude-plugin/marketplace.json"),
        )?;
        let source_name = source_manifest.name.unwrap_or_else(|| source.name.clone());
        let installed = self
            .load_installed_plugin_records()
            .await?
            .into_iter()
            .map(|record| (record.path, record.enabled))
            .collect::<HashMap<_, _>>();

        let mut items = Vec::new();
        for root_name in ["plugins", "external_plugins"] {
            let root = cache_dir.join(root_name);
            if !root.is_dir() {
                continue;
            }
            for plugin_dir in read_child_dirs(&root)? {
                let plugin_manifest_path = plugin_dir.join(".claude-plugin/plugin.json");
                if !plugin_manifest_path.is_file() {
                    continue;
                }
                let raw = fs::read_to_string(&plugin_manifest_path)?;
                let manifest = parse_plugin_manifest(&raw, &plugin_dir).map_err(|error| {
                    AppError::recoverable(
                        ErrorSource::Settings,
                        "extensions.marketplace.invalid_plugin_manifest",
                        format!(
                            "Invalid marketplace plugin manifest '{}': {error}",
                            plugin_manifest_path.display()
                        ),
                    )
                })?;
                let plugin_path = plugin_dir.to_string_lossy().to_string();
                let plugin_id = format!(
                    "{}:{}",
                    source.id,
                    plugin_dir
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or("plugin")
                );
                let mut tags = Vec::new();
                let skill_names =
                    self.collect_skill_bundle_names(&plugin_dir, manifest.skills_dir.as_deref());
                if !skill_names.is_empty() {
                    tags.push("skill-pack".to_string());
                }
                let command_names = self.collect_command_bundle_names(&plugin_dir, &manifest);
                if !command_names.is_empty() || plugin_dir.join("commands").is_dir() {
                    tags.push("command-provider".to_string());
                }
                let mcp_servers = self.collect_mcp_bundle_names(&plugin_dir);
                if !mcp_servers.is_empty() {
                    tags.push("mcp-bundle".to_string());
                }
                let (installed_flag, enabled_flag) = installed
                    .get(&plugin_path)
                    .map(|enabled| (true, *enabled))
                    .unwrap_or((false, false));
                items.push(MarketplaceItemDto {
                    id: plugin_id,
                    source_id: source.id.clone(),
                    source_name: source_name.clone(),
                    kind: "plugin".to_string(),
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    summary: manifest
                        .description
                        .clone()
                        .unwrap_or_else(|| "Marketplace plugin".to_string()),
                    description: manifest
                        .description
                        .clone()
                        .unwrap_or_else(|| "Marketplace plugin".to_string()),
                    publisher: manifest
                        .author
                        .clone()
                        .unwrap_or_else(|| source_name.clone()),
                    tags,
                    hooks: self.build_plugin_hook_groups(&manifest),
                    command_names,
                    mcp_servers,
                    skill_names,
                    path: plugin_path,
                    installable: true,
                    installed: installed_flag,
                    enabled: enabled_flag,
                });
            }
        }

        Ok(items)
    }

    pub(super) fn build_marketplace_source_dto(
        &self,
        source: &MarketplaceSourceRecord,
        plugin_count: Option<usize>,
    ) -> MarketplaceSourceDto {
        let plugin_count = plugin_count.unwrap_or_else(|| {
            if source.last_error.is_some() {
                0
            } else {
                marketplace_cache_plugin_count(source)
            }
        });

        MarketplaceSourceDto {
            id: source.id.clone(),
            name: source.name.clone(),
            url: source.url.clone(),
            builtin: is_builtin_marketplace_source_id(&source.id),
            kind: source.kind.clone(),
            status: if source.last_error.is_some() {
                "error".to_string()
            } else if source.last_synced_at.is_some() {
                "ready".to_string()
            } else {
                "idle".to_string()
            },
            last_synced_at: source.last_synced_at.clone(),
            last_error: source.last_error.clone(),
            plugin_count,
        }
    }

    pub(super) async fn build_marketplace_remove_source_plan(
        &self,
        id: &str,
    ) -> Result<MarketplaceRemoveSourcePlanDto, AppError> {
        if is_builtin_marketplace_source_id(id) {
            return Err(AppError::validation(
                ErrorSource::Settings,
                "Builtin marketplace sources cannot be removed",
            ));
        }

        let store = self.load_marketplace_sources()?;
        let source = store
            .sources
            .into_iter()
            .find(|source| source.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("marketplace source '{id}'"))
            })?;
        let source_root = marketplace_cache_root().join(&source.id);

        let mut installed_plugins = self
            .load_installed_plugin_records()
            .await?
            .into_iter()
            .filter(|record| Path::new(&record.path).starts_with(&source_root))
            .map(|record| {
                let runtime = self
                    .load_plugin_from_dir(Path::new(&record.path), false)
                    .ok();
                MarketplaceSourcePluginRefDto {
                    id: record.id,
                    name: runtime
                        .as_ref()
                        .map(|plugin| plugin.manifest.name.clone())
                        .unwrap_or_else(|| "Unknown plugin".to_string()),
                    version: runtime
                        .as_ref()
                        .map(|plugin| plugin.manifest.version.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    enabled: record.enabled,
                    path: record.path,
                }
            })
            .collect::<Vec<_>>();

        installed_plugins.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });

        let (blocking_plugins, removable_installed_plugins): (Vec<_>, Vec<_>) = installed_plugins
            .into_iter()
            .partition(|plugin| plugin.enabled);
        let can_remove = blocking_plugins.is_empty();
        let summary = if can_remove {
            let removable_count = removable_installed_plugins.len();
            if removable_count == 0 {
                format!("Remove '{}' from Extensions Center.", source.name)
            } else {
                format!(
                    "Remove '{}' and {} installed plugin{} from this source.",
                    source.name,
                    removable_count,
                    if removable_count == 1 { "" } else { "s" }
                )
            }
        } else {
            format!(
                "Disable {} enabled plugin{} before removing '{}'.",
                blocking_plugins.len(),
                if blocking_plugins.len() == 1 { "" } else { "s" },
                source.name
            )
        };

        Ok(MarketplaceRemoveSourcePlanDto {
            source: self.build_marketplace_source_dto(&source, None),
            can_remove,
            blocking_plugins,
            removable_installed_plugins,
            summary,
        })
    }
}

// --- Marketplace free functions ---

pub(super) fn global_marketplace_sources_path() -> PathBuf {
    tiy_home().join("marketplaces.json")
}

pub(super) fn global_plugins_config_path() -> PathBuf {
    tiy_home().join(EXTENSIONS_PLUGINS_FILE_NAME)
}

pub(super) fn marketplace_cache_root() -> PathBuf {
    tiy_home().join("catalog/marketplaces")
}

pub(super) fn builtin_marketplace_sources() -> Vec<MarketplaceSourceRecord> {
    vec![MarketplaceSourceRecord {
        id: marketplace_source_id(BUILTIN_MARKETPLACE_ANTHROPIC_URL),
        name: BUILTIN_MARKETPLACE_ANTHROPIC_NAME.to_string(),
        url: BUILTIN_MARKETPLACE_ANTHROPIC_URL.to_string(),
        kind: DEFAULT_MARKETPLACE_SOURCE_KIND.to_string(),
        last_synced_at: None,
        last_error: None,
    }]
}

pub(super) fn is_builtin_marketplace_source_id(id: &str) -> bool {
    builtin_marketplace_sources()
        .iter()
        .any(|source| source.id == id)
}

pub(super) fn marketplace_source_id(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

pub(super) fn marketplace_cache_plugin_count(source: &MarketplaceSourceRecord) -> usize {
    let cache_dir = marketplace_cache_root().join(&source.id);
    ["plugins", "external_plugins"]
        .into_iter()
        .map(|root_name| cache_dir.join(root_name))
        .filter(|root| root.is_dir())
        .map(|root| {
            read_child_dirs(&root)
                .unwrap_or_default()
                .into_iter()
                .filter(|plugin_dir| plugin_dir.join(".claude-plugin/plugin.json").is_file())
                .count()
        })
        .sum()
}

pub(super) fn read_child_dirs(path: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    Ok(dirs)
}

pub(super) fn compare_marketplace_items(
    left: &MarketplaceItemDto,
    right: &MarketplaceItemDto,
) -> Ordering {
    right
        .enabled
        .cmp(&left.enabled)
        .then_with(|| right.installed.cmp(&left.installed))
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::mcp::workspace_mcp_path;
    use tempfile::tempdir;

    #[test]
    fn config_path_and_marketplace_helpers_are_stable() {
        let workspace = tempdir().expect("workspace");
        assert!(workspace_mcp_path(Some(workspace.path().to_str().unwrap()))
            .unwrap()
            .ends_with(".tiy/mcp.local.json"));
        assert_eq!(
            workspace_mcp_path(None).unwrap_err().error_code,
            "settings.validation"
        );

        let diagnostic_id =
            config_diagnostic_id(Path::new("/tmp/config.json"), "mcp", ConfigScope::Workspace);
        assert!(diagnostic_id.starts_with("workspace:mcp:"));

        let builtins = builtin_marketplace_sources();
        assert_eq!(builtins.len(), 1);
        assert!(is_builtin_marketplace_source_id(&builtins[0].id));
        let id = marketplace_source_id("https://example.com/catalog.git");
        assert!(!id.is_empty());
        assert!(id.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn compare_marketplace_items_sorts_enabled_then_installed_then_name() {
        let mut items = vec![
            MarketplaceItemDto {
                id: "market-zeta".to_string(),
                source_id: "source".to_string(),
                source_name: "Source".to_string(),
                kind: "plugin".to_string(),
                name: "Zeta".to_string(),
                version: "1.0.0".to_string(),
                summary: "summary".to_string(),
                description: "description".to_string(),
                publisher: "publisher".to_string(),
                tags: Vec::new(),
                hooks: Vec::new(),
                command_names: Vec::new(),
                mcp_servers: Vec::new(),
                skill_names: Vec::new(),
                path: "/tmp/zeta".to_string(),
                installable: true,
                installed: false,
                enabled: false,
            },
            MarketplaceItemDto {
                id: "market-bravo".to_string(),
                source_id: "source".to_string(),
                source_name: "Source".to_string(),
                kind: "plugin".to_string(),
                name: "bravo".to_string(),
                version: "1.0.0".to_string(),
                summary: "summary".to_string(),
                description: "description".to_string(),
                publisher: "publisher".to_string(),
                tags: Vec::new(),
                hooks: Vec::new(),
                command_names: Vec::new(),
                mcp_servers: Vec::new(),
                skill_names: Vec::new(),
                path: "/tmp/bravo".to_string(),
                installable: true,
                installed: true,
                enabled: false,
            },
            MarketplaceItemDto {
                id: "market-alpha".to_string(),
                source_id: "source".to_string(),
                source_name: "Source".to_string(),
                kind: "plugin".to_string(),
                name: "Alpha".to_string(),
                version: "1.0.0".to_string(),
                summary: "summary".to_string(),
                description: "description".to_string(),
                publisher: "publisher".to_string(),
                tags: Vec::new(),
                hooks: Vec::new(),
                command_names: Vec::new(),
                mcp_servers: Vec::new(),
                skill_names: Vec::new(),
                path: "/tmp/alpha".to_string(),
                installable: true,
                installed: true,
                enabled: true,
            },
        ];

        items.sort_by(compare_marketplace_items);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["market-alpha", "market-bravo", "market-zeta"]
        );
    }
}
