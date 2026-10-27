# Post-Processing Architecture

Related work:

- Epic: [Post-processing (#200)](https://github.com/cmilatinov/calyx-rust/issues/200)
- This design: [#112](https://github.com/cmilatinov/calyx-rust/issues/112)
- Follow-ups: effects (#113), local volumes (#114), editor profile UI (#115), overlay compositing (#379), showcase scene (#378)

## Goals

- Lit shaders write linear HDR radiance. Tone mapping is the single display transform, applied once, after the scene is drawn.
- An ordered chain of full-screen passes runs between the MSAA resolve and the presented scene texture, so effects can be added without touching the forward pass.
- A frame with only tone mapping costs one full-screen pass and allocates no intermediate targets.
- A scene configures the chain with a reflected, serialized component the inspector edits with no custom UI. A scene without one still gets tone mapping.
- Every view of a scene goes through the same chain: editor viewport, Game tab, player, prefab mode and asset thumbnails.

## Frame Today

`SceneRenderer` (`engine/src/render/scene_renderer.rs`) renders a frame in two calls.

```text
render_scene_base:  meshes (pbr.wgsl)   albedo * 5, Reinhard, gamma 1.4
                    skybox              exposure 0.5, gamma 0.9
                    grid, particles, gizmos   no tone mapping
                    -> scene_texture_msaa (Rgba16Float)
finalize_scene:     outline -> resolve MSAA -> scene_texture -> egui, readback
```

Each shader applies its own curve, so the scene texture mixes color spaces and no full-screen effect can be added after the forward pass. `render/postprocess/` holds only commented-out stubs.

## Frame After

```text
render_scene_base:  every lit shader writes linear HDR -> scene_texture_msaa
                    resolves PostProcessSettings from the scene and keeps them
finalize_scene:     outline
                    resolve MSAA -> hdr_resolved (renderer-owned)
                    PostProcessChain:
                      Hdr passes      ping-pong A/B pool targets  (#113)
                      Tonemap pass    always runs
                      Ldr passes
                    the last pass writes scene_texture (format and size unchanged)
```

`finalize_scene` does not take the scene: `render_scene_base` resolves the settings and stores them on the renderer, so the editor's split between the two calls stays as it is.

With a single sample the multisampled target is already single-sample, so it is the chain's input and nothing is resolved or copied. With only tone mapping enabled the chain reads `hdr_resolved` (or that single-sample target) and writes `scene_texture` in one full-screen pass, with no pool target.

## Display Encoding

`scene_texture` holds **display-linear** values: tone-mapped linear color in `[0, 1]`, with no gamma applied. Everything that reads it already assumes so:

- egui-wgpu 0.31 samples a native texture as linear and sRGB-encodes the result itself: both of its fragment entry points (`fs_main_gamma_framebuffer`, and `fs_main_linear_framebuffer` for an sRGB target) run `gamma_from_linear_rgba` on the sampled texel.
- `TextureReadback`, behind screenshots, `calyx_ctl screenshot --target viewport|game` and thumbnails, sRGB-encodes the color channels and keeps alpha linear.

So the tone-map pass outputs the curve's result unchanged. The old `pbr.wgsl` raised its Reinhard output to `1 / 1.4` before egui encoded it again, which is why the old look was lifted and washed out. The headless test `tonemap_output_is_display_linear` pins this: a known HDR value goes through the tone-map pass, the raw `Rgba16Float` texel must equal the curve's linear output, and the readback byte must equal its sRGB encoding.

Textures are still uploaded as `Rgba8Unorm`, so albedo maps are sampled without sRGB decoding. That is a separate color-space issue, outside this design.

## Pass Interface

`engine/src/render/postprocess/` holds the chain.

```rust
pub enum PostProcessStage { Hdr, Tonemap, Ldr }

pub struct PassInputs { pub depth: bool, pub normals: bool, pub velocity: bool }

pub trait PostProcessPass {
    fn name(&self) -> &str;
    fn stage(&self) -> PostProcessStage;
    fn inputs(&self) -> PassInputs;                          // default: none
    fn enabled(&self, settings: &PostProcessSettings) -> bool;
    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings);
    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo);
}

pub struct PassIo<'a> {
    pub input: &'a Texture,          // never the output
    pub output: &'a Texture,
    pub depth: Option<&'a Texture>,  // when requested
}
```

`prepare` uploads the pass's uniforms from the settings. `record` draws from `input` into `output`. A pass reads its input with `textureLoad` or a sampler and writes every output pixel, so it never needs to load the output.

## Chain And Target Pool

`PostProcessChain` owns the passes and a `TargetPool`.

- Passes run in stage order (`Hdr`, `Tonemap`, `Ldr`), then insertion order within a stage.
- Each frame the chain keeps the passes whose `enabled` returns true and whose inputs are available, then wires them: the first reads the chain's source, the last writes the chain's output, and each pass between writes the pool target the next one reads. Intermediates alternate between two pool targets (A, B), so a pass never reads the texture it writes.
- The pool is fetched from only when two or more passes run. The tone-map-only chain never touches it.
- `TargetPool` keys targets by size, format and usage, reuses them across frames and frees them all when the renderer resizes.

The wiring is a pure plan (`ChainPlan`) built before any GPU work, which keeps ordering and ping-pong testable without a device.

## Shared Inputs

A pass declares the scene inputs it reads in `inputs()`. The chain builds each one only when an enabled pass asks for it.

- **Depth**: without MSAA the scene's depth texture is passed through. With MSAA a small pass copies sample 0 of the multisampled depth into a single-sample `Depth32Float` texture, built once per frame for every pass that asks.
- **Normals, velocity**: not produced yet. A pass that asks for one is skipped with a `PostProcessError::InputUnavailable` warning naming the pass and input.

Tone mapping asks for nothing.

## Tone Mapping

`TonemapPass` is the `Tonemap` stage. It always runs: a missing or disabled volume selects `PostProcessSettings::default()`, never untone-mapped output.

- Its uniform holds the exposure scale `2^EV` and the preset.
- It draws one full-screen triangle (`shaders/postprocess/fullscreen.wgsl`, no vertex buffers).
- `TonemappingPreset::Aces` is Stephen Hill's fit of the ACES RRT and ODT (BakingLab, MIT): the ACES input matrix, the rational RRT/ODT curve, then the output matrix, clamped to `[0, 1]`. It maps linear 0.18 to about 0.106 and 1.0 to about 0.619.
- Alpha passes through unchanged, so views cleared to transparent, such as thumbnails, keep their transparency.

## Settings And Volumes

```rust
pub struct PostProcessSettings { pub tonemapping: TonemapSettings }
pub struct TonemapSettings { pub preset: TonemappingPreset, pub exposure_ev: f32 }  // Aces, 0.0
pub enum TonemappingPreset { Aces }

pub struct ComponentGlobalPostProcessVolume {
    pub enabled: bool,
    pub priority: i32,
    pub settings: PostProcessSettings,
}
```

- The component is reflected and serialized like the other built-in components, so the inspector edits it with its default field inspectors.
- Every settings struct is `#[serde(default)]`. Effects added later become optional sub-structs of `PostProcessSettings`, so saved scenes stay compatible.
- **Selection**: the enabled volume with the highest priority. On a priority tie the renderer warns once and uses the first found. With no enabled volume it uses `PostProcessSettings::default()`.
- Prefab mode and thumbnails render scenes with no volume, so they get the defaults with no special case.
- #114's local volumes blend `PostProcessSettings` values before the chain runs; the chain only ever sees one resolved value.

## Shaders

- `assets/shaders/postprocess/fullscreen.wgsl`: shared full-screen-triangle vertex module.
- `assets/shaders/postprocess/tonemap.wgsl`: the tone-map pass.
- `assets/shaders/postprocess/depth_resolve.wgsl`: copies sample 0 of the multisampled depth.
- `pbr.wgsl` drops Reinhard, gamma 1.4 and the `* 5.0` on albedo.
- `skybox.wgsl` drops its exposure and gamma and scales the sky by the sky light's intensity, the same factor that scales its image-based lighting.
- Particles already write linear color.

Post shaders use `//#include` modules, which carry over unchanged to the `calyx_shader` crate.

## Callers

The editor viewport, the Game tab, the thumbnail generator and the standalone player all call `render_scene_base` then `finalize_scene`, so each gets the chain with no change. The viewport and the Game tab render the same scene, so they select the same settings.

## Calibration

Removing the per-shader curves and the `* 5.0` changes every scene's brightness. Scenes are recalibrated through data, never a shader constant: a global volume's exposure and the scene's light intensities. The sandbox scene carries a `ComponentGlobalPostProcessVolume` for this.

The grid, gizmos, outline and editor clear colors stay in the HDR scene pass until #379 composites them after post, so tone mapping applies to them in the meantime and they are retuned by eye.

## Testing

Headless tests in `engine/src/render/postprocess/`:

- The plan orders passes by stage, then insertion order, and ping-pongs two pool targets.
- A tone-map-only chain allocates no pool target, and a chain of three passes allocates two.
- Two non-commuting test passes produce the result of their declared order.
- Tone mapping preserves alpha.
- ACES maps known HDR inputs to the reference fit within tolerance.
- The display-encoding test above.
- Depth reaches a pass that requests it, single-sample at one and four samples; normals and velocity report unavailable.
- Volume selection picks the highest enabled priority, warns on a tie, and falls back to the defaults.

## Roadmap

- **#379** moves the grid, gizmos and outline to an overlay composited after post, which also lets editor clear colors skip tone mapping.
- **#113** adds effects as `Hdr` and `Ldr` passes, each with an optional settings sub-struct.
- **#114** adds local volumes that blend `PostProcessSettings` before the chain runs.
- **#115** adds editor UI for post-processing profiles.
- **#378** adds a showcase scene that validates the look end to end.
