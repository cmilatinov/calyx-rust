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

`PassContext::camera` carries the near and far planes of the camera the frame was drawn with, as `CameraPlanes`. `render_scene_base` hands them to the chain with `PostProcessChain::set_camera`, so `finalize_scene` still needs no camera. `CameraPlanes::linear_depth` turns a depth-buffer value into view distance: the projection is `perspective_lh` with OpenGL's `[-1, 1]` depth range, of which the depth buffer keeps `[0, 1]`, so `distance = 2nf / (f + n - d (f - n))`.

## Depth Of Field

`DepthOfFieldPass` (`postprocess/dof.rs`) is an `Hdr` pass that reads `SceneInput::Depth`. It runs while `settings.depth_of_field.enabled` is on, which it is not by default, so a frame without it keeps the tone-map-only plan. It is the first `Hdr` pass, before bloom, so bloom spreads highlights that are already out of focus, as a lens would. It records two render passes:

```text
coc     depth           -> coc (R16Float, owned)   signed circle of confusion, in pixels
blur    input + coc     -> output                  scatter-as-gather disc blur
```

- **Circle of confusion**: `clamp((distance - focus_distance) / focus_range, -1, 1) * max_blur`, from the linear depth above. It is negative in front of the focus plane (near field), positive behind it (far field) and zero on it, and reaches `max_blur` pixels `focus_range` world units either side of the plane. The sky, at the far plane, is fully blurred.
- **Blur**: a gather over a golden-angle spiral, after Dennis Gustafsson's single-pass bokeh depth of field. It starts one pixel out and steps the radius by `radius_scale / radius` up to `max_blur`. A tap counts when its own circle of confusion reaches the pixel, with a weight ramping from 0 to 1 over the pixel either side of the tap's distance, and the taps keep a running average, so a pixel blurs by its neighbours' circles, not only its own.
- **Near and far field**: one gather handles both. A tap behind the pixel (larger signed circle) is clamped to twice the pixel's own radius, so a blurred background never bleeds over a sharp or nearer edge. A tap in front of it is not clamped, so a blurred foreground spreads over the sharp pixels behind it.
- **In focus**: a pixel whose circle is under half a pixel, with no blurred foreground over it, keeps its exact input value: every tap weighs zero and leaves the average untouched.
- `max_blur` is clamped to 32 pixels. Taps number about `max_blur^2 / (2 * radius_scale)`, and `radius_scale` is `max(0.5, max_blur / 16)`, so a large blur stays near 256 taps.
- Color and alpha blur together, through one linear, clamp-to-edge sampler that also filters the circle of confusion.
- The pass owns the circle of confusion texture, separate from the pool targets. It creates it at the chain's size on the first frame depth of field runs, recreates it when the size changes, and frees it in `release_targets`, so a disabled or resized pass holds no texture.
- Out of scope: bokeh shapes and physically based aperture settings.

## Tone Mapping

`TonemapPass` is the `Tonemap` stage. It always runs: a missing or disabled volume selects `PostProcessSettings::default()`, never untone-mapped output.

- Its uniform holds the exposure scale `2^EV` and the preset.
- It draws one full-screen triangle (`shaders/fullscreen.wgsl`, no vertex buffers).
- `TonemappingPreset::Aces` is Stephen Hill's fit of the ACES RRT and ODT (BakingLab, MIT): the ACES input matrix, the rational RRT/ODT curve, then the output matrix, clamped to `[0, 1]`. It maps linear 0.18 to about 0.106 and 1.0 to about 0.619.
- Alpha passes through unchanged, so views cleared to transparent, such as thumbnails, keep their transparency.

## Bloom

`BloomPass` (`postprocess/bloom.rs`) is an `Hdr` pass. It is enabled by `settings.bloom.enabled`, off by default, so a frame without bloom keeps the tone-map-only plan. It records several render passes into the encoder:

```text
prefilter    input (full size)  -> mip 0 (1/2)   13-tap downsample, soft-knee threshold, then / N
downsample   mip i-1            -> mip i         13-tap downsample, for i in 1..N
upsample     mip i              -> mip i-1       3x3 tent, added (One + One blend), for i in N-1..1
composite    input + mip 0      -> output        input + tent(mip 0) * intensity, alpha kept
```

- **Threshold**: the prefilter measures a pixel's brightest channel `b`, so saturated lights bloom in their own color. With `t = threshold` and `k = threshold * knee`, light below `t - k` contributes nothing, light above `t + k` contributes what lies above `t`, and between the two a quadratic `(b - t + k)^2 / 4k` joins them. The color is scaled by that contribution over `b`. An image entirely below `t - k` therefore passes through bit for bit.
- **Downsample**: the 13-tap filter from Jimenez's Call of Duty: Advanced Warfare talk, five overlapping bilinear 2x2 boxes weighted to a 4x4 low-pass. It avoids the flicker a plain 2x2 box gives when a bright pixel moves.
- **Upsample**: each mip is tent-filtered and blended additively onto the next larger mip, so mip 0 ends up holding the sum of every level's blur, each blurred twice as wide as the one above it.
- **Composite**: reads the input with `textureLoad` and adds the tent-filtered mip 0, scaled by `intensity`. The prefilter already divided the light by the mip count `N`, which keeps the added energy about the same whatever the radius and keeps mip 0, the sum of `N` levels, within half-float range: a flat 20,000 summed over six undivided levels would reach 120,000, past the half-float maximum of 65,504. The sum is clamped to 65,504, so a bright input saturates instead of reaching the tone mapper as infinity.
- **Mip chain**: `radius` is the mip count `N`, clamped to `1..=8` and to the halvings the chain's size allows before a side reaches one pixel. Mip `i` is the chain's size shifted right by `i + 1`. The pass owns the mips, separate from the pool targets, in the input's format. It creates them on the first frame bloom runs, recreates any whose size or format no longer match the chain, and frees them in `release_targets`, so a disabled or resized bloom holds no texture.
- One linear, clamp-to-edge sampler and one uniform (threshold, absolute knee, intensity, `1 / N`) serve every draw. Each mip caches the bind groups that read into it.

## Colour Grading And Vignette

`ColorGradingPass` (`engine/src/render/postprocess/color_grading.rs`) is one `Ldr` pass that grades the tone-mapped image and then applies a vignette. It runs while `color_grading.enabled` or `vignette.enabled` is on, neither of which is by default, so a frame without them keeps the tone-map-only plan. A disabled half reaches the shader as neutral values, which leave the color unchanged.

It runs on display-linear color, in this order:

- **White balance**: `temperature` (blue to orange, -1 to 1) and `tint` (green to magenta, -1 to 1) shift the white point a von Kries scale corrects in CAT02 LMS. The illuminant's chromaticity moves along the daylight locus for temperature and off it for tint, as in Unity's post-processing stack. The CPU folds linear sRGB to LMS, the scale and LMS back to linear sRGB into one matrix, relative to the scale at 0 and 0, so a neutral white balance is exactly the identity.
- **Contrast**: `pivot * (c / pivot)^contrast` per channel, a straight line through linear 18% grey (`0.18`) in log space. 1 is unchanged, 0 is flat mid grey.
- **Saturation**: a mix between the color and its Rec. 709 luminance (`0.2126, 0.7152, 0.0722`, linear sRGB). 1 is unchanged, 0 is grey.
- The graded color is clamped to `[0, 1]`.
- **Vignette**: with `d = |uv - 0.5| * intensity * 3` and `d.x` scaled by the aspect ratio so the shape is round, the factor is `(1 - d.d)^(smoothness * 5)`, clamped. The color is multiplied by `mix(color, 1, factor)`: unchanged at the centre, fading to `color` (black by default) towards the edges. `color` is an sRGB `Color32` converted to linear on the CPU.

Grading runs before FXAA (#400), as is usual: FXAA's edge detection then sees the final image, and grading does not change the contrast FXAA already smoothed. Both are `Ldr` passes, so the order is their registration order in `SceneRenderer::new`: tone mapping, colour grading, then FXAA.

Alpha is read with the pixel and passes through, so transparent views such as thumbnails stay transparent.

## Anti-Aliasing

`FxaaPass` (`engine/src/render/postprocess/fxaa.rs`) is an `Ldr` pass that runs FXAA 3.11 Quality, preset 12, after tone mapping. It runs while `PostProcessSettings::anti_aliasing.fxaa` is on, which it is not by default, so a frame without it keeps the tone-map-only plan.

- `assets/shaders/postprocess/fxaa.wgsl` ports the PC quality path of Timothy Lottes' `FxaaPixelShader`: early exit below the local contrast threshold, horizontal or vertical edge choice, a five-step search along the edge (1, 1.5, 2, 4 and 12 pixels), and the sub-pixel blend. It samples its input with a linear, clamp-to-edge sampler the pass owns.
- It runs after tone mapping because its luma edge detection needs display-range values. Its input is display-linear, so luma is the square root of linear Rec. 601 luma, close to the gamma-encoded luma the reference expects.
- `edge_threshold` (default 0.166) is the minimum contrast, relative to the brightest neighbour, treated as an edge. `subpixel` (default 0.75) is how much sub-pixel aliasing is removed. Both are the reference's defaults. The absolute floor, 0.0833, is a shader constant.
- Alpha is read from the pixel itself, never filtered, so transparent views such as thumbnails stay transparent.
- FXAA does not see editor overlays: they composite after the chain and keep their MSAA.

## Settings And Volumes

```rust
pub struct PostProcessSettings {
    pub tonemapping: TonemapSettings,
    pub depth_of_field: DepthOfFieldSettings,
    pub bloom: BloomSettings,
    pub color_grading: ColorGradingSettings,
    pub vignette: VignetteSettings,
    pub anti_aliasing: AntiAliasingSettings,
}
pub struct TonemapSettings { pub preset: TonemappingPreset, pub exposure_ev: f32 }  // Aces, 0.0
pub struct BloomSettings {                                    // off, 1.0, 0.5, 0.5, 6
    pub enabled: bool, pub threshold: f32, pub knee: f32, pub intensity: f32, pub radius: u32,
}
pub enum TonemappingPreset { Aces }
pub struct DepthOfFieldSettings {                             // off, 10.0, 10.0, 8.0
    pub enabled: bool, pub focus_distance: f32, pub focus_range: f32, pub max_blur: f32,
}
pub struct ColorGradingSettings { pub enabled: bool, pub contrast: f32, pub saturation: f32, pub temperature: f32, pub tint: f32 }  // false, 1, 1, 0, 0
pub struct VignetteSettings { pub enabled: bool, pub intensity: f32, pub smoothness: f32, pub color: Color32 }  // false, 0.4, 0.2, black
pub struct AntiAliasingSettings { pub fxaa: bool, pub edge_threshold: f32, pub subpixel: f32 }  // false, 0.166, 0.75

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
- `assets/shaders/postprocess/bloom_prefilter.wgsl`, `bloom_downsample.wgsl`, `bloom_upsample.wgsl` and `bloom_composite.wgsl`: the bloom passes. `bloom_filters.wgsl` holds the 13-tap downsample and the tent upsample they share.
- `assets/shaders/postprocess/color_grading.wgsl`: the colour grading and vignette pass.
- `assets/shaders/postprocess/fxaa.wgsl`: the FXAA pass.
- `assets/shaders/postprocess/depth_resolve.wgsl`: copies sample 0 of the multisampled depth.
- `assets/shaders/postprocess/dof_coc.wgsl` and `dof_blur.wgsl`: the depth of field passes. `dof_common.wgsl` holds their uniform, the depth linearization and the circle of confusion.
- `assets/shaders/overlay_composite.wgsl`: blends the resolved overlay target over the scene texture.
- `pbr.wgsl` drops Reinhard, gamma 1.4 and the `* 5.0` on albedo. It adds linear HDR emission, `emission_color * emission_strength`, after the lighting. A material saved before those fields existed reads zero strength and does not emit.
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

- **Showcase scene**: `sandbox/assets/post_showcase.cxscene` (#378) is the fixed reference for the look: rough, glossy, metallic and emissive materials (`materials/post_showcase/`), the sunset sky at intensity 1, a sun at 2.5, point lights from 5 to 5000, the campfire particles and a global volume at 0 EV.

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
- Depth of field (`dof.rs`): `CameraPlanes` inverts the render projection; pixels at the focus distance are unchanged; far pixels blur more than pixels near focus, and pixels in front of it blur too; a blurred background stays behind a sharp edge while a blurred foreground spreads over it; the pass reads depth at one and four samples with the same result; with it disabled the plan is the tone-map-only plan and its texture is not allocated, follows the chain's size, and is freed when disabled and on resize. Settings saved before it load with it off.
- Bloom (`bloom.rs`): a single pixel above the threshold spreads energy into its neighbours, fading with distance, and keeps alpha; an image entirely below the threshold passes through unchanged; a flat image of 20,000 blooms to the expected value without overflowing the half-float mips; a composite past the half-float maximum saturates at it; with bloom disabled the renderer's plan is the tone-map-only plan; the mip chain follows the chain's size and the radius and is freed on release; the chain releases a pass's targets on resize and on frames the pass does not run.
- Colour grading (`color_grading/tests.rs`): neutral settings return the input, zero saturation gives grey at the input's luminance, contrast keeps mid grey, positive temperature warms and positive tint adds magenta, the vignette darkens the corners and leaves the centre unchanged, and alpha is preserved. With both effects off the pass is left out of the plan, the scene renderer plans it before FXAA, and settings saved before it load with both off.
- FXAA smooths a hard diagonal edge and keeps each pixel's alpha, leaves a flat colour and its alpha unchanged, and leaves the plan unchanged while disabled. Settings saved before it load with it off.

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
