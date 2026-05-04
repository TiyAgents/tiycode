# Repository Guidelines

## Project Structure & Module Organization
The desktop UI lives in `src/` with React + TypeScript. Use `src/app/` for bootstrap, routing, providers, and global styles; `src/pages/` for route-level surfaces such as onboarding and workbench entry points; `src/modules/` for domain areas such as workbench shell, onboarding, settings center, and extensions center; `src/features/` for platform-facing features like terminal and system metadata; `src/components/` for AI Elements such as messages, prompt input, code blocks, reasoning, plans, tools, and confirmations; `src/shared/` for reusable UI, types, config, and helpers; and `src/services/` for bridge and streaming integrations. Static assets belong in `public/`. In `src/modules/workbench-shell/ui/`, keep large workbench surfaces as orchestrators and place extracted presentation helpers and small logic helpers beside them, such as `runtime-thread-surface-*`, `runtime-thread-surface-state.ts`, `long-message-body.tsx`, `dashboard-sidebar.tsx`, `dashboard-overlays.tsx`, `dashboard-terminal-orchestrator.tsx`, `dashboard-workbench-logic.ts`, and `thread-rename-input.tsx`. Tauri/Rust code lives in `src-tauri/src/`: extension host/runtime code is organized under `src-tauri/src/extensions/` with `mod.rs` as the facade plus `config_io.rs`, `plugins.rs`, `mcp.rs`, `skills.rs`, `marketplace.rs`, and `runtime_tools.rs`; agent runtime orchestration stays under `src-tauri/src/core/` with focused companion modules such as `agent_session_execution.rs`, `agent_session_*`, `agent_run_compaction.rs`, and other `agent_run_*` modules. Use `src-tauri/src/ipc/` for frontend event/channel bridge code, keep bundled provider catalog snapshots in `src-tauri/bundled-catalog/`, migrations in `src-tauri/migrations/`, backend integration tests in `src-tauri/tests/`, and design references in `docs/`.

## Build, Test, and Development Commands
- `npm run dev` — start the full Tauri desktop app.
- `npm run dev:web` — run the Vite frontend only.
- `npm run build:web` — type-check and bundle web assets.
- `npm run build` — produce desktop binaries.
- `npm run typecheck` — run TypeScript validation for UI changes.
- `npm run test:unit` — run Vitest unit tests for frontend utilities and model helpers.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml` — run Rust integration tests with the committed lockfile.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — verify Rust formatting before opening a PR; use `cargo fmt --manifest-path src-tauri/Cargo.toml` to apply formatting locally.

## Test Coverage

- **Frontend** — run `npm run test:unit -- --coverage` to generate a Vitest + `@vitest/coverage-v8` report. The summary is printed to the terminal; detailed HTML reports are written to `coverage/`.
- **Backend** — run `cargo llvm-cov --manifest-path src-tauri/Cargo.toml` (requires [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov)) to generate line/branch coverage for Rust integration tests. Add `--html` to open a detailed report, or `--text` for a terminal summary.

## CI and Automated Review
Pull requests targeting `master` run `.github/workflows/ci.yml`, which validates commit messages, installs dependencies with `npm ci`, runs `npm run typecheck`, `npm run test:unit`, `npm run build:web`, checks Rust formatting with `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, and runs `cargo test --locked --manifest-path src-tauri/Cargo.toml`. Keep local verification aligned with those commands when possible. `.github/workflows/pr-review.yml` optionally runs `TiyAgents/code-review-agent-action@v3` when LLM configuration is available; review configuration changes should preserve its include/exclude globs, allowlist, confidence threshold, and bounded round/model-call limits unless there is a deliberate reason to change them.

## Coding Style & Naming Conventions
Use 2-space indentation in TypeScript/TSX. Prefer functional React components with named exports. File names should be kebab-case, for example `workbench-top-bar.tsx`; components use PascalCase; hooks use `use...`. Prefer the `@/` alias over deep relative imports. Keep code close to the feature unless it is clearly reusable. Reuse design tokens from `src/app/styles/globals.css` and align UI work with `docs/design-spec.md`. In Rust, preserve the existing `commands/`, `core/`, `model/`, and `persistence/` separation.

## Testing Guidelines
Frontend utility and model regressions can be covered with Vitest tests colocated under `src/`; UI changes should still pass `npm run typecheck` and receive manual verification. If project dependencies are missing, incomplete, or otherwise not installed in a clean/safe state for validation, install them with `npm ci` before running verification commands such as `npm run typecheck`, `npm run test:unit`, and any other relevant checks. Rust tests live in `src-tauri/tests/*.rs`; use module-oriented names such as `agent_run.rs`. Add or update backend integration tests whenever command, persistence, runtime, task-board, attachment, workspace/worktree, provider catalog, or tool-gateway behavior changes.

## Commit & Pull Request Guidelines
Follow Conventional Commits: `type(scope): short summary`, for example `feat(agent-session): enhance workspace context`. Common types include `feat`, `fix`, `refactor`, and `chore`. Keep scopes tied to the area changed. Pull requests should include a concise summary, linked issue or design doc when relevant, commands run, and screenshots or GIFs for visible UI changes. Call out migrations, capability updates, or setup steps explicitly.

## Settings Schema Version (`SETTINGS_STORAGE_SCHEMA_VERSION`)
The constant `SETTINGS_STORAGE_SCHEMA_VERSION` in `src/modules/settings-center/model/defaults.ts` now controls only the schema version of the frontend local UI storage key `tiy-agent-local-ui-settings`, which is read and written by `src/modules/settings-center/model/settings-storage.ts`. When the stored schema version is lower than this constant, the app discards that local UI payload and falls back to the code-defined defaults for the `general` and `terminal` sections.

**You must increment this value** whenever you change the shape or built-in defaults of the data stored in this local UI payload, including additions, removals, renames, or default-value changes under the `general` or `terminal` sections. Without the increment, existing users may keep stale `localStorage` data for those UI-only settings.

**Do not use this version as a catch-all settings migration switch anymore.** It no longer governs the full settings model, built-in slash command prompts, agent profiles, providers, workspaces, or other Rust/file/database-backed settings. Changes to those areas require their own migration or compatibility handling instead of bumping `SETTINGS_STORAGE_SCHEMA_VERSION`.

Because incrementing this value resets only the cached local UI settings payload, use it only when existing `general` or `terminal` local storage data must be invalidated or reshaped.

## Post-Implementation Checklist
After completing a task, always run the relevant formatting and validation commands before committing: `cargo fmt --check --manifest-path src-tauri/Cargo.toml` for Rust formatting verification, `cargo test --locked --manifest-path src-tauri/Cargo.toml` for Rust behavior changes, `npm run typecheck` for TypeScript/TSX changes, and `npm run test:unit` when frontend utility or model tests were added or affected. Use `cargo fmt --manifest-path src-tauri/Cargo.toml` to apply Rust formatting before re-running the check. Fix all warnings and errors before finalizing the commit.

## Agent-Specific Instructions
Address the user as `Buddy` in all collaborator-facing responses for this repository. Pay special attention to cross-platform differences when coding, and preserve cross-platform compatibility in implementation choices.
