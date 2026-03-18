# Provider Settings tiy-core Integration Design

## Summary

This design aligns the desktop Settings Provider experience with `tiy-core = "0.1.0-rc.26031901"` and makes Tauri/Rust the source of truth for provider definitions and provider settings.

The current desktop implementation treats providers as a frontend-managed generic HTTP configuration model with mutable API protocol selection. That no longer matches the target architecture. After this change:

- Built-in providers are generated from the `tiy-core` provider catalog and displayed in full.
- Built-in provider mappings are fixed and cannot be changed or deleted.
- Built-in providers still support editable provider settings such as enablement, API key, base URL override, custom headers, and models.
- Custom providers can be created and deleted by the user.
- Custom providers must choose one of four supported `tiy-core` provider classes:
  - `openai-compatible`
  - `anthropic`
  - `google`
  - `ollama`
- Existing provider data is not migrated. Provider state is cleared and rebuilt from the new schema.

## Goals

- Use `tiy-core` provider capabilities as the canonical source for the Settings Provider page.
- Show all currently supported built-in providers that `tiy-core` can instantiate out of the box.
- Replace the mutable `API Protocol` concept with a `Provider Type` concept that maps directly to `tiy-core`.
- Preserve provider settings for built-in and custom providers.
- Give agent profiles and model selectors a stable provider identity that does not depend on display name strings.

## Non-Goals

- This spec does not implement live model discovery from remote provider APIs.
- This spec does not define runtime chat execution changes beyond the provider/settings contract needed for the Settings page.
- This spec does not attempt backward migration of existing provider configuration data.

## Current Problems

### Frontend-only provider state

The Settings page currently manages providers primarily through frontend state and `localStorage`. The Tauri provider CRUD layer exists but is not the effective source of truth for the Settings page.

### Mutable protocol abstraction

The current `API Protocol` field models providers as generic HTTP endpoints. That conflicts with the new requirement that provider selection must come from `tiy-core` supported providers.

### Unstable model references

Agent profiles and other UI surfaces currently rely on display-oriented strings such as `provider.name/modelId`. Those references break if a provider display name changes.

## Provider Catalog

The built-in provider catalog comes from the set of `tiy-core` providers that have concrete default provider implementations available in `tiy-core` `0.1.0-rc.26031901`.

The initial built-in catalog is:

- `openai`
- `openai-compatible`
- `anthropic`
- `google`
- `ollama`
- `xai`
- `groq`
- `openrouter`
- `minimax`
- `minimax-cn`
- `kimi-coding`
- `zai`
- `deepseek`
- `zenmux`

These entries are always present in the Settings UI. They are not user-created rows.

## Custom Provider Types

Users can create custom providers. A custom provider must select one of these provider types:

- `openai-compatible`
- `anthropic`
- `google`
- `ollama`

This preserves the requirement that custom providers are still backed by `tiy-core` supported provider classes rather than arbitrary wire-protocol strings.

## Architecture

### Source of truth

Rust/Tauri becomes the source of truth for:

- available built-in provider catalog entries
- persisted provider settings
- validation for built-in versus custom provider behavior

The frontend consumes normalized DTOs and stops owning provider definitions locally.

### Separation of concerns

- `tiy-core` defines provider semantics and supported provider classes
- Tauri/Rust defines catalog exposure, persistence, validation, and DTO shaping
- React renders the catalog and issues specific update commands

This keeps provider semantics and runtime alignment close to the Rust side while allowing the frontend to stay thin.

## Data Model

### Provider entry shape

The frontend should consume a unified provider settings DTO with the following fields:

- `id`
- `kind`
  - `builtin`
  - `custom`
- `providerKey`
  - for built-in providers, a stable `tiy-core::Provider` key
  - for custom providers, a stable app-generated identifier
- `providerType`
  - a `tiy-core` provider type string used for runtime mapping
- `displayName`
- `enabled`
- `lockedMapping`
- `baseUrl`
- `apiKeyConfigured`
- `customHeaders`
- `models`

Frontend edit state may hold a transient plain `apiKey` while the user edits, but persisted DTOs should not return raw secrets by default.

### Provider model shape

Provider models remain editable settings entries associated with a provider and should continue to support:

- stable model `id`
- `modelId`
- `displayName`
- `enabled`
- optional capability overrides
- optional provider options

### Agent profile references

Agent profile model selections should stop storing provider-display-based strings. They should instead store stable references:

- `primaryProviderId`
- `primaryModelId`
- `assistantProviderId`
- `assistantModelId`
- `liteProviderId`
- `liteModelId`

Display labels are derived from the current provider/model catalog at render time.

## Tauri API Design

The current provider CRUD API is too generic for the new built-in versus custom split. The new Settings-facing API should be explicit.

### Catalog

- `provider_catalog_list()`

Returns the supported provider catalog for UI rendering and custom type selection.

### Settings read

- `provider_settings_get_all()`

Returns the normalized provider settings list shown in the UI. This command merges:

- the built-in provider catalog
- persisted provider settings

Built-in entries must always appear even when the user has not configured them yet.

### Built-in writes

- `provider_settings_upsert_builtin(provider_key, patch)`

Updates a built-in provider's editable settings only.

Forbidden actions:

- changing `providerType`
- deleting the provider
- changing the built-in provider mapping

### Custom writes

- `provider_settings_create_custom(input)`
- `provider_settings_update_custom(id, patch)`
- `provider_settings_delete_custom(id)`

Validation rules:

- custom `providerType` must be one of the four approved custom types
- custom rows may be renamed and deleted
- custom rows may keep provider settings and models

## Persistence Strategy

### Clear and rebuild

Existing provider settings are intentionally not migrated.

On first load of the new schema:

- legacy frontend provider state in `localStorage` is discarded
- legacy provider rows in the old persistence model are ignored or replaced by the new provider-settings flow
- built-in providers are reconstructed from the catalog

### Schema versioning

The frontend settings payload should include a schema version. If the stored version predates this design, provider state is reset and regenerated.

This reset applies only to provider-specific state and should not wipe unrelated settings.

## UI Design

### Provider list

The Provider list should show all built-in providers plus user-created custom providers.

Built-in entries:

- always visible
- marked as built-in
- cannot be deleted
- cannot change provider mapping

Custom entries:

- can be created through an add-provider action
- can be deleted
- can switch among the supported custom provider types

### Provider detail panel

For built-in providers:

- display `Provider Type` as read-only
- keep editable `enabled`, `API Key`, `Base URL`, `Custom Headers`, and `Models`

For custom providers:

- allow editing `displayName`
- allow changing `Provider Type` within the approved custom type set
- keep editable `enabled`, `API Key`, `Base URL`, `Custom Headers`, and `Models`

### Provider type replaces API protocol

The current `API Protocol` selector is removed.

It is replaced with:

- a read-only `Provider Type` display for built-in providers
- a constrained `Provider Type` selector for custom providers

This makes the UI describe actual `tiy-core` semantics instead of a synthetic protocol layer.

## Data Flow

### Settings load

1. Frontend opens Settings.
2. Frontend requests `provider_settings_get_all()`.
3. Rust loads the built-in provider catalog.
4. Rust loads persisted provider settings.
5. Rust merges catalog entries with saved settings and returns normalized DTOs.
6. Frontend renders the provider list and details from DTOs.

### Built-in provider update

1. User edits a built-in provider field.
2. Frontend calls `provider_settings_upsert_builtin(provider_key, patch)`.
3. Rust validates the patch against built-in restrictions.
4. Rust persists settings and returns the refreshed normalized entry or refreshed full list.
5. Frontend updates its view state from the returned DTO.

### Custom provider lifecycle

1. User creates a custom provider and chooses a supported provider type.
2. Frontend calls `provider_settings_create_custom(input)`.
3. Rust validates the type and persists the record.
4. Frontend renders the new row.
5. Later updates and deletion use the dedicated custom commands.

## Validation Rules

### Built-in providers

- cannot be deleted
- cannot change provider type
- cannot change built-in mapping key

### Custom providers

- can only use approved custom provider types
- can be deleted
- can be renamed

### Models

- model records remain provider-scoped
- model references in profiles must resolve by stable provider/model ids
- changing a provider display name must not invalidate model references

## Error Handling

- Frontend should disable invalid actions for built-in providers.
- Rust must enforce the same restrictions even if a client bypasses the UI.
- If a builtin provider key is unknown after a future `tiy-core` upgrade, catalog rebuild wins over stale persisted data.
- API key display remains masked by default.
- Invalid custom headers or invalid provider options remain client-validated before save, with Rust performing final validation as needed.

## Testing Strategy

### Rust

Add coverage for:

- built-in provider catalog generation
- provider settings merge between catalog and persisted data
- custom provider create, update, and delete flows
- rejection of built-in delete attempts
- rejection of built-in provider type remapping
- rejection of unsupported custom provider types

### Frontend

Add coverage for:

- rendering of built-in and custom providers in the same list
- read-only mapping behavior for built-in providers
- custom provider type selector behavior
- schema-version-triggered provider reset
- stable profile/model display when provider display names change

### Integration

Add at least one regression path covering:

1. configure provider settings in Settings
2. select a model in an agent profile
3. render the selected model label correctly in composer or related workbench UI

## Rollout Notes

- This design intentionally breaks backward compatibility for provider settings.
- The reset behavior is explicit and acceptable for this milestone.
- Implementation should keep unrelated settings, policy data, and workspaces intact.

## Open Questions

No open design questions remain for this scope. The user confirmed:

- show the full current built-in provider set from `tiy-core`
- do not migrate legacy provider configuration

