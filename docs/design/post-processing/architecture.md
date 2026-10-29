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
render_scene_base:  every lit shader and particles write linear HDR -> scene_texture_msaa
                    collects gizmos, resolves PostProcessSettings from the scene and keeps them
finalize_scene:     resolve MSAA -> hdr_resolved (renderer-owned)
                    PostProcessChain:
                      Hdr passes      ping-pong A/B pool targets  (#113)
                      Tonemap pass    always runs
                      Ldr passes
                    the last pass writes scene_texture (format and size unchanged)
                    overlay pass: grid, gizmos, outline -> overlay target (MSAA), resolved
                    composite the overlay target over scene_texture
```

`finalize_scene` does not take the scene: `render_scene_base` resolves the settings and stores them on the renderer, so the editor's split between the two calls stays as it is.

With a single sample the multisampled target is already single-sample, so it is the chain's input and nothing is resolved or copied. With only tone mapping enabled the chain reads `hdr_resolved` (or that single-sample target) and writes `scene_texture` in one full-screen pass, with no pool target.

## Editor Overlays

The grid, gizmos and the selection outline draw after the whole chain, Ldr stage included, so tone mapping, exposure and later effects never change their colors. `engine/src/render/overlay/` holds them.

```rust
pub trait OverlayPass {
    fn name(&self) -> &str;
    fn enabled(&self, context: &OverlayContext) -> bool;
    fn prepare(&mut self, context: &OverlayContext);
    fn record(&mut self, context: &OverlayContext, render_pass: &mut wgpu::RenderPass<'_>);
}
```

- `GridRenderer`, `GizmoRenderer` and `OutlineRenderer` implement it. The renderer hands them to `OverlayCompositor::run` as a plain list in that fixed order, with no stages.
- Every enabled overlay records into one shared render pass over the overlay target, cleared to transparent, and the scene's still-multisampled depth, both at the scene's sample count. Overlay edges and overlay-against-geometry intersections keep their MSAA anti-aliasing. The same pass resolves the target.
- The overlay target is `Rgba16Float`, display-linear like `scene_texture`. It exists only while an overlay is enabled: the editor viewport shows the grid and gizmos, and the outline is enabled while an object is hovered or selected. The Game tab, the player and thumbnails show none, so they allocate nothing for it.
- Overlays alpha blend (`RenderUtils::color_alpha_blending`) into the transparent target, which leaves premultiplied color with coverage alpha. The composite therefore blends the resolved target over `scene_texture` as premultiplied alpha, `rgb + dst * (1 - a)` (`shaders/overlay_composite.wgsl`). Blending it as `rgb * a + dst * (1 - a)` would multiply by alpha twice and darken anti-aliased edges.
- Particles stay in the scene pass and are tone mapped with it.

`OverlayPass` mirrors `PostProcessPass` but stays separate. Post-process passes take the encoder because some record several render passes, such as bloom's downsample and upsample chain, while overlays all draw into one shared multisampled pass. The shared code lives in the full-screen helpers below instead.

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

#[bitflags]
#[repr(u8)]
pub enum SceneInput { Depth = 0b001, Normals = 0b010, Velocity = 0b100 }
pub type PassInputs = BitFlags<SceneInput>;                  // enumflags2

pub trait PostProcessPass {
    fn name(&self) -> &str;
    fn stage(&self) -> PostProcessStage;
    fn inputs(&self) -> PassInputs;                          // default: none
    fn enabled(&self, settings: &PostProcessSettings) -> bool;
    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings);
    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo);
    fn release_targets(&mut self) {}                         // default: owns none
}

pub struct PassIo<'a> {
    pub input: &'a Texture,          // never the output
    pub output: &'a Texture,
    pub depth: Option<&'a Texture>,  // when requested
}
```

`prepare` uploads the pass's uniforms from the settings. `record` draws from `input` into `output`. A pass that owns textures besides the pool targets, such as bloom's mip chain, frees them in `release_targets`. A pass reads its input with `textureLoad` or a sampler and writes every output pixel, so it never needs to load the output.

`PassContext` carries the render context, the chain's size and `projection`, the projection matrix of the camera the frame was rendered with. `SceneRenderer::render_scene_base` hands it to the chain with `PostProcessChain::set_projection`, so a pass that reads depth can rebuild view-space positions with the same matrix the depth was written through.

## Chain And Target Pool

`PostProcessChain` owns the passes and a `TargetPool`.

- Passes run in stage order (`Hdr`, `Tonemap`, `Ldr`), then insertion order within a stage.
- Each frame the chain keeps the passes whose `enabled` returns true and whose inputs are available, then wires them: the first reads the chain's source, the last writes the chain's output, and each pass between writes the pool target the next one reads. Intermediates alternate between two pool targets (A, B), so a pass never reads the texture it writes.
- The pool is fetched from only when two or more passes run. The tone-map-only chain never touches it.
- The chain calls `release_targets` on every pass when the renderer resizes, and on each pass that does not run in a frame, so a disabled effect holds no texture.
- `TargetPool` holds at most the two slots, A and B. It creates each on first use at the chain's size and format, recreates one whose size or format differ, reuses them across frames and frees both when the renderer resizes.

The wiring is a pure plan (`ChainPlan`) built before any GPU work, which keeps ordering and ping-pong testable without a device.

## Shared Inputs

A pass declares the scene inputs it reads in `inputs()`. The chain builds each one only when an enabled pass asks for it.

- **Depth**: without MSAA the scene's depth texture is passed through. With MSAA a small pass copies sample 0 of the multisampled depth into a single-sample `Depth32Float` texture, built once per frame for every pass that asks.
- **Normals, velocity**: not produced yet. A pass that asks for one is skipped with a `PostProcessError::InputUnavailable` warning naming the pass and input.

Tone mapping asks for nothing.

## Tone Mapping

`TonemapPass` is the `Tonemap` stage. It always runs: a missing or disabled volume selects `PostProcessSettings::default()`, never untone-mapped output.

- Its uniform holds the exposure scale `2^EV` and the preset.
- It draws one full-screen triangle (`shaders/fullscreen.wgsl`, no vertex buffers).
- `TonemappingPreset::Aces` is Stephen Hill's fit of the ACES RRT and ODT (BakingLab, MIT): the ACES input matrix, the rational RRT/ODT curve, then the output matrix, clamped to `[0, 1]`. It maps linear 0.18 to about 0.106 and 1.0 to about 0.619.
- Alpha passes through unchanged, so views cleared to transparent, such as thumbnails, keep their transparency.

## Ambient Occlusion

`SsaoPass` (`postprocess/ssao.rs`) is an `Hdr` pass that reads `SceneInput::Depth`. It is enabled by `settings.ambient_occlusion.enabled`, off by default, so a frame without it keeps the tone-map-only plan and builds no depth copy. The renderer registers it before every other `Hdr` pass, so it darkens the scene before depth of field blurs it and before bloom spreads light. It records three render passes:

```text
occlusion   depth (full size)            -> occlusion (1/2)   visibility in r, view z in g
blur        occlusion                    -> blurred (1/2)     depth-aware 4x4
composite   input, blurred, depth        -> output            input.rgb * visibility, alpha kept
```

- **Reconstruction**: the renderer produces no normals (`SceneInput::Normals` is unavailable), so each occlusion texel rebuilds its view-space position from depth with the inverse of `PassContext::projection`, and its normal from the cross product of the position differences to its horizontal and vertical neighbours. On each axis it takes the neighbour closer in depth, so normals do not bend at silhouettes, and it turns the normal toward the camera.
- **Hemisphere**: up to 64 kernel offsets in the unit hemisphere around +z, generated once per sample count from a fixed seed. They cluster toward the center and keep at least 0.15 of their length along the normal, so a flat surface does not occlude itself. Each texel turns the kernel around its normal by one of 16 angles from a 4x4 tile in Bayer order, then scales it by `radius`.
- **Occlusion**: each sample is projected back to the screen and compared with the depth there. A surface more than `bias` in front of the sample occludes it, faded out by `smoothstep(radius / |dz|)` when that surface lies far in front of the pixel, so a foreground object does not darken the wall behind it. Samples off screen or on the sky do not count. Visibility is `(1 - occluded / samples) ^ intensity`.
- **Blur**: a 4x4 box over exactly one tile of the noise, which cancels the rotation pattern, weighting each texel by how close its view depth is to the center's (zero at a 10% difference).
- **Composite**: reads the input and depth with `textureLoad`, upsamples the blurred visibility from its four nearest texels, weighted bilinearly and by depth, and multiplies it into the color. Sky pixels, at depth `0.9999999` (what the skybox writes) or beyond, pass through bit for bit, and so does alpha.
- **Targets**: two `Rg16Float` textures at half the chain's size, rounded up. The pass creates them on the first frame SSAO runs, recreates them when the size changes, and frees them in `release_targets`, so a disabled or resized SSAO holds no texture.
- **Approximation**: the occlusion multiplies the final HDR color, which already holds direct light, so it also darkens creases a light shines straight into. Ambient occlusion should only attenuate indirect light. This stands until the renderer produces normals or a G-buffer, so the forward pass can apply occlusion to its ambient and image-based terms alone.

## Settings And Volumes

```rust
pub struct PostProcessSettings { pub tonemapping: TonemapSettings, pub ambient_occlusion: AmbientOcclusionSettings }
pub struct TonemapSettings { pub preset: TonemappingPreset, pub exposure_ev: f32 }  // Aces, 0.0
pub struct AmbientOcclusionSettings {                         // off, 0.5, 1.0, 0.025, 16
    pub enabled: bool, pub radius: f32, pub intensity: f32, pub bias: f32, pub sample_count: u32,
}
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

- `assets/shaders/fullscreen.wgsl`: the shared full-screen triangle, `fullscreen_vertex(index)`. `assets/shaders/fullscreen_pass.wgsl` includes it and declares the `vs_main` that calls it, so a shader that draws the plain triangle includes the entry point and declares only its fragment stage: the post passes, the overlay composite, the selection outline and 2D mip generation. The editor grid adds outputs of its own, so it includes `fullscreen.wgsl` and declares its own vertex stage. Every full-screen draw takes its pipeline options from `PipelineOptions::fullscreen_builder()`, which sets only the color target, depth and sample count; the engine has no full-screen quad mesh.
- `assets/shaders/postprocess/tonemap.wgsl`: the tone-map pass.
- `assets/shaders/postprocess/ssao.wgsl`, `ssao_blur.wgsl` and `ssao_composite.wgsl`: the SSAO passes. `ssao_common.wgsl` holds their uniform, the view-space reconstruction and the depth weight they share.
- `assets/shaders/postprocess/depth_resolve.wgsl`: copies sample 0 of the multisampled depth.
- `assets/shaders/overlay_composite.wgsl`: blends the resolved overlay target over the scene texture.
- `pbr.wgsl` drops Reinhard, gamma 1.4 and the `* 5.0` on albedo.
- `skybox.wgsl` drops its exposure and gamma and scales the sky by the sky light's intensity, the same factor that scales its image-based lighting.
- Particles already write linear color.

Post shaders use `//#include` modules, which carry over unchanged to the `calyx_shader` crate.

## Callers

The editor viewport, the Game tab, the thumbnail generator and the standalone player all call `render_scene_base` then `finalize_scene`, so each gets the chain with no change. The viewport and the Game tab render the same scene, so they select the same settings.

## Calibration

Removing the per-shader curves and the `* 5.0` changes every scene's brightness. Scenes are recalibrated through data, never a shader constant: a global volume's exposure and the scene's light intensities.

- **Sandbox scene**: lit only by its sky before, with dark materials (the arena floor's albedo is 0.13 with ambient occlusion 0.3) that the old curve lifted to mid grey. Exposure alone cannot bring them back: the sky and the image-based light it casts scale together, so any EV that lifts the floor blows out the sky. The scene gains a `Sun` directional light (intensity 5, pitched 60 degrees down), which lights the floor without brightening the sky, and a `Post Process` global volume at 0 EV, the knob for later tuning.
- **Thumbnails**: the preview lights rise to 6 (directional), 0.8 (ambient) and 0.8 (point) so prefab thumbnails read as before at the default 0 EV. The thumbnail disk cache key gains a render version, so thumbnails cached under the old curve render again.

The grid, gizmos and outline composite after post (#379, see Editor Overlays), so they keep their authored colors. Editor clear colors are still tone mapped.

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
- SSAO leaves a flat plane facing the camera unoccluded, darkens an inside corner most at its crease, and leaves the sky, at a cleared or skybox depth, bit for bit unchanged. With it disabled the renderer's plan is tone mapping alone. Its targets are half the chain's size and freed on release, and settings saved before it load with it off.

Headless tests in `engine/src/render/overlay/` and `scene_renderer.rs`:

- A half-covered, anti-aliased overlay edge pixel composites as premultiplied alpha, without darkening.
- Enabled overlays record in order, and the overlay target exists only while one is enabled.
- The selection outline keeps its exact authored color at +3 EV, at one and four samples, and picking is unchanged.
- Views without overlays allocate no overlay target.

## Roadmap

- **#113** adds effects as `Hdr` and `Ldr` passes, each with an optional settings sub-struct.
- **#114** adds local volumes that blend `PostProcessSettings` before the chain runs.
- **#115** adds editor UI for post-processing profiles.
- **#378** adds a showcase scene that validates the look end to end.
