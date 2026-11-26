# Model Generator

`calyx_model_gen` generates the sandbox's low-poly clay models in headless
Blender from TOML parameter files, with their materials and per-map prefabs.
Its `--help` lists the options; it runs as
`cargo run --release -p calyx_model_gen -- ...`. The look it serves is in
[Tank Game Visual Direction](../art/tank-game-direction.md).

Code: `crates/calyx_model_gen/src/`.

## Parameters

Each model is one TOML parameter file in `sandbox/asset_sources/models/`
(`params.rs`):

- a `kind`: `box`, `rock`, `cylinder`, `blob`, `prism`, or `import` for a
  sourced CC0 or CC-BY model with its `credit`;
- its sizes and bevel, a palette `role`, `shading` (`smooth` with a
  `smooth_angle`, or `flat`) and an optional `outline` hull thickness.

Shapes:

- Prism footprints provide clipped paint stripes, Jersey barrier sections and
  tileable wall junctions.
- Cylinders can taper through `top_radius`.
- Blobs can fuse their lobes through a voxel remesh (`fuse`).
- An assembly uses named `[[parts]]` tables, each with its own shape, palette
  role, offset and XYZ degree rotation. The generator emits one OBJ per part
  and a stable-id prefab per map in `generated/prefabs/<map>/`, keeping each
  part's mesh, material and transform under one root (`prefab.rs`).
  Single-shape files keep their output paths and ids.

## Output

- The embedded Blender script (`blender.rs`, `blender_models.py`) builds each
  model and writes it in Calyx's left-handed Y-up space, with an inverted ink
  hull when asked.
- The generator checks every OBJ (`obj.rs`: facing out, normals matching the
  shading, standing on y = 0, spanning its size) before writing its `.meta`.
- Materials are written per map for each role the models use (`palette.rs`,
  the direction's palette sRGB-decoded) into
  `sandbox/assets/generated/materials/<map>/<role>.cxmat`, meshes into
  `sandbox/assets/generated/models/`.
- Ids are version 5 UUIDs of the asset names (`layout.rs`, `ids.rs`), so
  regenerating keeps them. Files an earlier run wrote and this one does not
  are removed.
- An `import` model's source must be credited in
  `sandbox/THIRD_PARTY_ASSETS.md` (`credits.rs`).
