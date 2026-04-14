# Repository Guidelines

## Project Structure & Module Organization
The desktop UI lives in `src/` with React + TypeScript. Use `src/app/` for bootstrap, routing, providers, and global styles; `src/modules/` for domain areas such as workbench, settings, marketplace, and the newer extensions center surfaces; `src/features/` for platform-facing features like terminal and system metadata; `src/shared/` for reusable UI, types, config, and helpers; and `src/services/` for bridge and streaming integrations. Static assets belong in `public/`. Tauri/Rust code lives in `src-tauri/src/`, with extension host/runtime code now under `src-tauri/src/extensions/`; migrations stay in `src-tauri/migrations/`, backend integration tests in `src-tauri/tests/`, and design references in `docs/`.

## Build, Test, and Development Commands
- `npm run dev` — start the full Tauri desktop app.
- `npm run dev:web` — run the Vite frontend only.
- `npm run build:web` — type-check and bundle web assets.
- `npm run build` — produce desktop binaries.
- `npm run typecheck` — run TypeScript validation for UI changes.
- `npm run test:unit` — run Vitest unit tests for frontend utilities and model helpers.
- `cargo test --manifest-path src-tauri/Cargo.toml` — run Rust integration tests.
- `cargo fmt --manifest-path src-tauri/Cargo.toml` — format Rust code before committing.

## Coding Style & Naming Conventions
Use 2-space indentation in TypeScript/TSX. Prefer functional React components with named exports. File names should be kebab-case, for example `workbench-top-bar.tsx`; components use PascalCase; hooks use `use...`. Prefer the `@/` alias over deep relative imports. Keep code close to the feature unless it is clearly reusable. Reuse design tokens from `src/app/styles/globals.css` and align UI work with `docs/design-spec.md`. In Rust, preserve the existing `commands/`, `core/`, `model/`, and `persistence/` separation.

## Testing Guidelines
Frontend utility and model regressions can be covered with Vitest tests colocated under `src/`; UI changes should still pass `npm run typecheck` and receive manual verification. Rust tests live in `src-tauri/tests/*.rs`; follow the existing milestone-style naming pattern such as `m1_5_agent_run.rs`. Add or update backend integration tests whenever command, persistence, or runtime behavior changes.

## Commit & Pull Request Guidelines
Follow Conventional Commits: `type(scope): short summary`, for example `feat(agent-session): enhance workspace context`. Common types include `feat`, `fix`, `refactor`, and `chore`. Keep scopes tied to the area changed. Pull requests should include a concise summary, linked issue or design doc when relevant, commands run, and screenshots or GIFs for visible UI changes. Call out migrations, capability updates, or setup steps explicitly.

## Settings Schema Version (`SETTINGS_STORAGE_SCHEMA_VERSION`)
The constant `SETTINGS_STORAGE_SCHEMA_VERSION` in `src/modules/settings-center/model/defaults.ts` controls whether the app discards cached localStorage settings and falls back to code-defined defaults on startup. When the stored schema version is lower than this constant, the app resets all settings to defaults. **You must increment this value** whenever you change any built-in default that needs to reach existing users, including but not limited to: updates to default slash command prompts (`cmd-commit`, `cmd-create-pr`, etc.), additions or removals of built-in commands, and changes to default model, provider, or other setting values in `DEFAULT_COMMAND_SETTINGS` or `DEFAULT_SETTINGS`. Without the increment, existing users will keep stale localStorage data and never see the updated defaults. Note that incrementing this value resets *all* user-customized settings, so only do it when shipping defaults that must override prior values.

## Post-Implementation Checklist
After completing a task, always run the relevant formatting and validation commands before committing: `cargo fmt --manifest-path src-tauri/Cargo.toml` for any Rust changes, `npm run typecheck` for any TypeScript/TSX changes, and `npm run test:unit` when frontend utility or model tests were added or affected. Fix all warnings and errors before finalizing the commit.

## Agent-Specific Instructions
Address the user as `Buddy` in all collaborator-facing responses for this repository. Pay special attention to cross-platform differences when coding, and preserve cross-platform compatibility in implementation choices.
