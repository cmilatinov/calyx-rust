# HUD / Runtime UI Behavior

See [HUD / Runtime UI Architecture](architecture.md) for layout and styling requirements.

## Interaction

Required pointer events:

- `PointerEnter`, `PointerLeave`, and `PointerMove`.
- `PointerDown`, `PointerUp`, and `Click`.
- `DragStart`, `Drag`, and `DragEnd`.

Dispatch rules:

1. Hit-test the layout tree front to back.
2. Skip nodes with `pointer_events: None`.
3. Pick the deepest eligible target.
4. Dispatch capture from root to target.
5. Dispatch to the target.
6. Bubble from target to root.

Events can stop propagation or pass through where appropriate. Hover, pressed, focused, and disabled states are first-class style inputs.

Non-interactive HUD overlays must allow gameplay input through. Interactive panels consume pointer input only when intended.

## Rendering

The core emits backend-neutral commands for:

- Rectangle and rounded-rectangle fill.
- Rectangle and rounded-rectangle stroke.
- Text and image drawing.
- Clip push/pop.
- Custom paint operations when needed.

The egui backend translates these commands to egui painter operations. A future Calyx backend translates the same commands into renderer-native operations. Do not build a full custom renderer for the MVP.

## Backend Boundary

Keep a trait boundary similar to:

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

The exact API may evolve. Gameplay widgets, layout, event dispatch, style resolution, and paint-command generation must remain backend-neutral.

## Editor Compatibility

Code-first usage has priority. Where practical, make `Theme`, `Style`, `CornerRadius`, design tokens, screen-class overrides, and reusable layout data reflectable and serializable.

Future editor support may include theme assets, inspector-editable style resources, UI documents, and visual layout editing. Do not make visual authoring an MVP blocker.

## Styled text and gradients

`PaintCommand::Label` carries a backend-neutral font face, anchor, optional
maximum width and outline. The backend measures and truncates using the actual
font layout; gameplay code never estimates character advances. The egui adapter
loads bundled Barlow Bold/Bold Italic (OFL license and provenance
are adjacent to the font assets). An outline dilates the laid-out glyph coverage
mask and paints one image. A bounded 128-entry texture cache reuses those masks;
fade and pop apply as image tint and scale instead of rerasterizing each frame.

`ColorMesh` carries indexed vertices with individual colours for gradients,
including soft occluded radar visibility and team leader highlights. The
recording backend preserves these primitives for tests.

World HP annotations project `Scene::visual_bound_corners`: transformed local
bounds of visible enabled static meshes below the actor. They therefore follow
tank scale, hierarchy and camera distance instead of a fixed world-height guess.
