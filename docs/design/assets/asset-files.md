# Asset Files

An asset root holds files the `AssetRegistry` loads as assets, each beside a
`.meta` that owns its id. The registry watches the roots with `notify` and
reloads an asset when its file changes; `calyx_shader` dependencies decide
which shaders an included file reloads.

Code: `crates/calyx_engine/src/assets/asset_registry.rs` and
`crates/calyx_engine/src/assets/asset_registry/meta_sync.rs`.

## File types

| Extension | Asset |
| --- | --- |
| `.wgsl` | Shaders (`render::Shader`) |
| `.png`, `.jpg`, `.jpeg`, `.webp` | Textures |
| `.obj` | Meshes |
| `.fbx`, `.gltf`, `.glb` | Models, imported as prefabs; see [Model Import](model-import.md) |
| `.cxprefab` | Prefabs |
| `.cxscene` | Scenes (JSON-serialized `SceneData`) |
| `.hdr`, `.exr` | Skyboxes |
| `.cxanimclip` | Animation clips |
| `.cxanim` | Animation graphs |
| `.cxmat` | Materials |
| `.cxpostfx` | Post-processing profiles, shared between post-process volumes |
| `.cxinput` | Action maps; a project's bindings are `input.cxinput` |
| `.meta` | Asset metadata (UUID, name, type) |

`project.toml` is the project configuration, read by `calyx_project` and
`runtime::ProjectRuntimeSettings`; it is not an asset.

## `.meta` files

- Only a file whose extension is a registered asset type gets one; other files
  in an asset root are listed in the content browser but are not assets.
- A `.meta` on disk owns its asset's id: the editor never rewrites the id of
  one that exists.
- A file that appears while the editor runs waits half a second for its
  `.meta` before the editor generates one.
- A `.meta` another tool writes or changes later re-keys the asset under its
  id, with the old id resolving to the new one until the editor closes.

So tools can write an asset and its `.meta` in either order with the editor
open. Materials, scenes and prefabs reference an asset by the UUID in its
`.meta`, so a file moved with its `.meta` keeps its references.
