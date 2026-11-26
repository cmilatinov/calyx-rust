# Grass

Grass is a PBR material on `shaders/mesh/grass` that sways in the wind, bends
away from benders, and fades to show what stands in it.

Code: `crates/calyx_engine/src/component/grass.rs`,
`crates/calyx_engine/src/render/gbuffer.rs`,
`crates/calyx_engine/src/render/mesh_renderer.rs` (`MeshRenderer::render_grass`),
`assets/shaders/mesh/grass*.wgsl`.

## Drawing

- A material whose shader is `shaders/mesh/grass` is a PBR material drawn by
  the geometry pass with `grass_gbuffer.wgsl`, which sways each vertex in the
  wind and bends it away from the scene's grass benders by its height above
  its mesh's origin.
- The geometry pass leaves out the grass within each bender's reveal radius.
  `shaders/mesh/grass.wgsl` draws that grass after the lighting and forward
  passes in `MeshRenderer::render_grass`, alpha blended and lit through
  `pbr_lighting` (the grass field at group 0 binding 8), fading from opaque at
  the radius to 35 % a third of the way in, with no ink.
- Grass normals bend most of the way up, as foliage is lit.
- Shadows are cast from the grass at rest.

## The grass field

The first enabled `ComponentGrassField` in the scene supplies the wind
(`sway`, `sway_speed`, `bend`, `height`) and, as runtime state game code
writes every frame, up to `MAX_GRASS_BENDERS` benders. The renderer uploads
them with its own wrapped clock as group 3.

The sandbox's tall-grass brush feeds the benders; see
[Sandbox Gameplay](../sandbox/gameplay.md#brush).
