# Draw Collection and Static Batches

What a scene draws does not depend on the camera, so a `DrawCollection`
collects the meshes, foliage, terrain, water and skinned meshes of one scene
once a frame, with their resolved assets, object ids and material buffers, and
every view of that scene draws from it. Static mesh objects are merged into
static batches inside the collection.

Code: `crates/calyx_engine/src/render/draw_collection.rs`,
`crates/calyx_engine/src/render/static_batch.rs`,
`crates/calyx_engine/src/render/scene_renderer.rs`.

## Scene renderer

`SceneRenderer` draws mesh, skybox, grid, gizmos and particles. Opaque PBR
meshes shade only through the deferred path: a geometry pass into the
G-buffer, then a full-screen lighting pass through
`assets/shaders/common/pbr_lighting.wgsl`. Draws with any other shader take a
forward pass after it, against its depth; see [G-Buffer](gbuffer.md).

## Sharing a collection

- `SceneRenderer::render_scene_with` draws a shared collection. The editor
  collects once a frame for its Scene and Game views.
- `render_scene_base` collects into the renderer's own collection.
- A collection of another scene is not drawn: the renderer collects its own
  instead.
- Each view keeps its sky, its gizmos and the batching of the draws into its
  instance lists.

## Static batches

The collection keeps the scene's static mesh objects merged into one
world-space mesh per material, shadow casting and 32 m chunk of the XZ plane,
so the arena draws about a hundred batches instead of over a thousand
instances.

- **Who batches.** A mesh object batches when it is visible, its mesh is
  enabled, its material uses `shaders/mesh/pbr`, its mesh has no bones and no
  fourth UV channel, and neither it nor an ancestor has a non-fixed rigid
  body. The rest draw as instances.
- **Following the scene.** The collection follows the scene's
  `ChangeJournal` (every component write, reparent and departure in order,
  read from a cursor) instead of walking every object each frame. An object
  whose transform, visibility, mesh component or enabled state changes how it
  draws, or that is deleted, leaves its batch, which merges again without it,
  and draws on its own from then on. A registry change re-resolves the
  members' assets and merges again a batch whose geometry was reloaded.
- **Object ids.** Each merged vertex carries its object's id in `uv3.x` for
  the object id pass (the batch's instance has `object_id` `u32::MAX`), so
  picking and the selection outline see each object. Object ids therefore
  stay fixed while the collection draws the same scene.
- **Culling.** The camera passes skip a batch whose box lies outside the view
  frustum; shadow maps draw them all, culled per cascade as
  [Shadow Mapping](shadows.md#the-shadow-pass) describes.
- The player's renderer turns the object id pass off
  (`SceneRendererOptions::object_ids`).
