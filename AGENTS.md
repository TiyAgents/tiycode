# Repository Guidelines

## Project Structure & Module Organization
The desktop UI lives in `src/` with React + TypeScript. Use `src/app/` for bootstrap, routing, providers, and global styles; `src/modules/` for domain areas such as workbench, settings, marketplace, and the newer extensions center surfaces; `src/features/` for platform-facing features like terminal and system metadata; `src/shared/` for reusable UI, types, config, and helpers; and `src/services/` for bridge and streaming integrations. Static assets belong in `public/`. Tauri/Rust code lives in `src-tauri/src/`, with extension host/runtime code now under `src-tauri/src/extensions/`; migrations stay in `src-tauri/migrations/`, backend integration tests in `src-tauri/tests/`, and design references in `docs/`.

## Build, Test, and Development Commands
- `npm run dev` — start the full Tauri desktop app.
- `npm run dev:web` — run the Vite frontend only.
- `npm run build:web` — type-check and bundle web assets.
- `npm run build` — produce desktop binaries.
- `npm run typecheck` — run TypeScript validation for UI changes.
- `cargo test --manifest-path src-tauri/Cargo.toml` — run Rust integration tests.
- `cargo fmt --manifest-path src-tauri/Cargo.toml` — format Rust code before committing.

## Coding Style & Naming Conventions
Use 2-space indentation in TypeScript/TSX. Prefer functional React components with named exports. File names should be kebab-case, for example `workbench-top-bar.tsx`; components use PascalCase; hooks use `use...`. Prefer the `@/` alias over deep relative imports. Keep code close to the feature unless it is clearly reusable. Reuse design tokens from `src/app/styles/globals.css` and align UI work with `docs/design-spec.md`. In Rust, preserve the existing `commands/`, `core/`, `model/`, and `persistence/` separation.

## Testing Guidelines
No frontend unit test runner is configured yet, so every UI change should pass `npm run typecheck` and receive manual verification. Rust tests live in `src-tauri/tests/*.rs`; follow the existing milestone-style naming pattern such as `m1_5_agent_run.rs`. Add or update backend integration tests whenever command, persistence, or runtime behavior changes.

## Commit & Pull Request Guidelines
Follow Conventional Commits: `type(scope): short summary`, for example `feat(agent-session): enhance workspace context`. Common types include `feat`, `fix`, `refactor`, and `chore`. Keep scopes tied to the area changed. Pull requests should include a concise summary, linked issue or design doc when relevant, commands run, and screenshots or GIFs for visible UI changes. Call out migrations, capability updates, or setup steps explicitly.

## Post-Implementation Checklist
After completing a task, always run the relevant formatting and validation commands before committing: `cargo fmt --manifest-path src-tauri/Cargo.toml` for any Rust changes, and `npm run typecheck` for any TypeScript/TSX changes. Fix all warnings and errors before finalizing the commit.

## Agent-Specific Instructions
Address the user as `Jorben` in all collaborator-facing responses for this repository.
