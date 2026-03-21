# Code Block Refinement Design

## Goal

Refine code blocks so they feel lighter, more elegant, and visually integrated with the desktop app in both light and dark themes.

## Approved Direction

- Follow the active application theme instead of forcing a fixed dark terminal look.
- Reduce visual weight by collapsing the current multi-layer framed look into a single, quieter surface.
- Keep language and actions available, but make them low-emphasis by default and clearer on hover/focus.
- Preserve existing behavior and syntax highlighting logic.

## Visual Decisions

- Use a single rounded container with a subtle border and restrained shadow.
- Compress the header into a thin metadata row rather than a strong toolbar.
- Remove the heavy inner framed look from the code area.
- Keep the code area clean, with enough breathing room and low-contrast line numbers.
- Use shared theme tokens so light mode feels paper-like and dark mode feels calm, not harsh.

## Implementation Scope

- Update `src/components/ai-elements/code-block.tsx` so the internal code block component matches the approved minimal design.
- Add lightweight theme-aware code block surface tokens in `src/app/styles/globals.css`.
- Override Streamdown code block chrome through targeted global selectors so markdown-rendered code blocks align with the same design.

## Non-Goals

- No changes to syntax highlighting themes or language support.
- No behavior changes to copying, downloading, or code rendering beyond presentation.
- No unrelated message or layout redesign.

## Validation

- Run `npm run typecheck`.
- Manually verify that both markdown code blocks and tool/output code panels share the new lighter presentation.
