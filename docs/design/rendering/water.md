# Water

`ComponentWater` draws stylized, opaque water as a river along a curve, with
waves displaced in the vertex shader, foam around its shores and around what
stands in it, and optional banks.

Code: `crates/calyx_engine/src/component/water.rs`,
`crates/calyx_engine/src/render/water.rs`,
`crates/calyx_engine/src/math/water.rs`,
`crates/calyx_engine/src/math/waterline.rs`,
`assets/shaders/mesh/water*.wgsl`. `sandbox/examples/water_test.rs` builds the
look-dev creek, carving the ground around it as a terrain heightmap from the
same curve.

## Mesh

- The river runs along a centripetal Catmull-Rom centreline through the
  component's `points` (local XZ, each with a width), cut into irregular
  facets about `cell_size` across (`WaterPath`: rows evenly spaced along the
  curve, columns across the widest point, inner vertices jittered,
  alternating diagonals).
- The CPU builds the mesh once per path, footprints and bank profile; every
  displacement happens in the vertex shader.
- Each triangle has three vertices of its own carrying its river coordinates
  (`s` along the flow, `t` across), its distance to the shore, its position in
  the frame of the footprint nearest the facet with that footprint's shape,
  and its other two corners' rest positions and river coordinates (see
  `WaterGeometry` for the channels).
- So the shader moves all three corners by the same layered waves rolling
  downstream (two trains, a cross ripple and drifting value noise;
  `wave_amplitude`, `wave_length`, `flow_speed`), and every vertex computes the
  same moved facet normal, so the surface stays watertight.

## Shading

- `shading` picks the lighting. `Smooth` (the default) takes each vertex's
  normal from the waves' slope (central differences in river coordinates,
  carried into local XZ through its triangle's map between the two),
  interpolated across the facets, with colour per pixel and glints as small
  twinkling sparkle points. `Faceted` shades each facet flat with its own moved
  normal and one colour, and glints flash whole facets.
- The colour runs from shallow at the shore to deep over `shore_distance`.
- Where the waves turn most of the way towards mirroring the brightest enabled
  directional light into the camera, it glints as HDR emission
  (`glint_strength`).

## Foam and obstacles

- Foam is per pixel in hard-edged shapes driven by the interpolated shore
  distance, the per-pixel distance to the facet's footprint and scrolling
  noise: a broken band along both shores (`foam_width`), flecks, patches
  drifting downstream (`foam_patches`), and the same band and flecks around
  everything standing in the water (`obstacle_foam`), over its own shallows
  (`obstacle_shallows`).
- What stands in the water is found from the scene. `WaterCache` cuts every
  visible `ComponentMesh` whose box crosses the water's rest plane (local
  Y = 0) near it along that plane (`math::waterline`), following the scene's
  change journal so only objects that changed are cut again.
  `WaterPath::footprints` fits a rounded rectangle (`WaterFootprint`) around
  each piece that lies over the water and is at most 8 m across, merged with
  the authored `obstacles` circles where they overlap.
- A bridge deck clear of the surface cuts nothing; its piers do.
- A footprint is the right tool here rather than depth-based intersection
  foam, because the water draws opaque into the G-buffer pass and cannot read
  the depth of the scene it is drawn with.

## Banks

With a `bank_material`, the component also draws its banks along the same
curve (`WaterPath::banks`): a wall from `bank_depth` below the rest surface up
to `bank_height` over `bank_run`, crossing the rest surface at the shore, then
a flat `bank_lip`. They cast shadows.

## Rendering

- The draw collection's `WaterCache` keeps the meshes per owner and a
  renderer-owned material on `shaders/mesh/water`, filled from the component
  every frame (linear colours, the water's wall-clock time, the light), so
  inspector edits show at once in edit mode and in play.
- Water draws through the ordinary mesh batches with `cast_shadows` off, and
  the G-buffer's geometry pass draws it with `shaders/mesh/water_gbuffer.wgsl`,
  as it draws PBR meshes with `gbuffer.wgsl`. So the deferred pass lights and
  shadows it, and SSAO, fog and the Outline effect treat it as any opaque
  surface.

## Not covered

No transparency, refraction, scene reflections or interaction with tanks and
shells. Gameplay blocking is separate: the sandbox's `TANK_BLOCKER` collision
group ([Sandbox Gameplay](../sandbox/gameplay.md#collision-groups)).
