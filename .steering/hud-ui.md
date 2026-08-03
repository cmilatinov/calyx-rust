---
inclusion: auto
---

# HUD / Runtime UI Architecture

Related guidance:

- [Runtime and backend](hud-ui-runtime.md)
- [Testing strategy](hud-ui-testing.md)

## Goals

- Provide a code-first runtime UI system for Calyx HUDs.
- Keep gameplay composition simple and ergonomic.
- Support responsive layouts without relying on hardcoded pixel placement.
- Keep rendering backend-swappable between egui and a future Calyx renderer backend.
- Reuse existing rendering capabilities instead of building a custom renderer for the MVP.
- Make themes, styles, and layout data reflectable and serializable where practical.

## Pipeline

Use a Flutter-inspired pipeline:

```text
Widget tree -> Layout tree -> Paint commands -> Backend adapter
```

The core UI layer must not depend on egui, wgpu, or Calyx renderer internals. It produces backend-neutral layout, event, and paint data.

Core concepts:

- `Widget`: composable UI components.
- `ElementId`: stable identity for state, focus, hover, and event routing.
- `LayoutNode`: resolved tree with final screen-space rectangles.
- `PaintCommand`: backend-neutral drawing commands.
- `UiRuntime`: retained frame state, focus, pointer capture, and theme.
- `UiContext`: frame-local API for building UI and receiving events.
- `UiBackend`: adapter that consumes paint commands.

Use `EguiUiBackend` initially. A future `CalyxUiBackend` can translate the same commands into renderer-native primitives.

## Developer Experience

Gameplay code composes typed primitives and custom widgets:

```rust
hud.root()
    .padding(theme.space.lg)
    .child(
        row()
            .align_items(Align::Center)
            .gap(theme.space.md)
            .child(health_bar(player.health))
            .child(ammo_counter(player.ammo, player.reload))
    );
```

Favor typed builders over string-heavy CSS configuration. Provide stable IDs where widgets need state across frames.

## Primitive Widgets

Start with:

- `Container`, `Text`, and `Image`.
- `Row`, `Column`, and `Stack`.
- `Button` and `IconButton`.
- `ProgressBar` for health, reload, and similar meters.
- `Spacer`, `SizedBox`, `Padding`, and `Center`.
- `CustomPaint` for backend-neutral custom HUD visuals.

## Layout

Use Flutter-style constraints with flexbox-inspired containers:

- A parent gives child constraints.
- A child returns its desired size.
- The parent assigns final rectangles.

Support padding, margin, gap, min/max constraints, auto/fill/percent sizing, flex grow/shrink/basis, axis alignment, stack anchors, and safe-area insets.

Logical pixel units may exist, but common HUD composition should prefer constraints, fill, percent, flex, anchors, and theme tokens.

## Responsive UI

Prefer explicit screen classes over class strings:

```rust
match ctx.screen_class() {
    ScreenClass::Compact => compact_hud(),
    ScreenClass::Medium | ScreenClass::Wide => desktop_hud(),
}
```

Style and layout overrides may be conditional. Keep `Compact`, `Medium`, and `Wide` breakpoints simple and deterministic.

## Styling

Use a simplified CSS box model without a selector engine. Support display/layout mode, optional positioning, dimensions and constraints, margin, padding, background, border, per-corner radius, opacity, font, text color, and pointer-events behavior.

Use token-first themes for colors, spacing, fonts, radii, and HUD-specific values. Named presets are simple lookups rather than a CSS cascade.

Corner radius is a core style and paint-command feature, not an egui detail. Rounded backgrounds and borders are required. Rectangular child clipping is sufficient for the MVP; rounded child masking is deferred until needed.
