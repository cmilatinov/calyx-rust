---
inclusion: auto
---

# HUD / Runtime UI Testing

Most tests must be deterministic and run without a window, wgpu device, or real egui rendering.

## Unit Tests

Layout:

- Row and column flex sizing.
- Padding, margin, gap, and nested containers.
- Min/max constraints and percent/fill/auto sizing.
- Responsive screen-class branches.
- Overflow and clipping rectangles.

Style and theme resolution:

- Token and named-preset lookup.
- Inline and responsive overrides.
- Hover, pressed, focused, and disabled overrides.
- Per-corner radius resolution.

Hit testing and events:

- Front-to-back hit testing and `pointer_events: None` pass-through.
- Capture, target, and bubble order.
- Stop propagation.
- Hover enter/leave transitions.
- Click and drag capture rules.

Widget behavior:

- Button hover, pressed, and click state.
- Progress-bar clamping.
- Stable IDs preserving state.
- Responsive HUD composition.

Paint commands and backend adapters:

- Command count, order, resolved style, clipping, text, and image placement.
- State-dependent paint output.
- Translation of known commands through a fake backend.

Backend adapter tests prove translation, not visual output.

## Integration Tests

Run the complete frame loop with fake input and screen sizes:

1. Build the widget tree.
2. Resolve layout.
3. Dispatch pointer events.
4. Update UI state.
5. Generate paint commands.
6. Optionally send commands through a fake backend.

Assert state changes, callbacks, pass-through behavior, intended input consumption, responsive layout, and final paint/backend command output.

Screenshot tests, real windows, and real wgpu rendering are not required for the MVP.
