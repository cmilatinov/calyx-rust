# Shadow Mapping

Directional and spot lights cast shadows. A depth-only shadow pass renders the
scene's shadow-casting meshes from each shadowed light before the scene pass,
and the lighting samples those maps with PCF filtering and bias. Point light
shadows (cube maps), contact shadows and soft (PCSS) shadows are not
implemented.

Code: `crates/calyx_engine/src/render/shadows.rs` (`ShadowPlan`,
`ShadowRenderer`), `crates/calyx_engine/src/render/light_manager.rs`,
`assets/shaders/mesh/shadow.wgsl` and `assets/shaders/mesh/shadow_masked.wgsl`
(the shadow pass) and
`assets/shaders/common/shadows.wgsl` (the lookups).

## Authoring

| Component | Field | Default | Meaning |
| --- | --- | --- | --- |
| `ComponentDirectionalLight`, `ComponentSpotLight` | `cast_shadows` | `false` | The light asks for a shadow. Off by default, so scenes saved before shadows existed load and render unchanged. |
| | `shadow_distance` | 50 | Directional: the view depth from the camera the cascades cover; past it the light is unoccluded. Spot: the distance from the camera within which the light keeps its shadow. |
| | `depth_bias` | 1 | Scales the shadow pass's constant (1000 depth units) and slope-scaled (2) `DepthBiasState`. |
| | `normal_bias` | 1 | The normal offset of a lookup, in shadow texels. |
| `ComponentMesh`, `ComponentSkinnedMesh` | `cast_shadows` | `true` | The mesh draws into shadow maps. It is still lit and shadowed either way. |

## Which lights get a shadow

`LightManager::build_data` gathers the enabled lights with the scene's
enabled-state checks, groups them by kind, and hands the shadow casters to
`ShadowPlan::new` with the renderer's camera. A disabled light is never
gathered, so it holds no shadow slot; disabling the shadowed sun hands the
cascades to the next shadowed directional light.

- The first shadowed directional light, in scene order, gets the cascades and
  `shadow_index` 0. Other directional lights light without shadows.
- Shadowed spot lights within their `shadow_distance` of the camera are sorted
  by distance to it, and the nearest 8 (`MAX_SPOT_SHADOWS`) get atlas tiles 0
  to 7, which are also their `shadow_index`. The rest light without shadows.
- Every other light, point lights included, has `shadow_index` -1.

The plan is rebuilt every frame, so a tile follows its light's rank, not the
light.

## Cascades

The directional light's shadow is 4 cascades, the 4 layers of one
`Depth32Float` 2D array texture of 2048 x 2048 per layer.

- **Splits.** The camera's view depth from its near plane to
  `min(shadow_distance, far plane)` splits by the practical scheme: each split
  is 0.75 of the logarithmic split plus 0.25 of the even split.
- **Fit.** Each cascade is an orthographic projection fitted to the bounding
  sphere of its frustum slice, which starts where the previous cascade begins
  blending into it (see Sampling), so the blend band reads both maps. The
  sphere's center is the slice corners'
  centroid and its radius the farthest corner, rounded up to 1/16 world unit.
  The slice's shape is fixed relative to the camera, so the radius, and with it
  the texel size, does not change as the camera turns or moves.
- **Snapping.** The light's view is a rotation about the world origin. The
  sphere's center, in that space, snaps down to a whole shadow texel, so a
  moving camera moves the map by whole texels and shadow edges do not shimmer.
  The map spans the sphere plus one texel each side, which the snap can take
  up.
- **Depth range.** Depth spans the sphere plus `shadow_distance` towards the
  light, so casters outside the slice, between it and the light, still draw.

## Spot atlas

Spot shadows share one `Depth32Float` atlas of 4096 x 4096, cut into 1024 x
1024 tiles filled row by row; 8 lights use the top two rows. Each light renders
a perspective projection from its position along its axis: a square field of
view of twice the outer cone angle plus 2 degrees each side (at most 170
degrees), so filtering at the cone's edge reads inside the tile, from a near
plane of 1% of `range` (clamped to 0.02 to 0.5) to `range`.

## The shadow pass

`ShadowRenderer::render` runs in `SceneRenderer::render_scene_base` after the
lights are gathered and the instance buffers are uploaded, before the scene
pass, on the same command encoder.

- The pipeline comes from `assets/shaders/mesh/shadow.wgsl`, depth only, with
  no color target and an empty fragment stage. It reads the mesh vertex
  inputs, the instance buffer and the bone palette of group 1 exactly as
  `pbr.wgsl` does, and poses a skinned vertex with the same `skinned_local`
  of `assets/shaders/mesh/mesh.wgsl`, so a skinned mesh casts its posed
  shadow. Group 0 is the `CameraUniforms` of
  `assets/shaders/common/camera.wgsl`, one per cascade or tile in its own small
  buffer: the light's view and projection with their inverses, the
  projection's near and far planes in light view space, and the map's size
  (2048 for a cascade, 1024 for a spot tile) as the viewport.
- It is single-sample, whatever the scene's MSAA, and draws both faces (no
  culling), so open meshes such as planes and cards cast too. The light's
  `DepthBiasState` is part of the pipeline options, so each distinct bias
  builds and caches one pipeline.
- It draws from the scene pass's own draw list (`DrawBatch`), so a mesh the
  scene skips, disabled or hidden, is never drawn into a shadow either. Batches
  split on `cast_shadows` as well as shader, material and mesh, and the pass
  skips the batches that do not cast.
- A batch whose material is a PBR material with an `alpha_cutoff` above 0
  draws with `assets/shaders/mesh/shadow_masked.wgsl` instead. It binds the
  material's own bind group, built for group 3 of `pbr.wgsl`, as its group 2
  (it declares every binding as `pbr.wgsl` does, and both share
  `shaders/mesh/pbr_material.wgsl`). It discards a fragment whose base color
  alpha is below the cutoff, as the scene pass does, so cutout surfaces cast
  cutout shadows. Every other batch keeps the empty fragment stage.
- Each cascade is its own pass on its layer. The atlas is one pass, cleared
  once, with the viewport and scissor set to each light's tile.

## Sampling

`assets/shaders/common/shadows.wgsl` includes
`assets/shaders/common/camera.wgsl`, the scene camera in group 0, and binds, in
group 2 beside the light buffer at binding 0:

| Binding | Resource |
| --- | --- |
| 1 | `ShadowData` uniform: the cascades' matrices, far splits and texel sizes; the spot shadows' matrices, tile offsets and texel scales; the cascade blend fraction; the normal biases; the atlas and cascade texel sizes |
| 2 | `texture_depth_2d_array`, the cascades |
| 3 | `texture_depth_2d`, the atlas |
| 4 | `sampler_comparison`, `LessEqual` with linear filtering |

`directional_shadow(light, world_position, normal)` and
`spot_shadow(light, world_position, normal)` return how lit the point is, 1
lit and 0 in shadow, and 1 for a light whose `shadow_index` is -1.

- **Cascade selection.** The point's view depth, the Z of
  `camera.view * world_position` (the camera looks down +Z of its view space),
  picks the first cascade whose far split is past it. Over the last 10% of
  a cascade's depth range the result blends into the next cascade, and the
  last cascade fades out to lit, so neither the cascade seams nor the shadow's
  end show a line.
- **Filtering.** PCF over a 3 x 3 grid of comparison taps one texel apart.
  Each tap is a hardware 2 x 2 bilinear comparison, so the kernel is a 4 x 4
  texel tent. In the atlas every tap is clamped inside the light's tile.
- **Bias.** The shadow pass's constant and slope-scaled depth bias, plus a
  normal offset: the point moves `normal_bias` shadow texels along its
  geometric normal before the lookup. A cascade's texel is its world size; a
  spot light's grows with distance from the light. A point outside a map is
  lit.

## Lighting pass

The lookups need only the world position and the geometric normal of the
shaded point, and the scene camera's `CameraUniforms` in group 0, which every
lighting pass binds, so the deferred lighting pass includes
`shaders/common/shadows.wgsl` unchanged.

- The deferred lighting pass shades every PBR mesh through `pbr_lighting` of
  `shaders/common/pbr_lighting.wgsl`, which multiplies each directional and
  spot light's direct term by its shadow factor, looked up with the world
  position rebuilt from the depth and the G-buffer's geometric normal.
  Ambient light and image-based lighting are never shadowed.

## Budget

`SceneRendererOptions::shadows` switches shadows per renderer. The editor's
scene view and game view turn it on; the thumbnail renderer and the default
options leave it off, so they gather no shadow casters, run no shadow pass and
allocate no shadow map.

Each map is allocated only while a light shadows into it, and released the
first frame none does: 64 MB for the 4 cascades (4 x 2048 x 2048 x 4 bytes)
and 64 MB for the atlas (4096 x 4096 x 4 bytes), per renderer. Without them a
1 x 1 stand-in is bound, which nothing samples.

The pass costs one draw of every shadow-casting batch per cascade and per
shadowed spot light. The pull request that added shadows (#444) records the
frame time of the showcase scene with shadows on and off.
