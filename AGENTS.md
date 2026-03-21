# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the TypeScript/React desktop UI. Use `src/app/` for app bootstrap, providers, routing, and global styles; `src/modules/` for domain areas such as the workbench shell, settings center, and marketplace; `src/features/` for platform-facing features like terminal and system metadata; `src/shared/` for reusable UI, types, config, and helpers; and `src/services/` for bridge and streaming integrations. Static assets live in `public/`. Rust/Tauri code lives in `src-tauri/src/`, with database migrations in `src-tauri/migrations/`, integration tests in `src-tauri/tests/`, and design notes in `docs/`.

## Build, Test, and Development Commands
Use `npm run dev` to launch the full Tauri desktop app and `npm run dev:web` to run the Vite frontend alone. Run `npm run build:web` to type-check and bundle the web assets, and `npm run build` to produce desktop binaries. Use `npm run typecheck` for TypeScript-only validation. For backend coverage, run `cargo test --manifest-path src-tauri/Cargo.toml`. When editing Rust, format with `cargo fmt --manifest-path src-tauri/Cargo.toml`.

## Coding Style & Naming Conventions
Follow the existing 2-space indentation in TypeScript/TSX. Prefer functional React components with named exports. Keep filenames in kebab-case, for example `workbench-top-bar.tsx`, while exported components use PascalCase and hooks use `use...` names. Use the `@/` path alias instead of deep relative imports. Add new shared primitives only when they are reused across modules; otherwise keep code close to its feature. For UI work, reuse the tokens in `src/app/styles/globals.css` and align with `docs/design-spec.md`. Rust changes should preserve the current `commands/`, `core/`, `model/`, and `persistence/` separation.

## Testing Guidelines
There is no frontend unit test runner configured yet, so every UI change should at least pass `npm run typecheck` and receive manual verification. Backend tests live in `src-tauri/tests/*.rs`; follow the existing milestone-style naming such as `m1_5_agent_run.rs`. Add or update Rust integration tests for command, persistence, and runtime changes whenever behavior changes.

## Commit & Pull Request Guidelines
Match the recent Conventional Commit style: `type(scope): short summary`, for example `feat(agent-session): enhance workspace context`. Common types in history include `feat`, `fix`, `refactor`, and `chore`. Keep scopes tied to the area you changed. Pull requests should include a concise summary, linked issue or design doc when applicable, commands run, and screenshots or GIFs for visible UI changes. Call out migrations, capability updates, or setup steps explicitly.

## Agent-Specific Instructions
Address the user as `Jorben` in all collaborator-facing responses for this repository.
