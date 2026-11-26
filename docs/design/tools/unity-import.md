# Unity Import

`calyx_unity_import` imports a Unity Asset Store pack into a Calyx project:
its models, materials, prefabs and, when asked, a scene. Its `--help` lists
the options; it runs as `cargo run --release -p calyx_unity_import -- ...`.

Code: `crates/calyx_unity_import/src/`.

## General parts

- `package.rs` reads a `.unitypackage`: a gzipped tar of
  `<guid>/pathname`, `asset` and `asset.meta`.
- `yaml.rs` reads Unity's multi-document YAML.
- `material.rs` converts Standard and Unlit materials to Calyx PBR materials.
  A renderer drawing a built-in Unlit material casts no shadow, as Unity's
  Unlit shaders have no shadow caster pass.
- `prefab.rs` resolves prefabs, prefab variants and model prefabs with their
  modifications.
- `scene.rs` converts a `.unity` scene's hierarchy.
- `blender.rs` runs headless Blender with an embedded script
  (`blender_convert.py`) that converts FBX to OBJ and TGA to PNG. It follows
  each model's Unity normal import settings and writes meshes in Unity's
  left-handed space, which is Calyx's.
- `json.rs` writes the generated files with ordered keys and shortest
  round-trip floats, so a regenerated scene diffs cleanly.
- `ids.rs`: asset and object ids are version 5 UUIDs derived from the pack's
  namespace, so re-imports keep them.

## Packs

Each pack's own choices (scene, output folder, lighting, post-processing with
fog from the Unity scene's `RenderSettings`, camera path) live in `packs/`.
`packs/rpg_poly.rs` builds the demos project's RPG Poly village (see
`demos/THIRD_PARTY_ASSETS.md`).
