# Post-Processing Architecture

Related work:

- Epic: [Post-processing (#200)](https://github.com/cmilatinov/calyx-rust/issues/200)
- This design: [#112](https://github.com/cmilatinov/calyx-rust/issues/112)
- Follow-ups: effects (#113), local volumes (#114), editor profile UI (#115), overlay compositing (#379), showcase scene (#378)

## Goals

- Lit shaders write linear HDR radiance. Tone mapping is the single display transform, applied once, after the scene is drawn.
- An ordered chain of full-screen passes runs between the MSAA resolve and the presented scene texture, so effects can be added without touching the forward pass.
- A frame with only tone mapping costs one full-screen pass and allocates no intermediate targets.
- A scene configures the chain with a volume component and one reflected, serialized component per effect on the same game object, which the inspector edits with no custom UI. A scene without a volume still gets tone mapping.
- Every view of a scene goes through the same chain: editor viewport, Game tab, player, prefab mode and asset thumbnails.

## Frame Today

`SceneRenderer` (`crates/calyx_engine/src/render/scene_renderer.rs`) renders a frame in two calls.

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
                    (opaque PBR meshes through the deferred lighting pass, see Deferred Shading)
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

The grid, gizmos and the selection outline draw after the whole chain, Ldr stage included, so tone mapping, exposure and later effects never change their colors. `crates/calyx_engine/src/render/overlay/` holds them.

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
- Overlays alpha blend (`RenderUtils::color_alpha_blending`) into the transparent target, which leaves premultiplied color with coverage alpha. The composite therefore blends the resolved target over `scene_texture` as premultiplied alpha, `rgb + dst * (1 - a)` (`shaders/editor/overlay_composite.wgsl`). Blending it as `rgb * a + dst * (1 - a)` would multiply by alpha twice and darken anti-aliased edges.
- Particles stay in the scene pass and are tone mapped with it. Alpha-blended particles past half coverage write the scene depth, so they hide overlays behind them (see Particles And Depth).

`OverlayPass` mirrors `PostProcessPass` but stays separate. Post-process passes take the encoder because some record several render passes, such as bloom's downsample and upsample chain, while overlays all draw into one shared multisampled pass. The shared code lives in the full-screen helpers below instead.

## Display Encoding

`scene_texture` holds **display-linear** values: tone-mapped linear color in `[0, 1]`, with no gamma applied. Everything that reads it already assumes so:

- egui-wgpu 0.31 samples a native texture as linear and sRGB-encodes the result itself: both of its fragment entry points (`fs_main_gamma_framebuffer`, and `fs_main_linear_framebuffer` for an sRGB target) run `gamma_from_linear_rgba` on the sampled texel.
- `TextureReadback`, behind screenshots, `calyx_ctl screenshot --target viewport|game` and thumbnails, sRGB-encodes the color channels and keeps alpha linear.

So the tone-map pass outputs the curve's result unchanged. The old `pbr.wgsl` raised its Reinhard output to `1 / 1.4` before egui encoded it again, which is why the old look was lifted and washed out. The headless test `tonemap_output_is_display_linear` pins this: a known HDR value goes through the tone-map pass, the raw `Rgba16Float` texel must equal the curve's linear output, and the readback byte must equal its sRGB encoding.

An 8-bit colour texture gets an sRGB view beside its `Rgba8Unorm` data, and a material samples its color slots (base color, albedo, diffuse, emissive) through it, so albedo maps decode from sRGB; data slots such as normals and metallic-roughness sample the linear view. A material's solid `Color` value is a 1x1 texture with no sRGB view, so it is read as linear.

## Pass Interface

`crates/calyx_engine/src/render/postprocess/` holds the chain.

```rust
pub enum PostProcessStage { Hdr, Tonemap, Ldr }

#[bitflags]
#[repr(u8)]
pub enum SceneInput { Depth, Normals, Velocity, ParticleDepth, Albedo, GBufferDepth, Material }  // one bit each
pub type PassInputs = BitFlags<SceneInput>;                  // enumflags2

pub trait PostProcessPass {
    fn name(&self) -> &str;
    fn stage(&self) -> PostProcessStage;
    fn inputs(&self) -> PassInputs;                          // default: none
    fn enabled(&self, settings: &PostProcessSettings) -> bool;
    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings);
    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo)
        -> Result<(), NoPipeline>;
    fn release_targets(&mut self) {}                         // default: owns none
}

pub struct PassIo<'a> {
    pub input: &'a Texture,          // never the output
    pub output: &'a Texture,
    pub depth: Option<&'a Texture>,  // each scene input only when requested
    pub particle_depth: Option<&'a Texture>,
    pub normals: Option<&'a Texture>,
    pub albedo: Option<&'a Texture>,
    pub gbuffer_depth: Option<&'a Texture>,
    pub material: Option<&'a Texture>,
}
```

`prepare` uploads the pass's uniforms from the settings. `record` draws from `input` into `output`. A pass that owns textures besides the pool targets, such as bloom's mip chain, frees them in `release_targets`. A pass reads its input with `textureLoad` or a sampler and writes every output pixel, so it never needs to load the output.

## Chain And Target Pool

`PostProcessChain` owns the passes and a `TargetPool`.

- Passes run in stage order (`Hdr`, `Tonemap`, `Ldr`), then insertion order within a stage.
- Each frame the chain keeps the passes whose `enabled` returns true and whose inputs are available, then wires them: the first reads the chain's source, the last writes the chain's output, and each pass between writes the pool target the next one reads. Intermediates alternate between two pool targets (A, B), so a pass never reads the texture it writes.
- The pool is fetched from only when two or more passes run. The tone-map-only chain never touches it.
- A pass whose shader has no pipeline (wgpu rejected it, and no earlier version of the shader built one) records nothing and returns `NoPipeline`. The chain then copies the pass's input to its output, so the target never keeps a stale frame, and logs a warning once per pass and shader. Pool targets carry `COPY_SRC` and `COPY_DST` for that copy. The copy is a plain texture copy, not a shader, so no hot reload can take it away.
- The chain calls `release_targets` on every pass when the renderer resizes, and on each pass that does not run in a frame, so a disabled effect holds no texture.
- `TargetPool` holds at most the two slots, A and B. It creates each on first use at the chain's size and format, recreates one whose size or format differ, reuses them across frames and frees both when the renderer resizes.

The wiring is a pure plan (`ChainPlan`) built before any GPU work, which keeps ordering and ping-pong testable without a device.

## Shared Inputs

A pass declares the scene inputs it reads in `inputs()`. The chain builds each one only when an enabled pass asks for it, and hands it only to the passes that asked, in `PassIo`.

- **How they reach the chain**: `render_scene_base` draws the scene and keeps its depth, the particle depth and the G-buffer on the renderer. `finalize_scene` hands them to `PostProcessChain::run` in `ChainTargets`, at whatever sample count they were drawn. `ChainPlan::build` records which inputs the frame's planned passes read (`plan.depth`, `plan.particle_depth`, `plan.normals`, `plan.albedo`), and the chain resolves or passes through only those. `PostProcessChain::reads` answers the same question for the renderer before the scene is drawn, which the particle depth uses.
- **Depth**: without MSAA the scene's depth texture is passed through. With MSAA a small pass copies sample 0 of the multisampled depth into a single-sample `Depth32Float` texture, built once per frame for every pass that asks.
- **Particle depth**: the scene depth with every particle drawn in, including those that stay out of the scene depth, nearest wins (see Particles And Depth). The particle renderer writes it at the scene's sample count only when a pass that runs asks for it and a particle is drawn; the chain resolves it like depth. Otherwise it is the scene depth, resolved once for both.
- **Normals**: the G-buffer's `normals` target itself, `Rgba16Float`: the view-space shading normal in RG and the view-space geometric normal in BA, each octahedral (see [G-Buffer](../rendering/gbuffer.md#normal-encoding)). A pass decodes the one it needs with `gbuffer_decode_normal` from `shaders/deferred/gbuffer_encoding.wgsl`. The chain passes the raw target, not a decoded copy: decoding is a few instructions where a pass loads a texel, while a decoded copy would cost a full-screen pass and a target every frame and would keep only one of the two normals. Both are in the view space of `camera.view`, `+z` away from the camera. SSAO reads the same target, but before the lighting pass and outside the chain (see Ambient Occlusion).
- **Albedo**: the G-buffer's `albedo` target itself, `Rgba8UnormSrgb`, so a pass that samples or loads it gets linear base color. Its alpha is the G-buffer's coverage: 1 where an opaque PBR surface was drawn, 0 elsewhere (sky, particles, meshes drawn with another shader). The normals target holds zeros where alpha is 0, so a pass that reads normals and needs to know where they hold a surface asks for albedo as well.
- **Normals and albedo under MSAA**: the G-buffer is single-sample at every sample count and is never resolved (see [G-Buffer](../rendering/gbuffer.md#msaa)), so the chain hands passes the same single-sample targets at one and four samples, and builds nothing for them. Above one sample the chain's depth is sample 0 of the scene depth, which for a PBR pixel is the G-buffer depth the lighting pass wrote into every sample, so depth, normals and albedo describe the same surface there.
- **Normals and albedo are lazy for free**: every frame draws the G-buffer for the deferred lighting pass, so a pass reading them adds no pass and no target, and a frame without such a pass does no extra work either. A G-buffer view replaces the chain, so it never reads them.
- **What they do not see**: the G-buffer holds only opaque PBR surfaces. Where a mesh drawn with another shader, or an alpha-blended particle written into the scene depth, covers a PBR surface, the depth holds the nearer draw while normals and albedo still hold the surface behind it. A pass that must agree with the depth there compares the depth with the particle depth, treats such pixels as it treats sky, or reads the G-buffer depth.
- **G-buffer depth**: the depth of the opaque PBR surfaces alone, the one the normals and albedo describe, before any forward draw, the skybox or a particle writes the scene depth; cleared to 1 where no surface was drawn. Above one sample it is the G-buffer's own single-sample depth, which exists anyway. At one sample the geometry pass draws the scene depth itself, so the renderer copies it into a texture it keeps (`gbuffer_depth_copy`) right after the geometry pass, only on frames whose chain reads it (`PostProcessChain::reads`), and frees the copy on the other frames and on resize. The ink outlines read it (see Outlines).
- **Material**: the G-buffer's `material` target itself, `Rgba8Unorm`: metallic, roughness and ambient occlusion in RGB and the material's outline style in A, as `style / 255` (see [G-Buffer](../rendering/gbuffer.md)). Like normals and albedo it is single-sample at every sample count and costs nothing to read. The ink outlines read the style from it.
- **Velocity**: not produced yet. A pass that asks for it is skipped with a `PostProcessError::InputUnavailable` warning naming the pass and input.

Tone mapping asks for nothing.

`PassContext::camera` carries the near and far planes of the camera the frame was drawn with, as `CameraPlanes`. `CameraPlanes` is `#[repr(C)]` and `Pod`, laid out as the WGSL struct of the same name in `shaders/postprocess/camera_planes.wgsl` (near, far, then `tan_half_fov` at offset 8; 16 bytes), so the fog, SSAO, depth of field and outline uniforms embed it as a `camera` member and the Rust side writes it as it is. That module holds the helpers every pass uses in place of its own copy of the formulas: `camera_linear_depth`, `camera_inverse_depth`, `camera_ndc` and `camera_uv`, `camera_ray`, `camera_view_position`, `camera_project` and `camera_ray_distance`. `render_scene_base` hands them to the chain with `PostProcessChain::set_camera`, and the camera's world transform, from its view space to world space, with `PostProcessChain::set_view_to_world`, which passes see as `PassContext::view_to_world`, so `finalize_scene` still needs no camera. `CameraPlanes::linear_depth` turns a depth-buffer value into view distance: the projection is `perspective_lh` with OpenGL's `[-1, 1]` depth range, of which the depth buffer keeps `[0, 1]`, so `distance = 2nf / (f + n - d (f - n))`.

## Particles And Depth

Particles draw in the scene pass, after the meshes and the skybox, sorted back to front within each system. Their colour draws never write depth, so particles blend against each other in that order. What reaches depth depends on the blend mode (#419):

| Blend mode | Scene depth | Particle depth | Depth of field | SSAO |
|---|---|---|---|---|
| `Alpha` | written where coverage is at least 0.5 | written where coverage is at least 0.1 | blurred by its own distance | neither occludes nor is darkened |
| `Additive` | never written | written where coverage is at least 0.1 | blurred by its own distance | neither occludes nor is darkened |

- **Coverage** is the texture's alpha times the particle's alpha, inside the billboard's disc. `ParticleRenderer::SCENE_DEPTH_COVERAGE` and `PARTICLE_DEPTH_COVERAGE` hold the thresholds.
- **Alpha-blended particles write depth**: past half coverage a particle hides more of what is behind it than it shows, so it occludes like geometry. A depth-only pass (`shaders/particles/particles_depth.wgsl`) draws every alpha system's particles into the scene depth after all particle colour has drawn, so a depth write never hides a particle from one drawn after it. Every reader of the scene depth sees them: depth of field focuses on them, and the grid and gizmos behind dense smoke are hidden. Their fringe below half coverage stays out of the scene depth, like an additive particle.
- **Additive particles stay out of depth**: they add light and hide nothing, so the scene depth, the grid and gizmos never see them, nor the faint fringe of alpha-blended particles. For the passes that must not treat them as background, the renderer copies the scene depth into the particle depth and the same depth-only pass draws every particle into it, at a low threshold so the visible glow and fringe count.
- **Depth of field** reads the particle depth, so every visible particle gets the circle of confusion of its own distance, never the background's. Through a particle the background takes the particle's circle too, which a single depth per pixel cannot avoid.
- **SSAO** runs before any particle draws, on the G-buffer's depth (see Ambient Occlusion), so no particle occludes geometry, and a particle's light, added over the lit surface, is never darkened.
- **Fog** reads the scene depth, so alpha-blended particles past half coverage take the fog of their own distance, while additive particles and alpha fringes take the fog of the surface behind them (see Fog).
- Soft particles, which fade where they meet geometry, do not exist yet. They would be additive or below the alpha threshold, so they would stay out of the scene depth the same way.
- A deferred G-buffer (#410) supplies opaque depth before the forward particles draw, so the same depth-only pass writes into it unchanged.

`CameraPlanes::tan_half_fov` holds the tangents of half the horizontal and vertical fields of view, taken from the projection matrix (`1 / P[0][0]`, `1 / P[1][1]`), so a pass can rebuild view-space positions: the pixel at normalized device coordinates `ndc` and view distance `z` lies at `(ndc * tan_half_fov * z, z)`, and a view-space point projects back to `ndc = xy / (tan_half_fov * z)`.
## Fog

`FogPass` (`postprocess/fog.rs`) is an `Hdr` pass that reads `SceneInput::Depth` and fades each pixel towards a fog colour by its distance from the camera. It runs while the settings hold a `fog`, from an enabled Fog component on the volume, so a frame without it keeps the tone-map-only plan and builds no depth copy. It records one full-screen pass (`shaders/postprocess/fog.wgsl`) and owns no texture.

- **Distance**: the view depth from `CameraPlanes::linear_depth`, stretched along the pixel's ray with `tan_half_fov`, so a surface's fog does not change as the camera turns about it. Unity's built-in fog uses the view depth instead, which fogs the screen's edges less than its centre.
- **Amount**: `Linear` is `saturate((distance - start) / (end - start))`, a hard step at `start` when `end <= start`; `Exponential` is `1 - exp(-density * distance)`. Either is scaled by `opacity`, the most of the fog colour any pixel takes. The output is `mix(color, fog_color, amount)`, alpha unchanged.
- **Colour**: an sRGB `Color32` converted to linear and scaled by `intensity`, so it is linear HDR radiance mixed in before tone mapping. Exposure and the curve then apply to the fog as to the scene, so a fog colour shows darker than its swatch at 0 EV under ACES; `intensity` brings it back, as it does for a light.
- **Background**: the cleared depth (1) and the skybox's depth (`0.9999999`) both linearize to the far plane, so the sky or the clear colour takes the fog of the far plane's distance with no special case: linear fog that ends before the far plane covers it entirely, and exponential fog covers it by `1 - exp(-density * far)`. Distant geometry and the background therefore meet in the same colour at the horizon. The sky above the horizon takes the same fog as the horizon; fading the fog with height on the sky would need the camera's orientation in `PassContext`.
- **Order**: first in the `Hdr` stage. SSAO is applied inside the lighting pass, before the chain (see Ambient Occlusion), so occlusion darkens only the surfaces' ambient light and never the fog in front of them, as the fog's in-scattered light is not occluded by the geometry. Before depth of field, so a fogged distant surface blurs to its fogged colour, as a lens sees it. Before bloom, so bloom spreads the light that reaches the camera through the fog: a bright light far in the fog is dimmed before it blooms.
- **What it fogs**: everything in the scene depth by its own distance: the opaque PBR surfaces, meshes drawn with another shader in the forward pass, and alpha-blended particles past half coverage (see Particles And Depth). Additive particles and the faint fringe of alpha-blended ones are not in the scene depth, so their light is mixed towards the fog by the distance of the surface behind them: a glow in front of distant fog fades with that fog. The pass reads the scene depth rather than the particle depth because the particle depth would fog the background behind a particle's soft edge by the particle's nearer distance, leaving an unfogged halo around every particle in front of fog.
- **MSAA**: depth reaches the pass single-sample at every sample count, sample 0 of the scene depth above one sample (see Shared Inputs), so a multisampled forward edge takes the fog of one of its samples.

## Outlines

`OutlinePass` (`postprocess/outline.rs`) is an `Ldr` pass that draws ink lines of a constant width in pixels around and inside the opaque PBR surfaces whose material asks for them, the screen-space replacement for inverted-hull outline meshes (#509). It runs while the settings hold an `outline`, from an enabled Outline component on the volume, so a frame without it keeps the tone-map-only plan. It reads `SceneInput::GBufferDepth`, `SceneInput::Normals` and `SceneInput::Material` to find edges and their styles, and `SceneInput::Depth` to hide lines behind other draws; it records one full-screen pass (`shaders/postprocess/outline.wgsl`, uniform `InkOutlineUniform`, not to be confused with the editor selection outline's `OutlineUniform`) and owns no texture.

- **Styles**: the Outline component holds a list of `OutlineStyle`s (ink `color`, silhouette `width`, `inner_width`, `inner_opacity`, `opacity`), and each material picks one with its `outline_style` (`u32`): 0 for no outline, the default, so every material saved before outlines draws none, and `N` for `styles[N - 1]`. There is no built-in style: an index only refers to the volume's list. The inspector labels the entries "Style 1", "Style 2", ..., the numbers materials use, through the `element_label` and `element_base` reflect attributes on `styles`; other lists keep "Element 0", "Element 1", .... The geometry pass writes the style into the G-buffer material target's alpha as `style / 255`, and the pass reads it back as `round(a * 255)` (see [G-Buffer](../rendering/gbuffer.md)). At most `MAX_OUTLINE_STYLES` (8) are drawn, a fixed-size array in the uniform; extra entries are ignored with a warning, once. A material whose style is past the end of the list draws no line, and the renderer warns once per material and style, naming the material.
- **Which style an edge draws in**: the nearer surface's. A foreground surface of style 0 hides ink from the surface behind it, so an emissive signal crossing outlined cover stays unoutlined. A styled object against farther unoutlined ground keeps its silhouette; nearby edges also cannot dilate ink onto a nearer style-0 surface. A style past the end of the list draws nothing.
- **Edges and their kinds**: an edge lies between two neighbouring pixels of the G-buffer. It is a **silhouette**, drawn at the style's `width` in full ink, when one pixel holds a surface and the other none (against the sky or the clear colour), when the two surfaces have different styles (touching or not), or when the distance jumps by more than `silhouette_threshold`: two different surfaces, one in front of the other. It is an **inner line**, drawn at `inner_width` with the ink mixed in at `inner_opacity`, when the distance jumps by more than `depth_threshold` but not the silhouette threshold (a small step within one surface), or when the geometric normals turn by more than `normal_threshold` degrees (a crease). The G-buffer holds no object identity, so whether two surfaces are different objects is read from the size of the jump; when that is ambiguous the edge is an inner line. Where a pixel is within reach of several edges, the strongest ink wins: a silhouette over an inner line.
- **Distance jump**: each side extrapolates its own surface one pixel across the edge, linearly in inverse view depth, which is linear in screen space across a plane, and the smaller of the two errors, relative to the nearer surface's inverse depth, is compared with the thresholds. A plane at any angle, the ground at a grazing angle included, predicts its next pixel exactly and draws no line; a crease bends the extrapolation only by its change of slope, which the normal test covers. Where both surfaces turn edge-on to the pixel's ray, as a rounded shape does towards its silhouette, the distance bends faster than either side predicts, so the jump is scaled by the larger `|n . ray|` of the two, at least 0.2: the thresholds are up to five times higher on an edge-on curve, and unchanged where either side faces the camera, as the ground behind a silhouette does.
- **Gentle curves**: the normal test reads the geometric normal (BA), the interpolated vertex normal, so normal maps draw no lines, and a smooth-shaded clay shape, whose normals turn a few degrees from pixel to pixel, draws none across its curve; flat-shaded hard edges, whose normals jump, do.
- **Width**: each width is rounded to a whole number of pixels from 1 to 8 (`outline_pixel_width`). Each pixel looks along its row, its column and both diagonals for edges within the widest line's reach: a line straddles its edge, `ceil(width / 2)` pixels on the nearer surface and `floor(width / 2)` on the farther one, so a one-pixel line lies on the object, not on the background. A diagonal step counts as `sqrt(2)` pixels, so a slanted edge gets about the width a straight one does. A line is the same number of pixels at every distance, unlike a hull, which thins with distance.
- **Colour**: the ink is an sRGB `Color32` converted to linear and drawn on display-linear colour, so it shows as its hex (`#121212` by default, the tank game's ink) whatever the exposure, the curve or the grading. `opacity`, times `inner_opacity` for an inner line, mixes it in, and alpha rises by the same amount, so a line over a transparent background still shows.
- **Thresholds**: `depth_threshold` (0.01), `silhouette_threshold` (0.04) and `normal_threshold` (40 degrees) are the component's, shared by every style: they say what the geometry is, while the styles say how it is inked.
- **Blending**: thresholds interpolate. Styles blend entry by entry, by index, over the longer of the two lists; an entry one side lacks blends from, or to, an invisible copy of the other side's entry (its values at opacity 0), so a style a local volume adds fades in and one it lacks fades out. Colours blend in linear space, widths as floats rounded when uploaded, opacities linearly. The neutral value an effect fades in from is every style at opacity 0. Profiles store the list as the component's settings.
- **Order**: after tone mapping, so the ink never goes through the curve: in the `Hdr` stage an ink of `#121212` would reach the screen darker under ACES, and bloom would treat it as light. After colour grading, so the grade does not lift or tint the ink, and before FXAA, whose edge detection then smooths the lines' stair steps like any other edge. It follows that fog does not fade distant lines; the Sunbaked haze fogs surfaces by at most about 15% at the Far framing, and full-strength lines read better there.
- **What draws lines**: only opaque PBR surfaces whose material picks a style. Edges come from the G-buffer's own depth, normals and material (see Shared Inputs), so meshes drawn with another shader, the skybox and particles, alpha-blended ones that write the scene depth included, draw none. Where one of them lies more than 1% nearer than the G-buffer surface in the scene depth, it hides the line there, as it hides the surface. An additive particle stays out of the scene depth, so a line behind it is drawn over its glow.
- **MSAA**: the G-buffer, its depth, normals and material are single-sample at every sample count, and the PBR surfaces shade at one sample (see Deferred Shading), so the edges are the same at one and four samples. Above one sample the scene depth that hides lines is sample 0, so a multisampled forward edge hides the line by its first sample.
- **Cost**: each pixel checks four directions, two edges per direction at a widest line of 2 pixels, loading each G-buffer pixel along a direction once (depth, normals and material). Headless, 400 cubes at 2062x932 on an RTX 2070, the pass adds about 0.6 ms a frame.

## Fog Of War

`FogOfWarPass` (`postprocess/fog_of_war.rs`) is an `Ldr` pass that darkens what a side cannot see, by the world position of the surface each pixel shows. It runs while the settings hold a `ComponentFogOfWar` (`component/post_process/fog_of_war.rs`) whose `sight` game code has set; in edit mode no game code sets it, so the plan is unchanged. It reads `SceneInput::Depth` and records one full-screen pass (`shaders/postprocess/fog_of_war.wgsl`).

- **Sight from game code**: what a side sees is game state, not a look. The component's `color` and `opacity` are authored on the volume like any effect, while `sight` (`FogOfWarSight`) is runtime state, `#[serde(skip)]` and `#[reflect_skip]`, that game code writes into the volume's component every frame; settings resolution copies it in place with the rest of the effect, reusing its storage. The sandbox writes it from the tank HUD's update hook (`sandbox/src/vision/sight.rs`).
- **Viewers and blockers**: a sight is a list of `FogOfWarViewer`s, each a ground position `(x, z)`, a distance fade (`fade_start`, `fade_end`, and `fade_curve` `k`: `1 - (e^(k t) - 1) / (e^k - 1)` over the fade, linear at 0) and a `weight` from 0 to 1 that scales its sight, so game code can fade a viewer in or out; and a list of `FogOfWarBlocker`s, ground rectangles (a centre and two half axes, so turned footprints are exact) that block sight. A viewer sees a ground point, as much as its fade and weight leave, when the line from the viewer to it leaves no blocker before reaching it into open ground: sight carries on through the run of blockers it meets that overlap or touch, within `FogOfWarSight::TOUCHING` (5 cm), so a blocker in sight and the blocks it touches are lit, and only the ground past the run's far side is in shadow. The side sees a point as well as its best viewer does. `FogOfWarSight::visibility` holds the same rules on the CPU for game code and the tests.
- **Exact, antialiased shadow sides**: lines of sight are traced per pixel, not looked up from rays, so a shadow's side is the exact line from the viewer past a blocker's corner and moves smoothly as the viewer does, with no ray spacing to snap to. Each pixel traces four lines, to the rotated-grid points `FogOfWarSight::PIXEL_SAMPLES` across its own ground footprint, and takes their mean (`FogOfWarSight::pixel_visibility`): a hard side covers part of the pixel, so it is antialiased at the pixel's scale without being softened. The footprint is the ground step to the neighbouring pixels, rebuilt from their depth, taking the shorter of the steps ahead and behind on each axis, so a pixel on a silhouette steps along its own surface rather than onto the ground behind it.
- **Depth**: each pixel's world position is rebuilt from the scene depth (`camera_view_position`, then `view_to_world`), and its fog comes from its ground position. A wall a viewer sees, which game code reaches through, is lit up to its top, while ground in its shadow stays dark wherever it lands on screen, including behind the wall's top in a tilted view. The sky lies at the far plane and is fogged by that far point.
- **Data and cost**: the viewers and blockers go to the GPU each frame in storage buffers, 32-byte viewers (`GpuViewer`) and 32-byte blockers (`GpuBlocker`: the centre, the rows of the matrix to the blocker's local space and their lengths), each holding at least one element; a flat blocker is left out. Each viewer's surroundings split into `SIGHT_SECTORS` (64) equal angular sectors, and a sector table lists, per viewer and sector, the blockers whose angular span seen from the viewer (the circle around the blocker's corners, widened by `TOUCHING`) overlaps it; a viewer within that circle sees the blocker in every sector. Each pixel loops over the viewers whose fade reaches one of its samples, and for each over the blockers of the sectors its four lines lie in (those within `asin(spread / reach)` of the line to its centre, every sector close to the viewer), reading each once for its four lines: a blocker whose footprint, widened by the samples' spread, misses the line to the pixel's centre is skipped, and the rest are traced four lines at a time in `vec4` lanes. A line of sight only meets blockers of its own sector, so the sectors change no pixel; they spare each pixel the blockers in other directions. Game code hands over only the blockers within reach of a viewer's faded-out range. In the sandbox arena (one tank, a vision range of 15 m faded out 12 m further) at 1920x1080, the pass took about 1.1 ms of GPU time testing every blocker in reach and about 0.75 ms with the sectors.
- **Colour**: the pixel mixes towards the sRGB `color` by `opacity * (1 - seen)` in sRGB-encoded colour, as a translucent overlay drawn over the image in egui would, and alpha passes through. A pixel the side sees fully is returned unchanged.
- **Order**: after the ink outlines, so lines in unseen ground darken with it, and before FXAA. Being a chain pass, it fogs the scene and not the HUD the game view draws over it, nor the editor overlays. Every view of the scene goes through it, so the editor's scene view fogs too while game code sets a sight.
- **Blending**: colour and opacity blend as usual and fade in from opacity 0; the sight is game state, so it switches to the volume's at 0.5 and a volume without one keeps the sight below.

## Depth Of Field

`DepthOfFieldPass` (`postprocess/dof.rs`) is an `Hdr` pass that reads `SceneInput::ParticleDepth`, so particles blur by their own distance (see Particles And Depth). It runs while the settings hold a `depth_of_field`, which they do only when the volume has an enabled Depth Of Field component, so a frame without it keeps the tone-map-only plan. It runs after fog and before bloom, so bloom spreads highlights that are already out of focus, as a lens would. It records two render passes:

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

`TonemapPass` is the `Tonemap` stage. It always runs: a missing or disabled volume, or a missing or disabled Tone Mapping component, leaves `ComponentTonemap::default()`, never untone-mapped output.

- Its uniform holds the exposure scale `2^EV` and the preset.
- It draws one full-screen triangle (`shaders/common/fullscreen.wgsl`, no vertex buffers).
- `TonemappingPreset::Aces` is Stephen Hill's fit of the ACES RRT and ODT (BakingLab, MIT): the ACES input matrix, the rational RRT/ODT curve, then the output matrix, clamped to `[0, 1]`. It maps linear 0.18 to about 0.106 and 1.0 to about 0.619. It is the default, so a scene or profile that does not name a preset, and every scene saved before `Neutral` existed, keeps it.
- `TonemappingPreset::Neutral` ("Neutral" in the inspector) is for content authored for a plain LDR pipeline, such as the Unity scene the RPG Poly demo recreates: a color whose channels are all in `[0, 1]` after exposure reaches the display unchanged, so an unlit color shows exactly at 0 EV. A brighter color is divided by its largest channel, which keeps its hue, then mixed toward white by `1 - 1 / (0.15 (peak - 1) + 1)`: Khronos PBR Neutral's highlight roll-off with its compression start moved to 1 and its toe offset dropped, both of which would change display-range color. A grey of 4 maps to white and `(2, 0.5, 0.1)` to `(1, 8/23, 4/23)`. Unlike ACES it has no shoulder below 1, so a lit scene clips sooner and wants a lower exposure or dimmer lights than under ACES.
- Alpha passes through unchanged, so views cleared to transparent, such as thumbnails, keep their transparency.

## Ambient Occlusion

`SsaoPass` (`postprocess/ssao.rs`) measures how much ambient light reaches each opaque PBR surface from the G-buffer's depth and normals, and the deferred lighting pass multiplies it into the ambient and image-based light only. Direct light, shadowed or not, is never darkened, so a crease a light shines straight into keeps that light. SSAO is authored and switched like every effect: an enabled Ambient Occlusion component on a volume, or a profile entry, resolved and blended into `PostProcessSettings::ambient_occlusion` (see Settings And Volumes). It is not a chain pass: `render_scene_base` resolves the settings before the geometry pass, runs SSAO right after it, and hands the occlusion to the lighting pass.

```text
geometry pass  PBR draws                 -> G-buffer, depth
occlusion      G-buffer depth + normals  -> occlusion (1/2)   visibility in r, view z in g
blur           occlusion                 -> blurred (1/2)     depth-aware 4x4
lighting pass  G-buffer + blurred        -> HDR target        ambient occlusion = material's * upsampled visibility
```

- **Reconstruction**: each occlusion texel takes its view-space position from the G-buffer's depth with the camera's `CameraPlanes` (linear depth and `tan_half_fov`), and its normal from the G-buffer's `normals` target. It decodes the geometric normal (BA), not the shading normal: the hemisphere is tested against the depth, which holds the geometry, so a normal-mapped normal would tip it into its own surface and darken flat walls. A back face drawn double-sided has its normal turned toward the camera.
- **Hemisphere**: up to 64 kernel offsets in the unit hemisphere around +z, generated once per sample count from a fixed seed. They cluster toward the center and keep at least 0.15 of their length along the normal, so a flat surface does not occlude itself. Each texel turns the kernel around its normal by one of 16 angles from a 4x4 tile in Bayer order, then scales it by `radius`.
- **Occlusion**: each sample is projected back to the screen and compared with the depth there. A surface more than `bias` in front of the sample occludes it, faded out by `smoothstep(radius / |dz|)` when that surface lies far in front of the pixel, so a foreground object does not darken the wall behind it. Samples off screen or where no surface was drawn do not count. Visibility is `(1 - occluded / samples) ^ intensity`.
- **Blur**: a 4x4 box over exactly one tile of the noise, which cancels the rotation pattern, weighting each texel by how close its view depth is to the center's (zero at a 10% difference).
- **Composite into the ambient term**: the lighting pass binds the blurred occlusion in group 1 (`occlusion_texture`) and calls `ssao_visibility` for each pixel it shades. That upsamples the visibility from the four nearest texels, weighted bilinearly and by how close their view depth is to the pixel's; a pixel whose four texels all lie on other surfaces, such as a thin object between them, keeps a visibility of 1. The visibility multiplies the material's own occlusion, and `pbr_lighting` applies the product to the sky light and the flat ambient term, as it already applied the material's occlusion.
- **Off**: with no Ambient Occlusion in the settings, or an intensity of 0 (the neutral value a local volume fades in from), the passes do not run, the targets are freed, and the lighting pass binds a single texel of visibility 1 instead. A shader without a pipeline turns SSAO off the same way, with one warning.
- **Targets**: two `Rg16Float` textures at half the G-buffer's size, rounded up. The pass creates them on the first frame SSAO runs, recreates them when the size changes, and frees them on resize and on every frame it does not run.
- **What it sees**: SSAO runs before the forward pass, the skybox and the particles draw, so the depth holds only the opaque PBR surfaces, the ones the normals describe, at every sample count: the G-buffer and its depth are single-sample (see [G-Buffer](../rendering/gbuffer.md#msaa)), and nothing is resolved for SSAO. So the caveat of Shared Inputs, a depth that holds a nearer non-PBR draw while the normals hold the PBR surface behind it, never reaches SSAO. Meshes drawn with another shader and particles neither occlude PBR surfaces nor are darkened: they draw over the lit image afterwards, with no ambient term SSAO could reach. The forward pass has no PBR shading left (see Deferred Shading), so there is no forward SSAO path, and no full-screen composite darkens their light.

Why the lighting pass reads the occlusion, rather than keeping the ambient light in a target of its own for a composite in the chain:

- **Cost**: the occlusion is two half-resolution passes and four texel loads in the lighting pass, one full-screen pass fewer than the old composite. A separate ambient target is a full-size `Rgba16Float` target the lighting pass writes every frame, plus a full-screen composite, and every forward draw would have to split its light the same way.
- **MSAA**: the G-buffer and its depth are single-sample at every sample count, so SSAO reads them directly. In the chain the depth is the multisampled scene depth, which needs a resolve, and holds forward draws and particles that the normals do not describe.
- **Settings and volumes**: the settings are resolved once per frame, at the camera, before the geometry pass, so global volumes, local volumes with their blending, and profiles drive SSAO as they drive the chain's passes. A blended radius, bias or intensity reaches the uniform the same frame, the sample count switches at a blend factor of 0.5, and an effect off below fades in from intensity 0.

## Bloom

`BloomPass` (`postprocess/bloom.rs`) is an `Hdr` pass. It runs while the settings hold a `bloom`, from an enabled Bloom component on the volume, so a frame without bloom keeps the tone-map-only plan. It records several render passes into the encoder:

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

`ColorGradingPass` (`crates/calyx_engine/src/render/postprocess/color_grading.rs`) is one `Ldr` pass that grades the tone-mapped image and then applies a vignette. It runs while the settings hold a `color_grading` or a `vignette`, from enabled Colour Grading and Vignette components on the volume, so a frame without them keeps the tone-map-only plan. A missing half reaches the shader as neutral values, which leave the color unchanged.

It runs on display-linear color, in this order:

- **White balance**: `temperature` (blue to orange, -1 to 1) and `tint` (green to magenta, -1 to 1) shift the white point a von Kries scale corrects in CAT02 LMS. The illuminant's chromaticity moves along the daylight locus for temperature and off it for tint, as in Unity's post-processing stack. The CPU folds linear sRGB to LMS, the scale and LMS back to linear sRGB into one matrix, relative to the scale at 0 and 0, so a neutral white balance is exactly the identity.
- **Contrast**: `pivot * (c / pivot)^contrast` per channel, a straight line through linear 18% grey (`0.18`) in log space. 1 is unchanged, 0 is flat mid grey.
- **Saturation**: a mix between the color and its Rec. 709 luminance (`0.2126, 0.7152, 0.0722`, linear sRGB). 1 is unchanged, 0 is grey.
- The graded color is clamped to `[0, 1]`.
- **Vignette**: with `d = |uv - 0.5| * intensity * 3` and `d.x` scaled by the aspect ratio so the shape is round, the factor is `(1 - d.d)^(smoothness * 5)`, clamped. The color is multiplied by `mix(color, 1, factor)`: unchanged at the centre, fading to `color` (black by default) towards the edges. `color` is an sRGB `Color32` converted to linear on the CPU.

Grading runs before FXAA (#400), as is usual: FXAA's edge detection then sees the final image, and grading does not change the contrast FXAA already smoothed. Both are `Ldr` passes, so the order is their registration order in `SceneRenderer::new`: tone mapping, colour grading, the ink outlines (see Outlines), then FXAA.

Alpha is read with the pixel and passes through, so transparent views such as thumbnails stay transparent.

## Anti-Aliasing

`FxaaPass` (`crates/calyx_engine/src/render/postprocess/fxaa.rs`) is an `Ldr` pass that runs FXAA 3.11 Quality after tone mapping, at the preset the component's `quality` names (#489). It runs while the settings hold an `anti_aliasing`, from an enabled Anti-Aliasing (FXAA) component on the volume, so a frame without it keeps the tone-map-only plan.

- `assets/shaders/postprocess/fxaa.wgsl` ports the PC quality path of Timothy Lottes' `FxaaPixelShader`: early exit below the local contrast threshold, horizontal or vertical edge choice, a search along the edge in both directions until its contrast ends, and the sub-pixel blend. It samples its input with a linear, clamp-to-edge sampler the pass owns.
- It runs after tone mapping because its luma edge detection needs display-range values. Its input is display-linear, so luma is the square root of linear Rec. 601 luma, close to the gamma-encoded luma the reference expects.
- `edge_threshold` (default 0.166) is the minimum contrast, relative to the brightest neighbour, treated as an edge. `subpixel` (default 0.75) is how much sub-pixel aliasing is removed. Both are the reference's defaults.
- `quality` (`FxaaQuality`) picks the reference's search preset and the absolute contrast floor below which nothing is an edge:

  | Quality | Preset | Search steps (pixels) | Reach each way | Floor |
  |---|---|---|---|---|
  | `Low` (default) | 12 | 1, 1.5, 2, 4, 12 | 20.5, sampled to 8.5 | 0.0833 |
  | `High` | 39 | 1, 1, 1, 1, 1, 1.5, 2, 2, 2, 2, 4, 8 | 26.5, sampled to 18.5 | 0.0312 |

  The search ends where it first samples past the end of the edge, and one that never does takes the full reach. `Low` stops sampling 8.5 pixels out, so on a long, nearly horizontal or vertical step a pixel further than that from the step's end gets no blend, while `High` samples one pixel at a time near the pixel and out to 18.5. `High`'s floor lets FXAA find dark, low-contrast edges `Low` leaves alone. On the RPG Poly demo at 2062x931, the FXAA pass takes 0.16 ms at `Low` and 0.23 ms at `High`.
- The pass carries the preset in its uniform (`FxaaUniform`: the floor, the step count and up to 12 step sizes) rather than in a pipeline variant per preset: one pipeline serves every volume, a blend that switches the quality never waits on a pipeline, and reading the steps from the uniform cost `Low` about 0.005 ms on the same view. `Low`'s uniform holds the constants the shader held before, so scenes saved without `quality` render as they did.
- Alpha is read from the pixel itself, never filtered, so transparent views such as thumbnails stay transparent.
- FXAA does not see editor overlays: they composite after the chain and keep their MSAA.

## G-Buffer

A geometry pass draws the opaque PBR meshes into a G-buffer that `SceneRenderer` owns, single-sample at the scene's size. [G-Buffer](../rendering/gbuffer.md) holds the pass, the MSAA decision, the debug view and the full channel mapping; the layout is:

| Target | Format | Channels |
| --- | --- | --- |
| `albedo` | `Rgba8UnormSrgb` | RGB base color texture times `base_color_factor`; A 1 where a surface was drawn |
| `normals` | `Rgba16Float` | RG view-space shading normal (normal map with vertex tangents and `normal_scale`), BA view-space geometric normal, both octahedral |
| `material` | `Rgba8Unorm` | R metallic (factor times texture B), G roughness (factor times texture G), B ambient occlusion (`ambient_occlusion` with the occlusion texture's R at `occlusion_strength`) |
| `emissive` | `Rgba16Float` | RGB `emission_color * emission_strength * emissive_texture`, linear HDR |

The chain hands the `normals` and `albedo` targets to the passes that ask for `SceneInput::Normals` and `SceneInput::Albedo` (see Shared Inputs). A G-buffer view (`SceneRendererOptions::gbuffer_view`) replaces the chain's output with one target.

## Deferred Shading

Opaque PBR meshes, the draws whose shader is `shaders/mesh/pbr`, alpha-cutoff materials included, shade only through the deferred path: the geometry pass writes them into the G-buffer and a full-screen lighting pass shades them. Every other draw takes the forward pass after lighting, against the deferred depth: a mesh with another shader, and any blended material added later.

The lighting pass shades through `pbr_lighting` in `assets/shaders/common/pbr_lighting.wgsl`. It binds group 0 (the camera, the sky light's irradiance, prefiltered environment and BRDF maps, and the environment uniform) and group 2 (the lights and the shadow maps), which `MeshRenderer::lighting_bind_groups` builds once per frame; the forward pass binds the same groups for its draws. Each light's direct term is dimmed by its shadow, looked up with the geometric normal; the sky light and the flat ambient term are dimmed by the material's occlusion times SSAO's, never by shadows.

`assets/shaders/mesh/pbr.wgsl` stays the PBR materials' shader: its id selects the batches the geometry pass draws, and its group 3 layout is the one material bind groups are built for. Its fragment stage, `pbr_surface` then `pbr_lighting`, is not drawn by any pass; it stays as the starting point of a forward shader for blended materials.

Pass order in `render_scene_base`, after the shadow pass:

```text
geometry pass    PBR draws -> G-buffer; clears and writes the depth, Less, draw order
SSAO             G-buffer depth + normals -> half-resolution occlusion, while it is on
lighting pass    full screen, G-buffer + depth + occlusion -> scene_texture_msaa (cleared first)
forward pass     every other draw, loading color and depth
object ids, skybox, particles; then finalize_scene: the chain and the overlays
```

- **Depth.** The geometry pass owns the depth: at one sample it draws the scene depth itself, and the lighting pass leaves it. Above one sample the G-buffer has its own single-sample depth (the multisampled scene depth cannot be attached beside single-sample targets), and the lighting pass writes it, through `frag_depth`, into every sample of the scene depth. The forward draws, the skybox and the particles then test against the deferred surfaces, and post-processing reads that depth.
- **Lighting pass.** `DeferredLightingRenderer` (`crates/calyx_engine/src/render/deferred_lighting.rs`) draws `shaders/deferred/lighting.wgsl` at one sample and `shaders/deferred/lighting_msaa.wgsl` above one; both include `shaders/deferred/lighting_common.wgsl`. Group 1 binds the four G-buffer targets, the depth and SSAO's occlusion (see Ambient Occlusion). A pixel whose albedo alpha is 0 is discarded, so it keeps the clear color for the skybox or a forward draw. Elsewhere the world position is rebuilt from the depth with `inverse_projection` and `inverse_view`, the normals are decoded and taken to world space, and the color is written with alpha 1: every fragment kept is opaque.
- **MSAA.** The G-buffer is single-sample, so above one sample the lighting pass writes each pixel's color into all of its samples: PBR edges are not multisampled, while forward draws, the skybox and particles still are. The editor's views and thumbnails render at one sample, and FXAA (Anti-Aliasing on the volume) smooths the edges.
- **Thumbnails.** The thumbnail renderer shades through the same passes, with shadows off; its transparent clear color stays where no surface is drawn.

## Settings And Volumes

A volume decides where post-processing applies and with what priority. Each effect's settings are their own component on the volume's game object. The volume and the effect components live in `crates/calyx_engine/src/component/`, the effects in `component/post_process/`, one file each, and hold authoring data only; the passes in `render/postprocess/` read them through `PostProcessSettings`.

```rust
pub struct ComponentGlobalPostProcessVolume { pub priority: i32, pub profile: AssetRef<PostProcessProfile> }  // 0, none

// Effect components, each `#[serde(default)]` and switchable (`can_disable`).
pub struct ComponentTonemap { pub preset: TonemappingPreset, pub exposure_ev: f32 }  // Aces, 0.0
pub enum TonemappingPreset { Aces, Neutral }  // default Aces
pub struct ComponentAmbientOcclusion { pub radius: f32, pub intensity: f32, pub bias: f32, pub sample_count: u32 }  // 0.5, 1.0, 0.025, 16
pub struct ComponentDepthOfField { pub focus_distance: f32, pub focus_range: f32, pub max_blur: f32 }  // 10.0, 10.0, 8.0
pub struct ComponentBloom { pub threshold: f32, pub knee: f32, pub intensity: f32, pub radius: u32 }  // 1.0, 0.5, 0.5, 6
pub struct ComponentColorGrading { pub contrast: f32, pub saturation: f32, pub temperature: f32, pub tint: f32 }  // 1, 1, 0, 0
pub struct ComponentVignette { pub intensity: f32, pub smoothness: f32, pub color: Color32 }  // 0.4, 0.2, black
pub struct ComponentAntiAliasing { pub edge_threshold: f32, pub subpixel: f32 }  // 0.166, 0.75
pub struct ComponentOutline {
    pub styles: Vec<OutlineStyle>,  // none: materials pick styles[outline_style - 1]
    pub depth_threshold: f32, pub silhouette_threshold: f32, pub normal_threshold: f32,  // 0.01, 0.04, 40 degrees
}
pub struct OutlineStyle {
    pub color: Color32, pub width: f32, pub inner_width: f32,  // #121212, 2.0, 1.0 pixels
    pub inner_opacity: f32, pub opacity: f32,  // 0.55, 1.0
}
pub struct ComponentFog {
    pub mode: FogMode, pub color: Color32, pub intensity: f32,  // Linear, (180, 196, 212), 1.0
    pub start: f32, pub end: f32, pub density: f32, pub opacity: f32,  // 0.0, 300.0, 0.01, 1.0
}
pub enum FogMode { Linear, Exponential }

// Runtime-only storage retained by each SceneRenderer; never saved.
pub struct EffectSlot { pub active: bool, pub effect: Box<dyn PostProcessEffect> }
pub struct PostProcessSettings {
    effects: HashMap<Uuid, EffectSlot>,
    // Retained volume queries, contributing-volume buffer and warned priorities.
}
// Passes borrow only the active effect of their component type:
let bloom: Option<&ComponentBloom> = settings.get::<ComponentBloom>();
```

- **Effect components**: each effect's settings struct is itself the component, with its own type UUID, so a pass reads exactly the values the inspector edits and #114 can blend the same types. None carries an `enabled` field: an effect is on when its component is present on the volume's game object and the scene holds it enabled (`Scene::is_component_enabled`, #415), or when the volume's profile has an enabled entry for it (see Profiles). The volume's own enabled state lives in the scene the same way.
- **Gathering**: each renderer owns one `PostProcessSettings` and calls `resolve_into(scene, camera_position)` on it before drawing. Resolution marks every slot inactive, copies the default tone mapping, copies the selected global volume and blends contributing local volumes. A volume borrows enabled profile entries under their read lock and registered effect components under the scene/registry borrows; it copies or blends each directly into its slot. An enabled component replaces its profile entry before blending, so a local override never blends the profile and component twice. A first-seen type gets a box through its reflected default and `ReflectPostProcessEffect`; later frames reuse that box. `resolve` and `from_game_object` remain one-shot inspection helpers, which allocate fresh settings.
- **Selection**: the enabled volume with the highest priority. On a priority tie the renderer warns once and uses the first found. With no enabled volume it uses `PostProcessSettings::default()`, tone mapping alone. Effect components on other game objects, or on a disabled or losing volume's object, are never read.
- **Passes**: `enabled` checks `settings.get::<Effect>()`, and `prepare` borrows the same value. Inactive slots return `None`, regardless of their retained contents. Tone mapping always runs and defaults when absent; SSAO borrows `ComponentAmbientOcclusion` before deferred lighting in the same way. The chain and passes read no scene data themselves.
- **Allocation lifetime**: warmed resolution retains its effect boxes and owned buffers, Legion query caches, warning-priority sets and local-volume sort buffer. The latter reserves for every local volume, so camera movement that makes more of them contribute does not grow it; sorting is in place and unstable, with object UUIDs making ties deterministic. The Outline copy operation reserves its eight supported styles even when the first volume has only one, preserving capacity through growing, shrinking and regrowing. Scene topology, new effect types, first-time warnings and asset loading can allocate; ordinary frames after that warm-up do not. The allocation regression includes profiles, all built-in effects, an independently registered effect, 64 local volumes, priority ties and one-to-eight style growth.
- **Assembly lifetime**: a slot can own a project-defined effect and its DLL vtable. `SceneRenderer::clear_post_process_settings` drops those values before the editor unloads the previous assembly. A subsequent render resolves fresh values against the current registries. Cached thumbnail renderers release effect values at the end of each scene job, before its guard permits assembly reload. Volume query caches are reset when the renderer changes scenes so previous play snapshots do not accumulate.
- **Inspector**: the volume and the effect components carry `category = "Post Processing"`, and the add-component menu lists components without a category first, then each category under its heading, in name order. Each effect edits with the default field inspectors and switches on and off through the component header's checkbox.
- **Saved scenes**: a volume saves `priority` and its `profile` id (nil without one; a scene saved before profiles loads with none), and each effect saves its fields under its own type UUID on the same object. The committed scenes were converted once from the old layout, a `settings` struct inside the volume; loading has no migration path. `sandbox/tests/saved_data.rs` fails if a committed scene or prefab still holds the old layout.
- Prefab mode and thumbnails render scenes with no volume, so they get the defaults with no special case.

### Local Volumes

```rust
pub struct ComponentLocalPostProcessVolume {
    pub shape: LocalVolumeShape,  // Box { half_extents: (5, 5, 5) }
    pub blend_distance: f32,      // 2.0, world units
    pub weight: f32,              // 1.0, in [0, 1]
    pub priority: i32,            // 0
    pub profile: AssetRef<PostProcessProfile>,  // none; see Profiles
}
pub enum LocalVolumeShape { Box { half_extents: Vec3 }, Sphere { radius: f32 } }



```

A local volume changes part of the look in a region and fades it in and out as the camera crosses its edge. Its effects are the same effect components, on its own game object. The chain still sees one resolved `PostProcessSettings`.

- **Camera position**: `settings.resolve_into(scene, camera_position)` takes the world position of the camera the view draws with. `render_scene_base` passes its `camera_transform`, so the editor viewport, the Game tab, the player and thumbnails each blend at their own camera.
- **Shape**: in the game object's space. A box is centred on the object and rotates and scales with it. A sphere is centred on the object, and its radius scales with the largest axis of the object's world scale, so it stays a sphere.
- **Blend factor** (`ComponentLocalPostProcessVolume::blend_factor`): `weight` inside the shape and on its surface, falling linearly to zero at `blend_distance` world units outside it, and zero beyond. A box measures from its nearest point, so the fade rounds its edges and corners. A blend distance of zero switches the volume at its surface.
- **Order**: blending starts from the global volume's resolved settings, the defaults without one; global selection is unchanged. `PostProcessSettings::local_volumes` lists the enabled local volumes with a factor above zero in ascending priority, ties in ascending object id with a warning once per priority, and each applies over the result of those before it, so the highest priority applies last.
- **Override per effect**: an effect component present and enabled on the volume overrides every one of its fields; there are no per-field override flags. One the volume lacks or holds disabled inherits the value below it. A disabled local volume takes no part.
- **Blending per field** (`render/postprocess/blend.rs`, `BlendEffect`): `f32` fields interpolate, colours interpolate in linear space, and integer and enum fields (bloom radius, SSAO sample count, the tone-mapping preset, the fog mode) switch to the volume's at a factor of 0.5.
- **Fade-in**: an effect off below and on in the volume fades in from its neutral values, the volume's own values with the strength at rest: SSAO, bloom and vignette intensity 0, fog and outline opacity 0, depth of field blur 0, and the colour grading defaults. FXAA has no neutral value and switches on at 0.5. Tone mapping is always on below a volume.
- **Gathering**: global and local volumes use the same borrowed effect path. A local volume without tone mapping leaves the always-active tone mapping below it alone; absent or disabled effects inherit the value below.
- **Gizmos**: the editor draws a local volume's shape, and more faintly the outer edge of its blend distance: a sphere of radius plus blend distance, or the box grown by the blend distance on every side, which traces the faces of the rounded blend region.

### Profiles

```rust
// assets/post_process_profile/, saved as `.cxpostfx` JSON.
pub struct PostProcessProfile { entries: Vec<ProfileEntry> }  // at most one entry per effect type
pub struct ProfileEntry { pub enabled: bool, pub settings: Box<dyn PostProcessEffect> }

// component/post_process/: the API every effect component implements and
// lists in its #[reflect(...)], so the type registry knows the effect types.
#[reflect_trait]
pub trait PostProcessEffect: Component {
    fn copy_from(&mut self, source: &dyn PostProcessEffect);
    fn blend_into(&mut self, target: &dyn PostProcessEffect, factor: f32);
    fn reset_neutral(&mut self) -> bool;
}

// Both volumes gain:
pub profile: AssetRef<PostProcessProfile>,  // nil: no profile
```

```json
{
  "entries": [
    { "type": "68ac12ac-fc9a-489e-b305-363eaf593966", "enabled": true,
      "settings": { "preset": "Aces", "exposure_ev": 1.5 } },
    { "type": "7e09858d-fd71-4729-bb90-8ed76c0bda9a", "enabled": false,
      "settings": { "intensity": 0.75 } }
  ]
}
```

A profile is a look saved as an asset, which any number of global and local volumes, in any scene, reference.

- **Entries**: a list of entries, at most one per effect, each keyed in the file by its effect component's type UUID (`type`), with its own `enabled` flag and the component's serialized fields (`settings`), as a scene saves the component. The settings are a value of the effect component itself, so the passes read profile values exactly as component values. A missing `enabled` is on, and missing settings fields take the component's defaults. An empty profile saves as `{ "entries": [] }`.
- **Effect types**: the components registered with the `PostProcessEffect` reflect trait (`PostProcessProfile::effect_types`, from the type registry); a project component can be one too. Loading deserializes each entry through the component registry's prototype for its type, as a scene loads components, then through `ReflectPostProcessEffect`. An entry whose type is unknown or not an effect, whose settings do not deserialize, or whose type an earlier entry has, is skipped with a warning, and the rest of the profile loads. Adding an effect therefore needs no change to the profile or its file format.
- **Borrowed access**: `PostProcessProfile::enabled_effects` iterates enabled entries as `&dyn PostProcessEffect`; `effect::<T>()` borrows one enabled entry by its registered type UUID. Neither copies settings or names individual effect types. Resolution reads the cached asset handle directly before falling back to a load, avoiding a cloned asset metadata/path on every frame.
- **In-place API**: `PostProcessEffect::copy_from` copies matching concrete effects into existing storage. `blend_into` clamps its factor to 0..1 and uses the same float, colour and discrete-field rules as `BlendEffect`; a mismatched concrete type is a programming error and panics. `reset_neutral` preserves the effect's non-strength settings while resetting its strengths, returning `false` without changing FXAA because it has no neutral value. A resolver copies the target, resets it to neutral and blends toward it to fade an inactive effect in; when reset returns `false`, it switches on at half weight. Tone mapping retains its always-on default handling in the resolver. Each built-in component registers the reflected trait and derives `PostProcessEffect` (reexported beside the trait) to dispatch to its typed `BlendEffect` operations. Outlines specialize those operations to copy, blend and reset their style list in existing capacity instead of allocating a new list each time.
- **Adding an effect**: define and register its component with `PostProcessEffect`, implement its `BlendEffect` operations (including storage reuse for owned buffers), and register its pass in the chain. The profile, generic gather/resolver and settings require no per-effect fields or dispatch. `PostProcessSettings::with(effect)` constructs explicit pass settings outside a scene. Caching unchanged settings and dense type indices are not part of this API.
- **Precedence** (the shared volume gather path): the volume's profile entries, each enabled entry on and each disabled one off, then each effect component present and enabled on the volume's object replaces its effect's entry whole. A component the object lacks or holds disabled leaves the profile's entry; to switch off a profile's effect on one volume, give it the component and switch its entry off in the profile, or override the effect with neutral values. A volume without a profile, or whose profile does not load (warned once), reads its components alone, as before profiles. Tone mapping still falls back to its defaults when neither sets it. An object with both a global and a local volume uses the global volume's profile.
- **No blending or layering**: a profile never layers over another. Local volumes blend the settings each volume resolves to (Local Volumes), so a profile reaches the blend as that volume's effects.
- **Live edits**: `resolve_into` reads the loaded profile every frame through the asset registry, so an edit in the profile's inspector, a change of a volume's `profile` field and a hot reload of the file all show on the next frame, in every view and every volume that uses it. Saving the profile writes its file; an external change to the file hot reloads it in place.
- **Editor**: the asset inspector draws each entry as an effect component, with the header checkbox for `enabled`, Remove in the header's menu and the default field inspectors, and Add Effect adds an absent registered effect with its defaults; Save in the asset's menu writes the file. The content browser's Create New > Post Process Profile writes an empty profile. A volume's inspector shows Create Profile, which asks for a file, writes a profile with the volume's look (its current profile's entries with the object's effect components copied over them; a disabled component becomes a disabled entry only where the profile has none), points the volume at it and removes the effect components from the object, as one undo step. The look is unchanged, and editing the profile now changes it. `calyx_ctl profile create/get/set` and `asset save` drive the same paths; `profile get` and `set` use the file format.

## Shaders

- `assets/shaders/common/fullscreen.wgsl`: the shared full-screen triangle, `fullscreen_vertex(index)`. `assets/shaders/common/fullscreen_pass.wgsl` includes it and declares the `vs_main` that calls it, so a shader that draws the plain triangle includes the entry point and declares only its fragment stage: the post passes, the overlay composite, the selection outline and 2D mip generation. The editor grid adds outputs of its own, so it includes `fullscreen.wgsl` and declares its own vertex stage. Every full-screen draw takes its pipeline options from `PipelineOptions::fullscreen_builder()`, which sets only the color target, depth and sample count; the engine has no full-screen quad mesh.
- `assets/shaders/postprocess/tonemap.wgsl`: the tone-map pass.
- `assets/shaders/postprocess/ssao.wgsl` and `ssao_blur.wgsl`: the SSAO passes. `ssao_common.wgsl` holds their uniform, the view-space reconstruction, the depth weight they share and `ssao_visibility`, the upsample the deferred lighting pass includes.
- `assets/shaders/postprocess/bloom_prefilter.wgsl`, `bloom_downsample.wgsl`, `bloom_upsample.wgsl` and `bloom_composite.wgsl`: the bloom passes. `bloom_filters.wgsl` holds the 13-tap downsample and the tent upsample they share.
- `assets/shaders/postprocess/color_grading.wgsl`: the colour grading and vignette pass.
- `assets/shaders/postprocess/fxaa.wgsl`: the FXAA pass.
- `assets/shaders/postprocess/outline.wgsl`: the ink outline pass.
- `assets/shaders/postprocess/camera_planes.wgsl`: `CameraPlanes` and its depth, ray and position helpers, shared by the fog, SSAO, depth of field and outline shaders.
- `assets/shaders/postprocess/fog.wgsl`: the fog pass.
- `assets/shaders/postprocess/fog_of_war.wgsl`: the fog of war pass.
- `assets/shaders/postprocess/depth_resolve.wgsl`: copies sample 0 of the multisampled depth.
- `assets/shaders/postprocess/dof_coc.wgsl` and `dof_blur.wgsl`: the depth of field passes. `dof_common.wgsl` holds their uniform, the depth linearization and the circle of confusion.
- `assets/shaders/editor/overlay_composite.wgsl`: blends the resolved overlay target over the scene texture.
- `pbr.wgsl` drops Reinhard, gamma 1.4 and the `* 5.0` on albedo. It adds linear HDR emission, `emission_color * emission_strength`, after the lighting. A material saved before those fields existed reads zero strength and does not emit.
- `skybox.wgsl` drops its exposure and gamma and scales the sky by the sky light's intensity, the same factor that scales its image-based lighting.
- Particles already write linear color. `shaders/particles/particle_billboard.wgsl` holds the billboard vertex stage that `particles.wgsl` and the depth-only `particles_depth.wgsl` share.

Post shaders use `//#include` modules, which carry over unchanged to the `calyx_shader` crate.

## Callers

The editor viewport, the Game tab, the thumbnail generator and the standalone player all call `render_scene_base` then `finalize_scene`, so each gets the chain with no change. The viewport and the Game tab render the same scene, so they select the same settings.

## Calibration

Removing the per-shader curves and the `* 5.0` changes every scene's brightness. Scenes are recalibrated through data, never a shader constant: a global volume's exposure and the scene's light intensities.

- **Sandbox scene**: lit only by its sky before, with dark materials (the arena floor's albedo is 0.13 with ambient occlusion 0.3) that the old curve lifted to mid grey. Exposure alone cannot bring them back: the sky and the image-based light it casts scale together, so any EV that lifts the floor blows out the sky. The scene gains a `Sun` directional light (intensity 5, pitched 60 degrees down), which lights the floor without brightening the sky, and a `Post Process` global volume at 0 EV, the knob for later tuning.
- **Thumbnails**: the preview lights rise to 6 (directional), 0.8 (ambient) and 0.8 (point) so prefab thumbnails read as before at the default 0 EV. The thumbnail disk cache key gains a render version, so thumbnails cached under the old curve render again.

The grid, gizmos and outline composite after post (#379, see Editor Overlays), so they keep their authored colors. Editor clear colors are still tone mapped.

- **Showcase scene**: `sandbox/assets/showcase/post_processing/post_processing.cxscene` (#378) is the fixed reference for the look: rough, glossy, metallic and emissive materials (`showcase/post_processing/materials/` and `shared/materials/`, the latter also used by the main showcase), the sunset sky at intensity 1, a sun at 2.5, four point lights a decade apart, from 0.22 to 220 (named `Point 5` to `Point 5000` for their intensities under the falloff before #453), the campfire particles and a global volume at 0 EV.

## Testing

Headless tests in `crates/calyx_engine/src/render/postprocess/`:

- The plan orders passes by stage, then insertion order, and ping-pongs two pool targets.
- A tone-map-only chain allocates no pool target, and a chain of three passes allocates two.
- Two non-commuting test passes produce the result of their declared order.
- Tone mapping preserves alpha.
- ACES maps known HDR inputs to the reference fit within tolerance.
- The neutral preset leaves display-range colors and their alpha unchanged at 0 EV, still applies exposure, and maps a bright grey to white and a bright orange to the worked roll-off value. In `scene_renderer.rs`, an unlit color reaches the screen as its own sRGB encoding under it, and not under ACES. The preset switches at a blend factor of 0.5 between volumes (`local_volume_tests.rs`), and a profile's neutral entry round-trips and selects the curve (`post_process_profile/tests.rs`).
- The display-encoding test above.
- Depth reaches a pass that requests it, single-sample at one and four samples; velocity reports unavailable.
- A pass that requests normals or albedo is planned with them, and a plan without such a pass reads neither.
- In `scene_renderer.rs`, a pass that requests normals and albedo gets the G-buffer's own single-sample targets, holding a turned plane's normal and base color, at one and four samples, and no depth it did not ask for.
- Volume selection picks the highest enabled priority, warns on a tie, and falls back to the defaults. A volume with no effect components resolves to tone mapping alone, only the selected volume's effect components are gathered, a disabled tone-mapping component falls back to the defaults, and adding, disabling or removing each effect component switches its pass on or off in the renderer's plan.
- SSAO (`ssao.rs`) leaves a flat plane facing the camera unoccluded, occludes it where its G-buffer normal says it is turned, darkens an inside corner most at its crease, and leaves pixels with no surface at visibility 1, at a cleared or skybox depth. Its targets are half the G-buffer's size and freed on frames it does not run, an intensity of 0 runs nothing, adding, disabling and removing the component switches it in the renderer while the chain's plan stays tone mapping alone, and a partial component loads with the defaults.
- Depth of field (`dof.rs`): `CameraPlanes` inverts the render projection; pixels at the focus distance are unchanged; far pixels blur more than pixels near focus, and pixels in front of it blur too; a blurred background stays behind a sharp edge while a blurred foreground spreads over it; the pass reads depth at one and four samples with the same result; with it disabled the plan is the tone-map-only plan and its texture is not allocated, follows the chain's size, and is freed when disabled and on resize. A partial component loads with the defaults.
- Bloom (`bloom.rs`): a single pixel above the threshold spreads energy into its neighbours, fading with distance, and keeps alpha; an image entirely below the threshold passes through unchanged; a flat image of 20,000 blooms to the expected value without overflowing the half-float mips; a composite past the half-float maximum saturates at it; with bloom disabled the renderer's plan is the tone-map-only plan; the mip chain follows the chain's size and the radius and is freed on release; the chain releases a pass's targets on resize and on frames the pass does not run.
- Colour grading (`color_grading/tests.rs`): neutral settings return the input, zero saturation gives grey at the input's luminance, contrast keeps mid grey, positive temperature warms and positive tint adds magenta, the vignette darkens the corners and leaves the centre unchanged, and alpha is preserved. With both effects off the pass is left out of the plan, the scene renderer plans it before FXAA, and partial components load with the defaults.
- Local volumes (`local_volume_tests.rs`): the blend factor inside, at the edge, within the blend distance and outside a box, a rotated and scaled box and a sphere, and with no blend distance; a factor of zero changes nothing; overlapping volumes apply in ascending priority, and ties in object id order with the tie reported; an absent or disabled effect inherits the value below; an effect off below fades in from its neutral values and FXAA switches on at 0.5; integer and enum fields switch at 0.5 and colours blend in linear space; a disabled volume takes no part; the scene renderer resolves at the camera it draws with; and a volume saves and loads with its scene.
- Fog (`fog/tests.rs`): a plane facing the camera fades to the fog colour by its distance along each pixel's ray, linear and exponential, at the expected amount, and keeps alpha; a surface before linear fog starts is unchanged; the background, at a cleared or skybox depth, takes the fog colour under linear fog that ends before the far plane and the far plane's fog under exponential fog; in a rendered scene the far of two identical cubes is closer to the fog colour than the near one, at one and four samples, while without fog they look the same; the renderer runs fog first in the `Hdr` stage, before depth of field and bloom; a partial component loads with the defaults. Fog blends between a global and a local volume and fades in from opacity 0 (`local_volume_tests.rs`), and a profile's fog entry round-trips and switches fog on (`post_process_profile/tests.rs`).
- Fog of war (`fog_of_war/tests.rs`, `component/post_process/fog_of_war/tests.rs`): from a tilted camera, every pixel of a plane is fogged by the CPU visibility of the ground footprint it shows, with two viewers and an axis-aligned and a turned blocker, and clear, fading and fully fogged pixels; the same pixel is clear on a near surface in sight, such as a crate's top, and fogged on a far one in its shadow; a side with no viewers fogs everything fully; without a sight the pass does not run; the renderer runs it after the outlines and before FXAA; packing turns blockers into the shader's local frames and leaves flat ones out; each viewer lists a blocker in the sectors its angular span overlaps, every sector for a viewer standing in it, and every line from the viewer through a blocker points into a sector listing it; a ring of blockers all around a viewer shades as the CPU rules say; the uniform, viewer and blocker match the shader's layout. On the CPU, a blocker is seen and the ground just behind it is not, a shadow side is the line past the blocker's corner, sight carries through touching blockers but not across a gap, a turned blocker shades its own footprint, a viewer inside a blocker sees out to its side only, the fade follows its exponential, a viewer's weight scales its sight, the side takes its best viewer, a pixel averages its samples across a shadow side, the sight is never saved, and settings copy it in place.
- FXAA smooths a hard diagonal edge and keeps each pixel's alpha, leaves a flat colour and its alpha unchanged, and leaves the plan unchanged while disabled. A partial component, and one saved before `quality`, loads with the defaults. `Low` is preset 12 and `High` preset 39, and `FxaaUniform` matches the shader's layout. On a long, nearly horizontal edge with one step, `High` smooths the pixels 10 to 18 pixels from the step that `Low` leaves hard. The quality switches at half weight between volumes while the thresholds interpolate (`local_volume_tests.rs`), and saves and loads with a scene's volume and through a profile entry (`post_process_profile/tests.rs`).
- Outlines (`outline/tests.rs`): a cube against the background gets a line of exactly the set width around it, 1 to 4 pixels, straddling the edge with the odd pixel on the cube, across the middle row and down the middle column, at one and four samples, and none on its flat face; a material of style 0 draws nothing, and a style past the end of the list draws nothing and warns, naming the material; an edge takes the nearer surface's style, style-0 foreground hides farther ink, and an emissive style-0 streak crossing outlined cover stays clean even where a wide background edge reaches toward it; a change of style between touching coplanar surfaces is an edge, and one style across them is not; a ground slab filling the view, seen steeply and at a grazing angle, gets none; a box on the ground gets an inner line along the crease at the inner width and half the ink, a full 2-pixel silhouette along its far top edge, and nothing on its face, its top or the ground in front; a smooth sphere gets only its silhouette; the renderer runs the pass after colour grading and before FXAA; the effect is off by default with no style, a partial component loads with the defaults, widths round to whole pixels from 1 to 8, only the first 8 styles are drawn, and `InkOutlineUniform` matches the shader's layout. In `scene_renderer.rs`, an unlit forward box and an alpha-blended particle on the ground draw no line while a PBR box beside them is outlined, at one and four samples. Outline styles blend entry by entry between a global and a local volume, an added style fading in from opacity 0 and a missing one fading out, and the effect fades in from opacity 0 (`local_volume_tests.rs`); a profile's outline entry round-trips its style list and switches outlines on (`post_process_profile/tests.rs`); a material's `outline_style` defaults to 0, loads as 0 from a material saved before it, and round-trips through the material file (`assets/material.rs`).
- Camera planes (`tests.rs`): the WGSL `CameraPlanes` has the Rust struct's size and offsets, and on the GPU its helpers give the linear depth, inverse depth, view-space position, distance along the ray and projection the fog, SSAO and depth of field shaders computed with their own formulas before.

Headless tests in `crates/calyx_engine/src/render/overlay/` and `scene_renderer.rs`:

- The deferred lighting matches known radiances in the HDR target, at one and four samples: a rough grey plane under an overhead sun reflects 3 (0.96 * 0.5 + 0.04 / 4) / pi, under a point light 2 units up 5 * `light_distance_attenuation(2, 10)` * 0.49 / pi; a point in a box's shadow is unlit and an unshadowed one gets the Cook-Torrance radiance worked out for its view and light directions; emission shows through an alpha-cutoff hole, a kept cutout covers it with its own emission, and the clear color stays where nothing is drawn.
- SSAO darkens the ambient light of a crease and keeps its direct light, at one and four samples: the sun shining straight into it reflects the same radiance with SSAO on and off, the ambient light alone is darker with it on, open ground keeps its ambient light, and both together reflect the direct light plus the darkened ambient light. An unlit forward mesh in the crease is the same, in the HDR target and on screen, with SSAO on and off.
- A mesh with another shader keeps the forward pass: in front of a deferred box it covers it and behind one it is hidden, at one and four samples, and only the PBR box enters the G-buffer.

- A half-covered, anti-aliased overlay edge pixel composites as premultiplied alpha, without darkening.
- Enabled overlays record in order, and the overlay target exists only while one is enabled.
- The selection outline keeps its exact authored color at +3 EV, at one and four samples, and picking is unchanged.
- Views without overlays allocate no overlay target.
- A particle focused by depth of field in front of the far-plane background is not blurred, alpha-blended and additive, at one and four samples (`a_particle_in_focus_is_not_blurred_as_the_background_behind_it`).

## Roadmap

- **#113** adds effects as `Hdr` and `Ldr` passes, each with its own effect component (#416).
- **#114** adds local volumes that blend `PostProcessSettings` before the chain runs (see Local Volumes).
- **#115** adds editor UI for post-processing profiles.
- **#378** adds a showcase scene that validates the look end to end.
