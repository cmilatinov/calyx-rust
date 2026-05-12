---
inclusion: auto
---

# HUD / Runtime UI System

This document captures the design requirements and testing strategy for the Calyx HUD / runtime UI system.

## Goals

- Provide a code-first runtime UI system for Calyx HUDs.
- Keep developer experience simple and ergonomic for gameplay code.
- Support responsive HUDs that adapt to different screen sizes without relying on hardcoded pixel layouts.
- Keep rendering backend-swappable so the first implementation can use egui while a later implementation can plug into the Calyx renderer.
- Reuse existing rendering capabilities where practical and avoid building a full custom renderer in the first pass.
- Make styles, themes, and layout data reflectable/serializable where practical so editor inspector support can be added later.

## Architecture

Use a Flutter-inspired pipeline:

```text
Widget tree -> Layout tree -> Paint commands -> Backend adapter
```

The core UI layer must not depend on egui, wgpu, or Calyx renderer internals. It should produce backend-neutral layout, event, and paint data.

Suggested core modules:

- `Widget`: composable UI components.
- `ElementId`: stable identity for widget state, hover state, focus, and event routing.
- `LayoutNode`: resolved tree with final screen-space rectangles.
- `PaintCommand`: backend-neutral drawing commands.
- `UiRuntime`: previous-frame state, hovered element, pressed element, focus, pointer capture, and theme.
- `UiContext`: frame-local API used by game code to build UI and receive events.
- `UiBackend`: adapter trait that consumes paint commands and sends them to egui or the Calyx renderer.

Initial backend:

- `EguiUiBackend`, using egui painter, egui text, egui clipping, and existing egui texture handles.

Future backend:

- `CalyxUiBackend`, using Calyx renderer primitives for batched quads, text, texture binding, clipping/scissor, and other renderer-native UI support.

## Developer Experience

Gameplay code should compose UI from primitives and custom widgets:

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

The API should favor typed builders and simple composition over string-heavy CSS-like configuration. Stable IDs should be available when a widget needs persistent state across frames.

## Primitive Widgets

Start with a small set of primitives:

- `Container`: background, border, radius, padding, margin, size constraints.
- `Text`: font token, color token, wrapping and overflow behavior.
- `Image`: texture asset, tint, and fit mode.
- `Row`: horizontal flex layout.
- `Column`: vertical flex layout.
- `Stack`: overlays and anchored HUD regions.
- `Button`: pointer states and callbacks.
- `IconButton`: icon-focused button variant.
- `ProgressBar`: health, reload, and similar HUD meters.
- `Spacer`: flexible empty space.
- `SizedBox`: explicit constraints when needed.
- `Padding`: simple spacing wrapper.
- `Center`: centering wrapper.
- `CustomPaint`: user callback that emits paint commands for custom HUD visuals.

## Layout

Use Flutter-style constraints with flexbox-inspired containers:

- Parent gives child constraints.
- Child returns desired size.
- Parent assigns final rectangles.

Supported layout concepts:

- Padding, margin, gap.
- Min and max constraints.
- Auto sizing.
- Fill sizing.
- Percent sizing.
- Flex grow, flex shrink, and basis for rows/columns.
- Main-axis and cross-axis alignment.
- Stack anchors: top-left, top-right, bottom-left, bottom-right, center.
- Safe-area/inset support for HUD placement.

Avoid requiring hardcoded pixel sizes. Pixel units can exist as logical points, but common HUD construction should rely on constraints, fill, percent, flex, anchors, and theme tokens.

## Responsive UI

Prefer explicit screen classes instead of Tailwind-like class strings:

```rust
match ctx.screen_class() {
    ScreenClass::Compact => compact_hud(),
    ScreenClass::Medium | ScreenClass::Wide => desktop_hud(),
}
```

Style and layout overrides can also be conditional:

```rust
style()
    .padding(Space::Md)
    .when(ScreenClass::Compact, |s| s.padding(Space::Sm))
```

Suggested screen classes:

- `Compact`
- `Medium`
- `Wide`

The exact breakpoint values can be tuned later. Keep the first implementation simple and deterministic.

## Styling

Use a simplified CSS box model without a full selector engine.

Core style fields:

- Display/layout mode.
- Position mode where needed.
- Width and height.
- Min/max width and height.
- Margin.
- Padding.
- Background.
- Border.
- Corner radius.
- Opacity.
- Font.
- Text color.
- Pointer-events behavior.

Use token-first themes:

```rust
Theme {
    colors: ColorTokens,
    spacing: SpaceTokens,
    fonts: FontTokens,
    radius: RadiusTokens,
    hud: HudTokens,
}
```

Named style presets/classes are allowed, but should be simple named lookups rather than a full CSS cascade:

```rust
container()
    .class("hud.panel")
    .style(|s| {
        s.padding(theme.space.md)
            .background(theme.colors.surface_translucent)
            .border(theme.colors.outline)
            .radius(theme.radius.sm)
    })
```

## Corner Radius

Corner radius is a core style feature, not an egui-only feature.

Support per-corner radius:

```rust
pub struct CornerRadius {
    pub top_left: UiUnit,
    pub top_right: UiUnit,
    pub bottom_right: UiUnit,
    pub bottom_left: UiUnit,
}
```

Provide convenience constructors:

- `CornerRadius::all(value)`
- `CornerRadius::top(value)`
- `CornerRadius::left(value)`
- `CornerRadius::none()`

MVP scope:

- Rounded backgrounds and borders are required.
- Rounded rectangle radius must be present in style and paint commands.
- Child overflow clipping is rectangular.
- Rounded child masking is deferred until there is a concrete UI need.

## Interaction

Pointer interaction should feel familiar to web development.

Required events:

- `PointerEnter`
- `PointerLeave`
- `PointerMove`
- `PointerDown`
- `PointerUp`
- `Click`
- `DragStart`
- `Drag`
- `DragEnd`

Dispatch model:

1. Hit-test the layout tree from front to back.
2. Skip nodes with `pointer_events: None`.
3. Pick the deepest eligible target.
4. Dispatch through phases:
   - `Capture`: root to target.
   - `Target`: target element.
   - `Bubble`: target back to root.
5. Events can stop propagation.
6. Events can pass through where appropriate.

Hover is required and should be first-class state. Widgets should be able to define hover, pressed, focused, and disabled style overrides.

Non-interactive HUD overlays should allow gameplay input to pass through. Interactive HUD panels should consume pointer input only when intended.

## Rendering

The core UI should emit backend-neutral paint commands:

- Rect fill.
- Rounded rect fill.
- Rect stroke.
- Rounded rect stroke.
- Text.
- Image.
- Clip push/pop.
- Custom paint command where needed.

The initial egui backend should translate these commands to egui painter calls. The later Calyx backend should translate the same commands into renderer-native operations.

Do not build a full custom renderer for the MVP. Keep rendering thin and swappable.

## Backend Boundary

Use a trait boundary similar to:

```rust
pub trait UiBackend {
    type TextureId;

    fn begin_frame(&mut self, viewport: UiRect, scale_factor: f32);
    fn push_clip(&mut self, rect: UiRect);
    fn pop_clip(&mut self);
    fn fill_rect(&mut self, rect: UiRect, style: FillStyle);
    fn stroke_rect(&mut self, rect: UiRect, style: StrokeStyle);
    fn draw_text(&mut self, rect: UiRect, text: &ResolvedText);
    fn draw_image(&mut self, rect: UiRect, texture: Self::TextureId, style: ImageStyle);
    fn end_frame(&mut self);
}
```

This exact API can change during implementation, but the core requirement is that gameplay widgets, layout, event dispatch, style resolution, and paint command generation remain backend-neutral.

## Editor Compatibility

Code-first usage has priority.

Where practical, the following should be reflectable/serializable:

- `Theme`
- `Style`
- `CornerRadius`
- Spacing/font/color/radius tokens
- Screen class overrides
- Widget/layout data that may become editor-authored later

Future editor support can include:

- Theme asset editing.
- Inspector editing of UI style resources.
- UI document or prefab-like authoring.
- Visual layout editing.

Do not make visual editor authoring a blocker for the MVP.

## Testing Strategy

Most tests should be deterministic and run without opening a window, creating a wgpu device, or depending on real egui rendering.

### Unit Tests

Layout algorithm:

- Row and column flex sizing.
- Padding, margin, gap.
- Min/max constraints.
- Percent, fill, and auto sizing.
- Nested containers.
- Screen-class responsive branches.
- Overflow and clipping rectangles.

Style and theme resolution:

- Token lookup.
- Named style preset lookup.
- Inline overrides.
- Responsive overrides.
- Hover, pressed, focused, disabled state overrides.
- Per-corner radius resolution.

Hit testing and event dispatch:

- Front-to-back hit testing.
- `pointer_events: None` pass-through.
- Capture, target, and bubble order.
- Stop propagation.
- Hover enter/leave transitions.
- Click rules.
- Drag capture and drag routing.

Widget behavior:

- Button hover/pressed/click state.
- Progress bar value clamping.
- Stable IDs preserving state across frames.
- Responsive HUD composition selecting expected layouts.

Paint command generation:

- Expected command count and order.
- Resolved colors, borders, radii, and clipping.
- Text and image placement.
- State-dependent paint output.

Backend adapter assertions:

- Feed known paint commands into a fake/mock backend.
- Assert rect fill/stroke, text, image, clip push/pop, and radius values are forwarded correctly.
- Backend adapter tests are still unit tests. They prove translation, not visual output.

### Integration Tests

Run the full UI frame loop using fake input and fake screen sizes:

1. Build widget tree.
2. Resolve layout.
3. Dispatch pointer events.
4. Update widget/UI state.
5. Generate paint commands.
6. Optionally send commands through a mocked backend.

Assert:

- Hover and pressed state changed correctly.
- Click and drag callbacks fired correctly.
- Gameplay-facing state changed correctly.
- Pass-through elements did not consume input.
- Interactive elements consumed input when intended.
- Responsive layout changed as expected.
- Final paint/backend command list is correct.

Screenshot tests, real windows, and real wgpu rendering are not required for the MVP.
