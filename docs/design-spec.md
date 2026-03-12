# Tiy Agent Design Spec

## Overview

Tiy Agent's current UI baseline is a desktop workbench rather than a flowing marketing page. The system prioritizes fixed viewport composition, multi-panel collaboration, high information density, low-saturation neutrals, and stable interaction feedback.

This document captures the visual rules already established in the codebase and should be treated as the primary design reference for future UI work.

## Reference Sources

- `src/app/styles/globals.css`
- `src/widgets/dashboard-overview/ui/dashboard-overview.tsx`
- `src/shared/ui/button.tsx`
- `src/shared/ui/card.tsx`
- `src/shared/ui/input.tsx`
- `src/app/providers/theme-provider.tsx`

## Layout System

### Workbench Shell

- The app uses a fixed desktop shell with a thin top chrome bar, side panels, a central content area, and an optional bottom terminal.
- The top bar height is `36px`.
- The main workbench is a three-column layout:
  - left sidebar: `320px`
  - main content: fluid
  - right drawer: `360px`
- The bottom terminal defaults to `260px`, with a minimum height of `180px`, and supports drag resize.

### Scrolling Model

- Global page scrolling is disabled.
- `html`, `body`, and `#root` remain full height with `overflow: hidden`.
- Scrolling is delegated to panel-local containers only.
- This behavior is part of the product language and should be preserved for new workbench screens.

### Content Width and Grid

- The product does not use a standard 12-column marketing grid.
- The dominant pattern is:
  - workbench shell composed with `flex`
  - content stacks composed with vertical spacing
  - information clusters composed with local `grid`
- Main reading content is constrained to `max-w-4xl` and centered.
- Compact information cards expand from single column to `md` two-column and `xl` three-column layouts.

## Theme and Color Tokens

### Theme Model

- Color is defined with OKLCH tokens.
- Supported theme preferences are `system`, `light`, and `dark`.
- The resolved theme is applied at runtime through:
  - `html.dark`
  - `data-theme`
  - `color-scheme`

### Token Layers

The system currently has two token tiers:

- semantic UI tokens:
  - `background`
  - `foreground`
  - `card`
  - `primary`
  - `border`
  - `muted`
- workbench-specific tokens:
  - `app-canvas`
  - `app-sidebar`
  - `app-drawer`
  - `app-chrome`
  - `app-terminal`
  - `app-surface`
  - `app-surface-muted`
  - `app-surface-hover`
  - `app-surface-active`
  - `app-border`
  - `app-border-strong`
  - `app-foreground`
  - `app-muted`
  - `app-subtle`
  - `app-code`
  - `app-overlay`
  - `app-success`
  - `app-danger`
  - `app-info`

### Color Behavior

- The primary action style is intentionally restrained.
- In light mode, `primary` behaves like a near-black button with light foreground text.
- In dark mode, `primary` flips to a near-light button with dark foreground text.
- The main workbench hierarchy is expressed through cool blue-gray neutrals, not through saturated brand accents.
- Semantic accent colors are used sparingly for state cues only:
  - success
  - danger
  - informational status
- Surface and overlay colors often include alpha, creating a soft frosted or layered feel without heavy contrast.

## Spacing, Radius, and Typography

### Radius

- The radius base token is `--radius: 0.75rem`.
- The interface commonly uses `rounded-xl` and `rounded-2xl`.
- The result should feel soft and modern, but not overly card-heavy or playful.

### Sizing and Density

- Standard controls such as buttons and inputs use a height around `36px` (`h-9`).
- Toolbar icon buttons are often compressed to around `28px`.
- Spacing is compact and tuned for dense desktop workflows rather than mobile-first breathing room.

### Type

- Primary UI font: `Inter`
- System fallbacks remain enabled for platform consistency.
- Typical text sizes cluster around:
  - `12px` for metadata and supporting labels
  - `13px` for code-like and compact utility text
  - `14px` for primary body and panel titles
- Main section headings often use `14px` with `semibold`.
- Terminal output uses a monospace face.
- Metadata labels frequently use uppercase plus expanded tracking to communicate "system" or "panel" semantics.

## Component Rules

### Buttons

- Base button behavior comes from the shared shadcn-style primitive.
- Buttons must preserve:
  - hover feedback
  - focus ring visibility
  - disabled state clarity
- Workbench views commonly override buttons toward lighter-weight variants:
  - ghost
  - icon
  - panel toggle

### Cards

- `Card` is the default information container.
- In the workbench, cards should usually avoid strong elevation.
- Prefer:
  - `bg-app-surface`
  - `border-app-border`
  - minimal or removed shadow

### Inputs and Composer Fields

- Inputs and textareas should visually merge into the workbench shell.
- Preferred behavior:
  - transparent or near-transparent background
  - light border
  - strong focus ring
  - subtle placeholder text

## Interaction and Motion

### Motion

- Panel transitions use `width`, `opacity`, and `transform`.
- Standard duration is `300ms`.
- Standard easing is `cubic-bezier(0.22,1,0.36,1)`.
- Motion should feel deliberate and desktop-like, not bouncy or promotional.

### State Hierarchy

- Hover feedback is more important than click theatrics.
- Preferred hover language:
  - slightly brighter surface
  - stronger foreground contrast
  - subtle shadow
  - fine underline or bottom indicator where appropriate
- Active and current states should be expressed primarily through:
  - stronger borders
  - deeper surface fills
  - thin active indicators

### Accessibility

- Shared primitives already provide focus ring support and disabled handling.
- Custom workbench controls should continue to expose clear keyboard focus visibility.
- New UI should preserve readable contrast between:
  - canvas and surface
  - surface and border
  - foreground and muted text

## Implementation Guidance

- Reuse `app-*` tokens before introducing hard-coded colors.
- Prefer extending the existing workbench shell rather than inventing isolated page-level layout systems.
- If a new token is required, decide whether it belongs to:
  - the shared semantic token layer
  - the workbench-specific `app-*` layer
- Avoid page-local token naming unless there is a strong reason.
- Keep README as a lightweight entry point and keep the full design baseline in this file.

## Validation Checklist

- Verify light, dark, and system theme modes preserve the canvas, surface, border, and foreground hierarchy.
- Verify sidebar, drawer, and terminal open and close without layout jank or body scrolling.
- Verify hover, active, disabled, and focus-visible states remain consistent across buttons, thread items, menus, and inputs.
- Verify the centered content column remains readable on narrow widths and that local grids expand cleanly from one to two to three columns.

## Current Scope Notes

- This spec is based on the current single primary workbench screen in the repository.
- It documents the most stable visual patterns already present, not a fully abstracted enterprise design system.
- If the product later introduces multiple major surfaces such as settings, onboarding, or marketing pages, this spec should be extended with page-type-specific rules rather than replacing the workbench baseline.
