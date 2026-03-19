# Provider Model Test Connection Design

## Summary

This design adds `Test Connection` support to `Settings > Provider > Model list > Item` and routes language-model testing through `tiy-core`.

The first version is intentionally narrow:

- language models can be tested from the model row
- the test request sends `Ping from Tiy Agent.`
- the request forces `max_tokens = 8`
- embedding models do not execute a real test yet
- embedding models return a clear `not supported yet` result to the UI

## Goals

- Add a working `Test Connection` action for provider models in Settings.
- Reuse persisted provider settings already managed by Tauri/Rust.
- Route language-model execution through `tiy-core` instead of a separate frontend HTTP client.
- Make the language-model test cheap, fast, and deterministic enough for a settings health check.
- Distinguish language-model and embedding-model behavior explicitly in the UI and backend.

## Non-Goals

- This spec does not implement embedding test execution.
- This spec does not add bulk provider validation or background health checks.
- This spec does not change agent runtime model routing.
- This spec does not add model-level secret storage beyond the current provider settings model.

## Current Problems

### No connected action behind the button

The `Test Connection` button exists in the Provider model row UI, but it currently has no click handler and no backend command.

### No model-type-specific test behavior

The Settings UI already understands model capabilities such as `embedding`, but there is no test flow that branches based on those capabilities.

### No runtime bridge from Settings to `tiy-core`

Provider settings can fetch model lists through Rust, but there is not yet a Rust command that constructs a runtime request and verifies that a configured provider/model can answer a simple prompt.

## Product Behavior

### Language model test

When the user clicks `Test Connection` for a non-embedding model:

- the frontend calls a new Tauri command with `providerId` and `modelId`
- Rust loads the provider settings and model settings from persistence
- Rust builds a `tiy-core` model request using the saved provider configuration
- Rust sends one short prompt: `Ping from Tiy Agent.`
- Rust forces `max_tokens = 8`
- the command returns success when the stream completes without provider error

The response body itself is not the product focus. The important result is whether a minimal request succeeds with the current provider configuration.

### Embedding model test

When the user clicks `Test Connection` for an embedding model:

- no external provider request is made
- the backend returns a structured informational result
- the UI shows a clear message such as `Embedding model test is not supported yet.`

This keeps the button behavior explicit without pretending the model was actually validated.

## Architecture

### Frontend responsibility

React remains responsible for:

- wiring the model-row button
- showing pending, success, and error feedback
- preventing duplicate clicks while a test is running

The frontend should not build provider HTTP requests directly.

### Rust/Tauri responsibility

Rust becomes responsible for:

- locating the selected provider and model from persisted settings
- deciding whether the model should be treated as an embedding model
- building the `tiy-core` request for language models
- normalizing the returned success or failure into a UI-friendly DTO

### `tiy-core` responsibility

`tiy-core` remains the execution layer for language-model requests:

- resolve the provider implementation from `providerType`
- stream a minimal completion request
- surface provider or transport errors back to Rust

## Capability Decision Rule

The backend should not trust the frontend's temporary state to decide model type.

Instead, Rust should determine whether the target model is an embedding model from persisted model capability overrides, using the same persisted `embedding` capability field already stored for provider models.

If `embedding = true`, the command returns `not supported yet`.

Otherwise, the command attempts the language-model test.

## Tauri API Design

Add a new command:

- `provider_model_test_connection(provider_id, model_id)`

Suggested response DTO:

- `success: bool`
- `message: string`
- `detail?: string`
- `unsupported: bool`

Examples:

- language model success:
  - `success = true`
  - `unsupported = false`
  - `message = "Connection test succeeded."`
- embedding model:
  - `success = false`
  - `unsupported = true`
  - `message = "Embedding model test is not supported yet."`
- provider failure:
  - `success = false`
  - `unsupported = false`
  - `message = "Connection test failed."`
  - `detail = provider error or timeout summary`

## Backend Flow

### 1. Load persisted settings

Rust loads:

- provider record by `provider_id`
- provider model record by `model_id`

If either record is missing, the command returns a recoverable not-found error.

### 2. Detect embedding models

Rust parses the model capability overrides JSON and checks whether `embedding` is set to `true`.

If yes:

- return an informational unsupported result
- skip all provider network activity

### 3. Build the `tiy-core` model

For language models, Rust builds a `tiy-core::types::Model` from persisted settings:

- `id = model.model_name`
- `name = display name if available, otherwise model id`
- `provider = tiy_core::types::Provider::from(provider.provider_type)`
- `base_url = Some(provider.base_url)`
- `context_window = parsed model context window or a safe fallback`
- `max_tokens = parsed model max output tokens or a safe fallback`
- `headers = parsed custom headers if present`

The test call still overrides request output length with `StreamOptions.max_tokens = Some(8)`.

### 4. Build the prompt

Rust constructs a minimal `Context`:

- no tools
- no extra system prompt required
- one user message containing `Ping from Tiy Agent.`

### 5. Execute through `tiy-core`

Rust resolves the provider via `tiy_core::provider::get_provider(...)` and starts a stream request.

The command treats the test as successful when the stream reaches `Done` without provider error.

The command treats the test as failed when:

- the provider cannot be resolved
- the API key or base URL is invalid
- the provider returns an error event
- the stream times out

### 6. Return normalized result

The command returns a small DTO that the frontend can render directly.

## Fallback Values

Some manually added models may not have complete metadata. To keep the first version practical, Rust should apply conservative fallbacks when building the `tiy-core` model:

- `context_window`: fallback to `8192`
- `max_tokens`: fallback to `4096`
- `input`: set to text-only for the test path
- `reasoning`: `false`
- `cost`: default value
- `compat`: none unless later required by provider-specific issues

These fallbacks are acceptable because the test only verifies that the configured provider/model can answer a minimal text request.

## Frontend Interaction Design

### Model row state

Each model row should support a local transient state:

- idle
- testing
- success
- error

Only the clicked row enters `testing`.

### Feedback placement

Feedback should stay close to the clicked model row so the user does not need to scan the entire settings page.

Acceptable first-version behavior:

- show a short inline status message in the row action area or below the row header
- keep the latest result visible until another test runs on that same row

### Unsupported embedding message

If the backend reports `unsupported = true`, the frontend should display the returned message as an informational state rather than a hard failure.

## Error Handling

### Recoverable failures

The command should surface recoverable problems clearly:

- missing provider
- missing model
- provider not registered in `tiy-core`
- invalid credentials
- invalid base URL
- timeout
- upstream provider rejection

### Message shape

The UI-facing message should stay short and readable. Raw provider details can go into `detail` when available.

## Testing Plan

### Rust tests

Add unit coverage for:

- embedding capability returns `not supported yet`
- missing provider or model returns a recoverable error
- language-model path builds a test request with prompt `Ping from Tiy Agent.`
- language-model path forces `max_tokens = 8`

Where practical, isolate request construction from actual network execution so core behavior can be validated without live provider credentials.

### Frontend verification

Verify:

- clicking the button starts and ends a loading state
- success feedback is shown for successful results
- unsupported embedding feedback is shown distinctly
- failure feedback is shown for thrown command errors or failed results

## Rollout Notes

This first version intentionally leaves embedding execution unimplemented. That is acceptable as long as the UI is explicit and does not imply a real validation happened for embedding models.

Once `tiy-core` exposes a reusable embedding execution interface, the same command can be extended to test embedding models without changing the button contract.
