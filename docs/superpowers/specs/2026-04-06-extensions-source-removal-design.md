# Extensions Source Removal Design

Date: 2026-04-06
Owner: Codex
Status: Approved by user

## Background

`Extensions Center` currently treats marketplace sources as catalog inputs, while installed plugins are persisted separately through `extensions.plugins.installed`. Installed plugin records only store `{ id, path, enabled }`, and marketplace-installed plugins point directly at marketplace cache paths under `catalog/marketplaces/<sourceId>/...`.

The current `marketplace_remove_source` implementation only removes the source record and cached repository directory. It does not reconcile installed plugins that still point at the deleted cache path. That can leave broken installed-plugin records and can cascade into plugin runtime loading failures.

This design introduces a safe source deletion flow that matches product intent:

- Builtin sources cannot be deleted.
- Non-builtin sources show a lightweight delete affordance.
- Source deletion is blocked only when the source still has enabled plugins.
- If deletion is allowed, installed but disabled plugins from that source are removed together with the source.

## Goals

- Prevent deleting a source that still has enabled plugins.
- Avoid leaving installed plugins with broken paths after source deletion.
- Keep the delete entry lightweight in the UI.
- Reuse the existing Extensions Center structure without adding a new page.
- Make backend validation the source of truth for deletion safety.

## Non-Goals

- No bulk disable flow inside the delete dialog.
- No migration of marketplace-installed plugins into a new local/imported source type.
- No changes to builtin source lifecycle beyond hiding delete affordances.

## Current Dependency Logic

### Source to marketplace items

- Marketplace source definitions are stored in `marketplaces.json`.
- Each source is synced into a cache repo under `catalog/marketplaces/<sourceId>`.
- Marketplace items are derived by scanning the cached repo for plugin manifests.

### Marketplace items to installed plugins

- Installing a marketplace item delegates to plugin installation from the cached plugin directory.
- Installed plugin records persist plugin `id`, `path`, and `enabled`.
- For marketplace-installed plugins, `path` points into the source cache directory.

### Installed plugins to derived runtime data

- Enabled plugins contribute:
  - plugin commands
  - bundled skills
  - plugin-managed MCP servers derived from `.mcp.json`
- Disabling or uninstalling a plugin removes plugin-managed MCP configs.
- Bundled skills disappear naturally when the plugin is no longer enabled or installed because skill roots are collected from enabled plugin runtimes.

### Current removal gap

- Deleting a source removes the cache directory that installed marketplace plugins depend on.
- Installed plugin records continue to reference deleted paths.
- Later runtime loading can fail while trying to resolve those plugins.

## Proposed Product Behavior

### Delete rule

- Builtin source: deletion not supported.
- Non-builtin source with one or more enabled plugins: deletion is blocked.
- Non-builtin source with no enabled plugins: deletion is allowed.

### Delete consequences

When deletion is allowed:

- The source is removed from marketplace source configuration.
- All installed but disabled plugins belonging to that source are uninstalled as part of the same operation.
- Plugin-managed MCP configs for those removed plugins are cleaned up.
- Bundled skills disappear automatically because their parent plugins are removed.
- The source cache directory is deleted.

## Data Contract

Add a dedicated preview result for source deletion.

### `MarketplaceRemoveSourcePlan`

- `source`: source metadata
- `canRemove`: boolean
- `blockingPlugins`: enabled plugins from this source
- `removableInstalledPlugins`: installed but disabled plugins from this source
- `summary`: human-readable delete summary for UI display

### `MarketplaceSourcePluginRef`

- `id`
- `name`
- `version`
- `enabled`
- `path`

The plan endpoint is used for dialog rendering. The remove endpoint must still rerun the same validation internally to protect against stale UI state.

## Backend Design

### New flow

Add a new preview-style method:

- `marketplace_get_remove_source_plan(id)`

Keep the execution method:

- `marketplace_remove_source(id)`

### Execution algorithm

1. Load the target source and reject if it is builtin.
2. Resolve all marketplace items belonging to that source.
3. Match those items against installed plugin records by path.
4. Partition matched plugins into:
   - blocking: enabled plugins
   - removable: disabled installed plugins
5. If blocking is not empty, return a structured business error and do not mutate state.
6. For each removable plugin:
   - uninstall plugin record
   - remove plugin-managed MCP configs and runtime state
7. Remove the source from `marketplaces.json`.
8. Delete the cached source repo directory.
9. Write audit data containing:
   - source id
   - removed plugin ids
   - blocking plugin ids when relevant
   - final result

### Failure semantics

Deletion should be atomic from the user’s perspective. If plugin cleanup fails, the source should not be removed. Do not allow a partial result where the source is deleted but plugin records still point to missing cache paths.

## Frontend Design

### Delete entry

Inside the `Marketplace Sources` section:

- Builtin sources do not expose a delete affordance.
- Non-builtin source items show an `x` in the top-right corner on hover.
- The same `x` should also be available on keyboard focus.
- If a source is refreshing, being previewed, or being deleted, the `x` is hidden or disabled.

Clicking the `x` never deletes immediately. It always opens the preview flow first.

### Dialog states

#### Blocked state

Shown when `blockingPlugins.length > 0`.

- Title: `Can't remove source`
- Body: explain that the source still has enabled plugins and must be disabled first
- List: blocking plugin names and versions, with enabled status shown
- Actions: `Close`

#### Removable state

Shown when `canRemove = true`.

- Title: `Remove source`
- Body:
  - removing the source will remove it from Extensions Center
  - installed but disabled plugins from this source will also be removed
- Summary block:
  - source name
  - removable plugin count
  - removable plugin list when count > 0
- Actions:
  - `Cancel`
  - `Remove source`

### Post-delete refresh behavior

After successful deletion:

- refresh marketplace sources
- refresh marketplace items
- refresh extensions list
- refresh MCP list
- refresh skill list

If the current plugin source filter points to the deleted source, reset it to `All`.

## Error Handling

- Preview requests that find blocking plugins should return structured data, not a generic failure toast.
- The delete endpoint must revalidate before mutating state and return the same business error shape if blockers appeared after preview.
- Operational failures such as source config write failure or cache deletion failure should surface as explicit deletion errors.
- Do not claim success for partial cleanup.

## Testing Strategy

### Backend

Add integration coverage for:

- builtin source cannot be deleted
- source with enabled plugin returns blocked plan and cannot be deleted
- source with only disabled installed plugins can be deleted
- deleting an allowed source removes installed plugin records for matching disabled plugins
- deleting an allowed source removes plugin-managed MCP configs for those plugins
- deleting an allowed source removes the source config and cache directory

### Frontend

Manual verification should cover:

- builtin source shows no delete `x`
- non-builtin source shows delete `x` on hover/focus
- blocked dialog shows enabled plugin list and no destructive action
- removable dialog shows cleanup summary and confirm action
- after deletion, source list and plugin list refresh correctly
- source filter resets if the deleted source was selected

## Acceptance Criteria

- Builtin marketplace sources never expose delete UI and cannot be deleted through backend commands.
- Hovering a non-builtin source reveals a top-right `x` delete entry.
- Clicking the `x` performs a preview/check before any mutation.
- Source deletion is blocked only when that source still has enabled plugins.
- When deletion is allowed, installed but disabled plugins from that source are removed as part of the delete operation.
- No installed plugin record remains pointing at a deleted marketplace cache path.
- Plugin-managed MCP configs for removed plugins are cleaned up.
- Extensions Center refreshes to a consistent state after deletion.

## Open Implementation Notes

- The current backend matches marketplace items to installed plugins by `path`; the delete plan should reuse that rule for consistency.
- The preview contract should be explicit enough for the frontend to render both blocked and removable dialogs without extra inference.
- The delete dialog should keep copy concise because the entry affordance is intentionally lightweight.
