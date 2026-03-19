# Provider Model Test Connection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a working `Test Connection` action for provider model rows, using `tiy-core` for language-model tests and returning `not supported yet` for embedding models.

**Architecture:** Add a Tauri settings command that loads persisted provider and model settings, detects embedding capability from stored model capabilities, and runs a minimal `tiy-core` text request with `Ping from Tiy Agent.` and `max_tokens = 8` for language models. Then wire the frontend model-row button to that command and show per-row loading and result feedback.

**Tech Stack:** Tauri 2, Rust, `tiy-core`, React 19, TypeScript, Vite

---

### Task 1: Add the API contract for model test results

**Files:**
- Modify: `src/shared/types/api.ts`
- Modify: `src/services/bridge/settings-commands.ts`

**Step 1: Add the DTO type**

Add a `ProviderModelConnectionTestResultDto` type with:

```ts
export interface ProviderModelConnectionTestResultDto {
  success: boolean;
  unsupported: boolean;
  message: string;
  detail?: string | null;
}
```

**Step 2: Add the bridge function**

Expose a bridge helper:

```ts
export async function providerModelTestConnection(
  providerId: string,
  modelId: string,
): Promise<ProviderModelConnectionTestResultDto> {
  requireTauri("provider_model_test_connection");
  return invoke<ProviderModelConnectionTestResultDto>("provider_model_test_connection", {
    providerId,
    modelId,
  });
}
```

**Step 3: Run TypeScript validation**

Run: `npm run typecheck`

Expected: TypeScript still compiles.

**Step 4: Commit**

```bash
git add src/shared/types/api.ts src/services/bridge/settings-commands.ts
git commit -m "feat(settings): add provider model test bridge"
```

### Task 2: Add the Rust command and DTO

**Files:**
- Modify: `src-tauri/src/model/provider.rs`
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Add the Rust response DTO**

Add a serialized DTO mirroring the frontend type:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConnectionTestResultDto {
    pub success: bool,
    pub unsupported: bool,
    pub message: String,
    pub detail: Option<String>,
}
```

**Step 2: Add the Tauri command**

Expose:

```rust
#[tauri::command]
pub async fn provider_model_test_connection(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<ProviderModelConnectionTestResultDto, AppError>
```

and forward to `settings_manager`.

**Step 3: Register the command**

Add `commands::settings::provider_model_test_connection` to the Tauri invoke handler list.

**Step 4: Run Rust compile check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml m1_3_settings --no-run`

Expected: Rust compiles with the new command surface.

**Step 5: Commit**

```bash
git add src-tauri/src/model/provider.rs src-tauri/src/commands/settings.rs src-tauri/src/lib.rs
git commit -m "feat(settings): add provider model test command"
```

### Task 3: Implement provider/model lookup and test execution in SettingsManager

**Files:**
- Modify: `src-tauri/src/core/settings_manager.rs`
- Modify: `src-tauri/src/persistence/repo/provider_repo.rs`
- Test: `src-tauri/tests/m1_3_settings.rs`

**Step 1: Add repo support to find a model by id**

Add a small helper in `provider_repo.rs`:

```rust
pub async fn find_model_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ProviderModelRecord>, AppError>
```

**Step 2: Add capability parsing helpers**

Add small helpers in `settings_manager.rs` for:

- parsing stored capability overrides JSON
- checking whether `embedding` is `true`
- parsing numeric strings with safe fallbacks

**Step 3: Add the core test method**

Implement:

```rust
pub async fn test_provider_model_connection(
    &self,
    provider_id: &str,
    model_id: &str,
) -> Result<ProviderModelConnectionTestResultDto, AppError>
```

Behavior:

- load provider and model from persistence
- ensure the model belongs to the provider
- return `unsupported = true` for embedding models
- build a minimal `tiy_core::types::Model`
- build a `Context` with `Ping from Tiy Agent.`
- call `tiy_core::provider::get_provider(...).stream(...)`
- force `StreamOptions.max_tokens = Some(8)`
- return success on `Done`, failure on `Error` or timeout

**Step 4: Add a timeout**

Wrap the test execution with `tokio::time::timeout(...)` so the settings action cannot hang forever.

**Step 5: Write failing tests**

Add Rust tests for:

- embedding models return `unsupported = true`
- missing model/provider returns error
- language-model request path uses the ping text and `max_tokens = 8`

For the request-shape test, isolate construction logic into a helper that can be asserted without live network calls.

**Step 6: Run targeted tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml m1_3_settings -- --nocapture`

Expected: New and existing settings tests pass.

**Step 7: Commit**

```bash
git add src-tauri/src/core/settings_manager.rs src-tauri/src/persistence/repo/provider_repo.rs src-tauri/tests/m1_3_settings.rs
git commit -m "feat(settings): implement provider model connection test"
```

### Task 4: Expose test action through the settings controller

**Files:**
- Modify: `src/modules/settings-center/model/use-settings-controller.ts`
- Modify: `src/modules/workbench-shell/ui/dashboard-workbench.tsx`

**Step 1: Import the bridge function**

Add `providerModelTestConnection` to the bridge imports.

**Step 2: Add a controller method**

Return:

```ts
const testProviderModelConnection = async (providerId: string, modelId: string) => {
  if (!isTauri()) {
    return {
      success: false,
      unsupported: false,
      message: "Test Connection requires Tauri runtime.",
      detail: null,
    };
  }

  return providerModelTestConnection(providerId, modelId);
};
```

**Step 3: Thread the prop through the workbench**

Pass the new callback into `SettingsCenterOverlay`.

**Step 4: Run TypeScript validation**

Run: `npm run typecheck`

Expected: Prop types stay aligned.

**Step 5: Commit**

```bash
git add src/modules/settings-center/model/use-settings-controller.ts src/modules/workbench-shell/ui/dashboard-workbench.tsx
git commit -m "feat(settings): expose model connection test action"
```

### Task 5: Wire the model-row button and feedback state

**Files:**
- Modify: `src/modules/settings-center/ui/settings-center-overlay.tsx`

**Step 1: Extend props**

Add a prop:

```ts
onTestProviderModelConnection: (
  providerId: string,
  modelId: string,
) => Promise<ProviderModelConnectionTestResultDto>
```

**Step 2: Add per-row transient state**

Track:

- current testing model id
- per-model latest result

Keep feedback close to each row.

**Step 3: Connect the button**

Add the click handler on `Test connection`:

- start loading state
- call the new prop
- store result keyed by model id
- clear loading state

**Step 4: Render result feedback**

Show short inline feedback:

- success state for successful language-model tests
- informational state for `unsupported = true`
- error state for thrown or returned failures

**Step 5: Prevent duplicate clicks**

Disable the row button while that row is testing.

**Step 6: Run TypeScript validation**

Run: `npm run typecheck`

Expected: UI compiles with the new prop and state.

**Step 7: Commit**

```bash
git add src/modules/settings-center/ui/settings-center-overlay.tsx
git commit -m "feat(settings): wire provider model test connection UI"
```

### Task 6: Final verification

**Files:**
- No code changes required unless fixes are needed

**Step 1: Run Rust tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml m1_3_settings -- --nocapture`

Expected: PASS

**Step 2: Run TypeScript check**

Run: `npm run typecheck`

Expected: PASS

**Step 3: Review working tree**

Run: `git status --short`

Expected: Only intended files for this feature remain modified.

**Step 4: Summarize**

Document:

- backend command added
- `tiy-core` language-model test path added
- embedding models return unsupported
- UI action wired with row-level feedback
