# Shaders

Engine shaders are WGSL assets under `assets/shaders/`, preprocessed by
`calyx_shader` and reflected through naga. `render::Shader` is the asset;
`AssetShaderLoader` feeds the preprocessor from the asset roots.

Code: `crates/calyx_engine/src/render/shader.rs`,
`crates/calyx_engine/src/render/shader_loader.rs`, `crates/calyx_shader/`
(its crate rustdoc documents the directive language).

## Loading

- `AssetShaderLoader` implements `calyx_shader::ShaderLoader` over the asset
  roots. A shader naga rejects reports the original file and line through the
  preprocessor's source map.
- An include names a file from an asset root
  (`//#include "shaders/common/camera.wgsl"`) and a load names the asset
  without its extension (`"shaders/mesh/pbr"`).
- Materials, scenes and prefabs reference a shader by the UUID in its `.meta`,
  so move a shader with its `.meta` and its references keep working.

## Compute

- `Shader::compute_pipeline(entry_point)` builds and caches one compute
  pipeline per entry point, laid out with only the bindings that entry point
  uses.
- A shader with several compute entry points keeps no whole-module layout, so
  its kernels together may declare more resources than one kernel may use.
- `assets/shaders/common/curve.wgsl` and `assets/shaders/common/gradient.wgsl`
  evaluate `Curve`s and `Gradient`s on the GPU as `Curve::evaluate` and
  `Gradient::evaluate` do, reading keys through their `curve_storage` and
  `gradient_storage` parameters, which `GpuCurveKeys` and `GpuGradientKeys`
  pack.

## Hot reload

A shader reloads when its file or an included file changes
(`Asset::accept_reload`).

- A reload that changes a shader's bind-group layout gets a new
  `Shader::layout_generation`, and a cache keyed on it, or on the live layout,
  rebuilds its bind group before the next draw.
- A renderer that builds a group through `Shader::create_bind_group`, which
  binds resources by the variable names the shader declares, follows a
  renumbered, reordered, dropped or restored binding; a binding under a name
  it does not bind is refused, naming it.
- A material follows any change to the groups it fills (3 and later, recorded
  on the shader when a material is built on it) that it can reflect: a
  uniform, a 2D texture or a filtering sampler (`Material::follow_shader`).
- A pass that binds another shader's materials checks
  `Shader::shares_material_group` and skips them until the two match.
- Groups bound by fixed binding numbers keep refusing any binding change until
  the editor restarts.

## Layout

Every engine shader sits in a folder of `assets/shaders/` for its purpose;
none sits at the top level.

- `common/`: include modules with no entry point that shaders of several
  folders share: camera, constants, inputs and lights uniforms, shadows, the
  PBR helpers and lighting, `curve.wgsl` and `gradient.wgsl`, the cubemap face
  directions, and the full-screen triangle (`fullscreen.wgsl`) with its
  `vs_main` (`fullscreen_pass.wgsl`).
- `mesh/`: scene geometry: `mesh.wgsl` (the mesh instance and skinning
  module), the PBR scene shader and its G-buffer pass, the grass, ground and
  water shaders, the shadow pass and the object-id pick pass.
- `deferred/`: the G-buffer encoding, its debug view and the deferred lighting
  pass.
- `environment/`: skybox drawing and image-based lighting: the
  equirectangular-to-cubemap, irradiance, prefilter and BRDF kernels and the
  skybox.
- `particles/`: the particle renderer and the particle simulation kernels.
- `postprocess/`: post-processing passes and the modules only they share
  (`bloom_filters.wgsl`, `dof_common.wgsl`, `ssao_common.wgsl`,
  `camera_planes.wgsl`), plus the multisampled depth resolve the chain runs
  first.
- `editor/`: editor overlays and tooling the game does not draw: gizmos and
  gizmo icons with their object-id pass, the grid, the selection outline, the
  overlay composite, and the thumbnail generator's shaders.
- `utility/`: GPU helpers with no part in a frame's scene: the 2D and cube mip
  generators.

A new shader goes in the folder of the pass or feature it belongs to. An
include module goes next to the shaders that use it when they share one
folder, as `postprocess/bloom_filters.wgsl` does, and in `common/` once
shaders of two folders use it. `render::shader_loader`'s
`every_engine_shader_resolves_its_includes_and_preprocesses` test fails when a
shader sits at the top level or an include path is stale.
