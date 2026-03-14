# Marketplace Redesign Spec

Date: 2026-03-14
Status: Approved for planning

## Summary

Redesign the Marketplace overlay to align with `docs/design-spec.md` using an elegant, minimal `Soft Glass` direction. The Marketplace keeps its current overlay plus right-side drawer structure, but shifts the primary experience toward cleaner catalog browsing with a lighter visual hierarchy and reduced item density.

## Goals

- Match the desktop workbench visual language defined in `docs/design-spec.md`
- Make Marketplace browsing feel calmer, lighter, and more intentional
- Convert list sections to three-column grids on desktop widths
- Reduce item density so cards support faster scanning
- Preserve clear distinction between installed items and recommended items
- Keep details and secondary actions in the right drawer instead of in the list

## Non-Goals

- No changes to install, uninstall, enable, or disable behavior
- No new filtering, sorting, or recommendation logic
- No changes to Marketplace data shape or persistence
- No redesign of other overlays or workbench surfaces

## Constraints

- Must use existing `app-*` semantic tokens rather than base shadcn tokens
- Must preserve the fixed-viewport workbench model with local scrolling only
- Must remain visually restrained: soft glass, not decorative or marketing-like
- Must keep the right-side details drawer
- Must keep separate `Installed` and `Recommended` sections

## Approved Direction

The approved visual direction is `Soft Glass Catalog`.

This means:

- Light layered surfaces with subtle translucency and soft contrast shifts
- Calm neutral palette driven by existing workbench tokens
- Minimal hover and selection cues
- Compact but breathable desktop spacing
- Stronger browsing rhythm through grid alignment instead of heavy card chrome

## Information Architecture

### Overall structure

The Marketplace overlay keeps the existing three major areas:

1. Header
2. Toolbar and catalog content
3. Right-side details drawer

### Header

The header remains a compact workbench-style title area and includes:

- Back action
- `Marketplace` title
- Current tab indicator
- Short single-line supporting description
- Lightweight summary badges for installed, enabled, and recommended counts
- `Add source` button

The header should feel lighter than the current implementation. Summary badges remain visible but should not compete with the title.

### Toolbar

The toolbar consolidates:

- Tab switching
- Search input
- Section context copy

This area should read as a single quiet control band rather than as multiple stacked content blocks.

### Catalog sections

Each tab continues to render two sections:

- `Installed`
- `Recommended`

Each section uses a three-column grid at large desktop widths. At narrower widths the layout may collapse to two columns or one column, but desktop should treat three columns as the default target.

## Card Design

### Card content

Marketplace list cards will use the approved `balanced` density model.

Each card shows only:

- Item icon
- Item name
- One-line summary, visually capped at two lines
- One secondary metadata field
- Lightweight state badge
- One primary action

### Metadata rules

- Installed items prefer showing `version`
- Not-installed items prefer showing `sourceLabel`
- Tag collections are removed from list cards

### Action rules

- Not installed: `Install`
- Installed and enabled: `Disable`
- Installed and disabled: `Enable`

`View` is removed as a persistent list button. Opening details moves to clicking the card itself.

`Uninstall` is removed from the list card and only appears in the drawer.

### Card states

- Default: quiet surface, subtle border
- Hover: slightly stronger border and surface emphasis
- Selected: use `app-surface-active` and `app-border-strong`
- Installed state badge: low-noise semantic emphasis
- Available state badge: neutral, not attention-grabbing

Cards should be visually consistent in height within the grid to improve scan rhythm.

## Drawer Design

The drawer stays in place but becomes more like a workbench inspector and less like a stack of separate cards.

### Drawer content

The drawer includes:

- Icon, name, version, and state at the top
- Full description
- Publisher, source, and category details
- Capability tags
- Secondary actions including uninstall

### Drawer styling

- Reduce card-within-card feeling
- Use weaker separations between content blocks
- Preserve clear action grouping without adding heavy shadows or extra containers

The drawer remains the place for full detail and destructive actions.

## Interaction Model

- Clicking a card opens its drawer details
- Esc still closes the drawer
- Search remains per-tab
- Changing tabs closes the drawer if the selected item no longer belongs to the active tab
- Empty states remain but use quieter dashed or muted surfaces

Hover, active, and selected feedback should stay subtle and token-driven.

## Responsive Behavior

- Desktop target: three-column section grids
- Medium widths: degrade to two columns
- Narrow widths: degrade to one column
- Drawer should continue to overlay from the right without introducing page scroll

The redesign is primarily desktop-first and should preserve usability across narrower overlay widths without compromising desktop density targets.

## Implementation Scope

Primary implementation is expected in:

- `src/modules/marketplace-center/ui/marketplace-overlay.tsx`

No model or storage changes are expected.

## Testing and Validation

Implementation should be validated against:

- Visual alignment with `docs/design-spec.md`
- Correct rendering for each tab
- Correct separation of installed versus recommended items
- Card action correctness for installed and non-installed states
- Drawer open and close behavior
- Search filtering behavior
- Stable grid behavior across desktop and narrower widths
- Light and dark theme compatibility

## Risks

- Three-column density can feel cramped if card internals are not aggressively simplified
- Soft glass styling can drift into decorative UI if borders, blur, or shadows are too strong
- Removing inline `View` must still leave card click affordance obvious enough for users

## Recommendation

Proceed with implementation as a focused UI refactor of the Marketplace overlay. Keep logic intact and treat this as a visual and interaction simplification pass centered on catalog readability.
