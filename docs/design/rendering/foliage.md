# Foliage

`ComponentFoliage` scatters prefabs over a rectangle and draws their static
meshes through the ordinary mesh batches, without instantiating the prefabs.

Code: `crates/calyx_engine/src/component/foliage.rs`,
`crates/calyx_engine/src/render/foliage.rs`,
`crates/calyx_engine/src/render/foliage/prefabs.rs`. The component's field
documentation defines units and limits.

## Layout

- The component scatters equally weighted prefab choices over its centered
  local XZ rectangle.
- Density, seed, uniform scale and Y rotation ranges determine a
  deterministic layout; inspector edits regenerate it in edit mode or play.
- The owner places the prefab origin on local Y = 0. Terrain projection,
  brushes and wind remain outside this component.

## Caching

- Each renderer caches local placements per scene and owner, and separately
  caches every visible, enabled static mesh part of each prefab, preserving
  its material, root/nested transforms and shadow setting.
- A fingerprint of borrowed visual data checks each used prefab once per
  frame, so in-place edits and reloads refresh extraction without rebuilding
  unchanged placements; mesh and material references resolve afresh.

## Drawing

- Instances use the ordinary mesh/material/shadow batches and share owner
  picking, visibility and enable state.
- Missing assets or meshless prefabs draw nothing. Hidden or malformed
  transform branches are pruned, and cycles, dangling links and unreachable
  objects cannot create extra parts.
- Scripts, physics, particles, lights and skinned animation are never
  instantiated.
