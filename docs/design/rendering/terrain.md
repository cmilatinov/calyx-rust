# Terrain

`ComponentTerrain` draws a heightmap texture as a ground surface through the
ordinary mesh batches, and gameplay code samples its exact height. Splatting
is a material: the blended ground shader.

Code: `crates/calyx_engine/src/component/terrain.rs`,
`crates/calyx_engine/src/render/terrain.rs`,
`crates/calyx_engine/src/math/heightfield.rs`,
`assets/shaders/mesh/ground*.wgsl`.

## Surface

- The terrain draws the heightmap texture's brightness centered on its owner,
  `size` wide (X) and deep (Z), from local Y = 0 (black) to `height_scale`
  (white), with its material, through the ordinary mesh/material/shadow
  batches. So it is shaded in the deferred pass, casts shadows when
  `cast_shadows` is set and receives them.
- The heights come from `Texture::heightfield`, which decodes the texture's
  image file once at the file's precision (a 16-bit grayscale PNG keeps its
  65536 levels; the GPU copy is 8-bit) and is dropped with the texture on
  reload. A texture not loaded from a file has none, and its terrain draws
  nothing.
- A `Heightfield` holds one sample per pixel, at most 1024 per side (a larger
  image is resampled down), with the image's top row on the +Z edge. Its mesh
  has a vertex per sample and two triangles per cell, and the renderer caches
  it per owner, rebuilding only for a new heightfield, size or height scale.
- The terrain is meant to stay upright (turned only around Y).
- There is no collider, LOD or sculpting.

## Sampling

Gameplay code reads the ground with `ComponentTerrain::sample(scene, terrain,
xz)` or, over every enabled terrain, `ComponentTerrain::sample_scene(scene,
xz)`: the world height and normal where a vertical line meets the drawn
triangles, exactly. The demos project's rover (`demos/src/terrain.rs`) drives
over `demos/assets/terrain/terrain_demo.cxscene`, a 512 x 512 terrain, this
way.

## Ground shader

A terrain drawn with `shaders/mesh/ground` mixes eight ground layers (grass,
dark and light grass, dirt, sand, gravel, field with crop rows, mud) from two
RGBA masks laid once over the terrain by its UVs. Its uniform,
`GroundProperties`, documents every input.

- Each layer is a palette colour shaded by a greyscale detail channel tiled in
  world space.
- World-space noise warps the masks' edges, picks between two turned detail
  samples against visible tiling, and adds broad light and shade.
- The detail acts as each layer's height for ragged edges, and fields keep
  crisp edges read from the unwarped mask.
- The geometry pass draws it with `shaders/mesh/ground_gbuffer.wgsl`.
- Its `marks_texture`, over the world rectangle `marks_rect` (x, z, width,
  depth), multiplies the ground's colour, white by default. It is the hook
  other effects darken or tint the ground through, as the sandbox's
  [track marks](../sandbox/gameplay.md#track-marks) do.

The sandbox arena's ground is one such terrain, written by `remake_arena`
(`sandbox/examples/remake_arena/ground.rs`).
