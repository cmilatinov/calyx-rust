# G-Buffer

A geometry pass draws the scene's opaque PBR meshes into a G-buffer owned by
`SceneRenderer`, and a debug view shows one of its targets in place of the lit
image. The deferred lighting pass shades every opaque PBR mesh from this
G-buffer; the pass order is in
[Post-Processing Architecture](../post-processing/architecture.md#deferred-shading).

Code: `crates/calyx_engine/src/render/gbuffer.rs` (`GBufferTargets`,
`GBufferRenderer`, `GBufferView`), `assets/shaders/mesh/gbuffer.wgsl` (the
geometry pass), `assets/shaders/deferred/gbuffer_encoding.wgsl` (the normal
encoding) and `assets/shaders/deferred/gbuffer_view.wgsl` (the debug view).

## Layout

Four color targets, single-sample at the scene's size, plus depth.

| Target | Format | R | G | B | A |
| --- | --- | --- | --- | --- | --- |
| `albedo` | `Rgba8UnormSrgb` | base color R | base color G | base color B | 1 where a surface was drawn, 0 elsewhere |
| `normals` | `Rgba16Float` | shading normal, octahedral X | shading normal, octahedral Y | geometric normal, octahedral X | geometric normal, octahedral Y |
| `material` | `Rgba8Unorm` | metallic | roughness | ambient occlusion | unused (1) |
| `emissive` | `Rgba16Float` | emission R | emission G | emission B | unused (1) |
| depth | `Depth32Float` | the scene's depth at one sample (see MSAA) | | | |

Every target clears to 0 each frame, so a pixel no PBR mesh covers (sky,
particles, meshes drawn with another shader) holds albedo alpha 0.

Which material input fills each channel, as `shaders/mesh/pbr_surface.wgsl`
reads it for both the forward and the geometry pass:

- **Albedo**: the base color texture (sRGB view) times `base_color_factor`,
  linear in the shader. The sRGB format stores it with more precision in the
  darks; a pass that samples or loads the target gets linear values back.
  Base color alpha is not stored: a fragment below `alpha_cutoff` is
  discarded, and every fragment kept is opaque.
- **Shading normal**: the vertex normal bent by the normal texture in the
  tangent frame of the vertex tangent, with X and Y scaled by `normal_scale`.
- **Geometric normal**: the interpolated vertex normal. The lighting pass
  offsets shadow lookups along it (see [Shadow Mapping](shadows.md)), so it
  is stored beside the shading normal.
- **Metallic**: `metallic` times the metallic-roughness texture's B.
- **Roughness**: `roughness` times the metallic-roughness texture's G.
- **Ambient occlusion**: `ambient_occlusion * (1 + occlusion_strength *
  (occlusion texture R - 1))`. The lighting pass applies it to the ambient
  and image-based light only.
- **Emission**: `emission_color * emission_strength * emissive texture`
  (sRGB view), linear HDR radiance.

Both normals are in view space, `camera.view` applied to the world-space
normal (the camera looks down +Z of its view space). A world normal is
`inverse_view` applied to it, which the shadow lookups need.

### Normal encoding

A normal is stored as its octahedral encoding (`gbuffer_encode_normal` and
`gbuffer_decode_normal` in `shaders/deferred/gbuffer_encoding.wgsl`): the unit
vector divided by `|x| + |y| + |z|`, which lands it on the octahedron, with
the lower half (z < 0) folded over the upper half. That leaves two values in
`[-1, 1]`. In a 16-bit float channel the round trip stays within about 0.1
degree, well inside a shadow texel for the normal-offset bias, and two
encodings fit in one `Rgba16Float` texel: the shading and the geometric
normal cost one target.

### Emission

Emission has its own float target, which the deferred lighting pass adds
into its output, and the debug view can show on its own.

## The geometry pass

`GBufferRenderer::render` runs first in `SceneRenderer::render_scene_base`,
after the shadow pass, on the same command encoder and draw list as the
forward pass.

- It draws every batch of the shared `DrawList` whose shader is
  `shaders/mesh/pbr`; a draw with any other shader takes the forward pass
  after lighting. The
  draw list is built once from the enabled meshes, so both passes skip the
  same disabled meshes, and with the renderer's `mesh_cull_mode`.
- Its vertex stage is `shaders/mesh/pbr_vertex.wgsl`, the one `pbr.wgsl`
  uses, so skinned meshes, the rest pose included, write the same skinned
  positions, normals and tangents (`skinned_local` of `shaders/mesh/mesh.wgsl`).
  The clip position is `@invariant`, so both pipelines compute the same
  depth.
- Group 0 is the scene camera, group 1 the mesh's instances and bones, and
  group 2 the material's own bind group, built for group 3 of `pbr.wgsl`, as
  the masked shadow pass binds it. It reads the material through the same
  `pbr_surface` that `pbr.wgsl` declares, and discards the fragments below
  the material's `alpha_cutoff`.
- It owns the depth: it clears it and tests `Less` in the draw order, so of
  fragments that tie in depth, such as two coplanar meshes, it keeps the
  first drawn. At one sample that depth is the scene depth; above one it is
  its own, which the lighting pass copies into the scene depth.

## MSAA

The G-buffer is single-sample and is never resolved: averaging normals or
material values across samples gives values no surface has, and a
multisampled G-buffer multiplies its memory by the sample count.

- PBR surfaces shade at one sample. Views replace MSAA with the FXAA pass
  the post-processing chain already has (Anti-Aliasing component on the
  volume). The editor scene view, the Game tab and thumbnails render at one
  sample. Above one sample the lighting pass writes each pixel into all of
  its samples, so PBR edges are not multisampled; forward draws, the skybox
  and particles still are.
- **Above one sample** (tests render at four) the scene depth is
  multisampled and cannot be attached beside single-sample targets. The
  geometry pass then clears and writes its own single-sample `Depth32Float`
  depth, allocated with the targets, and the lighting pass writes that depth
  into the scene depth. Meshes drawn with other shaders are drawn after the
  geometry pass, so they never occlude a surface in the G-buffer; the debug
  view shows the PBR surface behind them.

## Debug view

`SceneRendererOptions::gbuffer_view` selects a `GBufferView` (albedo,
normals, material or emissive) or `None` for the lit image.

- With a view selected, `finalize_scene` draws that target into the scene
  texture in place of the post-processing chain, and the editor overlays
  composite over it as usual. Albedo and emission are colors and show as
  such, emission through Reinhard (`x / (1 + x)`). Normals (each axis mapped
  from `[-1, 1]` to `[0, 1]`) and the material are data, written so the
  displayed byte is the stored value: 0.5 shows as 128. A pixel with albedo
  alpha 0 shows black.
- The view shows the G-buffer the frame already drew and shaded from; it
  adds no pass. The targets are created on the first frame and resized with
  the other scene textures.
- The editor's scene view has a view mode menu in its action bar (Lit,
  Albedo, Normals, Material, Emissive). `calyx_ctl view <lit|albedo|normals|material|emissive>`
  sets it remotely (`set_scene_view`).

## Deferred lighting

The lighting pass (`shaders/deferred/lighting_common.wgsl`) rebuilds the
world position from the depth with `inverse_projection` and `inverse_view`,
decodes both normals and brings them to world space with `inverse_view`. It
shades with the shading normal through `pbr_lighting`
(`shaders/common/pbr_lighting.wgsl`), which offsets shadow lookups along the
geometric normal and applies
ambient occlusion to the ambient and image-based terms only. It discards
pixels whose albedo alpha is 0: the forward pass or the skybox owns them.

## Testing

Headless tests in `crates/calyx_engine/src/render/scene_renderer.rs`:

- A plane turned 30 degrees writes its albedo, both normals, metallic,
  roughness, occlusion and emission at one and four samples, and nothing
  where it is not drawn.
- A normal map tilts the shading normal by the expected angle and leaves the
  geometric normal.
- A box discarded by its alpha cutoff leaves the box behind it in the
  G-buffer.
- Of two coplanar boxes, the G-buffer holds the one the lit image shows, at
  one and four samples.
- A skinned mesh in its rest pose writes its posed normal.
- Every frame draws the G-buffer, a view shows that same G-buffer, and the
  targets resize with the scene.
- The lit pixels match known radiances; see
  [Post-Processing Architecture](../post-processing/architecture.md#testing).
- Each debug view shows its target's expected bytes in the scene texture, and
  black where nothing is drawn.
