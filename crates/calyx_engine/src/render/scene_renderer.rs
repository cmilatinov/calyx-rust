use super::RenderContext;
use crate::assets::error::AssetError;
use crate::assets::mesh::{Instance, Mesh};
use crate::assets::skybox::Skybox;
use crate::assets::texture::Texture;
use crate::assets::{Asset, AssetId, AssetRegistry, BackgroundLoad};
use crate::component::{ComponentAmbientOcclusion, ComponentOutline, ComponentSkyLight};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::Transform;
use crate::profiler::GpuScope;
use crate::render::asset_render_state::{AssetRenderState, LockedAssetRenderState};
use crate::render::draw_collection::{register_object_id, DrawCollection, DrawListElement};
use crate::render::overlay::{OverlayCompositor, OverlayContext, OverlayPass};
use crate::render::postprocess::{
    drawn_outline_styles, BloomPass, CameraPlanes, ChainPlan, ChainTargets, ColorGradingPass,
    DepthOfFieldPass, FogOfWarPass, FogPass, FxaaPass, OutlinePass, PostProcessChain,
    PostProcessSettings, SceneInput, SsaoInputs, SsaoPass, TonemapPass,
};
use crate::render::render_utils::RenderUtils;
use crate::render::static_batch::outside_view;
use crate::render::{
    Camera, DeferredLightingRenderer, DrawBatch, GBufferRenderer, GBufferTargets, GBufferView,
    GizmoRenderer, GridRenderer, LightManager, MeshRenderDefaults, MeshRenderTargets, MeshRenderer,
    OutlineRenderer, ParticleRenderer, ParticleSimulation, PipelineOptions, ShadowCamera,
    ShadowFrameStats, ShadowRenderer, SkyboxRenderer,
};
use crate::scene::Scene;
use crate::utils::TypeUuid;
use egui::Color32;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use legion::{Entity, IntoQuery};
use nalgebra_glm as glm;
use nalgebra_glm::Mat4;
use rapier3d::pipeline::DebugRenderPipeline;
use std::collections::HashMap;
use std::default::Default;

use std::sync::{mpsc, Arc};
use uuid::Uuid;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CameraUniform {
    pub projection: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub inverse_projection: [[f32; 4]; 4],
    pub inverse_view: [[f32; 4]; 4],
    pub near_plane: f32,
    pub far_plane: f32,
    pub viewport_size: [f32; 2],
}

impl CameraUniform {
    /// The uniform of a view through `projection` from `view` (world to view
    /// space), with its inverses, its clip planes and the size in pixels of
    /// the target it renders into.
    pub fn new(
        view: &Mat4,
        projection: &Mat4,
        near_plane: f32,
        far_plane: f32,
        viewport_size: [f32; 2],
    ) -> Self {
        Self {
            projection: (*projection).into(),
            view: (*view).into(),
            inverse_projection: glm::inverse(projection).into(),
            inverse_view: glm::inverse(view).into(),
            near_plane,
            far_plane,
            viewport_size,
        }
    }
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            projection: Mat4::identity().into(),
            view: Mat4::identity().into(),
            inverse_view: Mat4::identity().into(),
            inverse_projection: Mat4::identity().into(),
            near_plane: 0.0,
            far_plane: 0.0,
            viewport_size: [1.0, 1.0],
        }
    }
}

/// What the view tells component gizmos about the editor around it, read
/// through [`crate::render::Gizmos`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GizmoOptions {
    /// The game view's aspect, which a camera's frustum gizmo takes; 16:9 when `None`.
    pub game_view_aspect: Option<f32>,
    /// The game object selected in the editor, for gizmos that only draw while
    /// their object, or one related to it, is selected (a camera path's).
    pub selected: Option<Uuid>,
}

/// Scene renderer feature toggles and default clear settings.
pub struct SceneRendererOptions {
    /// Whether to draw the editor grid.
    pub grid: bool,
    /// The debug gizmos to draw, with what the view tells them, or `None`
    /// to draw no gizmos.
    pub gizmos: Option<GizmoOptions>,
    /// Clear color used for the scene color target.
    pub clear_color: Color32,
    /// Mesh face-culling mode used for scene geometry.
    pub mesh_cull_mode: Option<wgpu::Face>,
    /// MSAA sample count used for offscreen scene rendering.
    ///
    /// Values above one must be supported by the adapter for `Rgba16Float`
    /// color and `Depth32Float` depth; WebGPU guarantees four.
    pub samples: u32,
    /// Lighting this view uses in place of the scene's sky lights, such as
    /// the editor's prefab mode. It belongs to the view, never to the scene.
    pub environment: Option<SceneEnvironment>,
    /// Whether lights that cast shadows get them. Off, no shadow pass runs
    /// and no shadow map is allocated, as thumbnails need.
    pub shadows: bool,
    /// The G-buffer target the view shows in place of the lit image, or
    /// `None` for the lit image. The G-buffer is the one the frame shades
    /// from.
    pub gbuffer_view: Option<GBufferView>,
    /// The name this view's CPU scopes and GPU passes go under in the
    /// profiler.
    pub profile_name: &'static str,
    /// Whether the view draws the object id pass, which picking and the
    /// selection outline read. A view nobody picks in, as the player's, turns
    /// it off and saves the pass.
    pub object_ids: bool,
}

/// Lighting a view supplies itself instead of taking it from the scene: an
/// environment map for image-based lighting, and whether to draw it as the
/// sky.
#[derive(Clone)]
pub struct SceneEnvironment {
    /// The environment map that lights the scene.
    pub skybox: Ref<Skybox>,
    /// Multiplier on the environment map's light, as a sky light's intensity.
    pub intensity: f32,
    /// Whether to draw the environment map behind the scene. When `false`
    /// the clear color shows where nothing is drawn.
    pub draw_sky: bool,
}

impl Default for SceneRendererOptions {
    fn default() -> Self {
        Self {
            grid: false,
            gizmos: None,
            clear_color: Color32::TRANSPARENT,
            mesh_cull_mode: Some(wgpu::Face::Back),
            samples: 0,
            environment: None,
            shadows: false,
            gbuffer_view: None,
            profile_name: "Scene renderer",
            object_ids: true,
        }
    }
}

/// Error returned when a [`SceneRenderer`] or one of its sub-renderers cannot
/// be created.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SceneRendererError {
    /// A shader, mesh or texture the renderer draws with failed to load.
    #[error("scene renderer asset `{name}` failed to load")]
    AssetLoad {
        /// Registry name or relative path of the asset.
        name: String,
        /// The registry's load error.
        #[source]
        error: AssetError,
    },
    /// A shader or built-in asset the renderer draws with does not exist.
    #[error("scene renderer asset `{name}` is missing")]
    MissingAsset {
        /// Registry name or relative path of the asset.
        name: String,
    },
}

impl SceneRendererError {
    /// Loads the asset `name` from the registry, or reports why it did not load.
    pub(crate) fn load<A: Asset + TypeUuid>(
        context: &ReadOnlyAssetContext,
        name: &str,
    ) -> Result<Ref<A>, Self> {
        context
            .registries
            .assets
            .read()
            .load::<A>(name)
            .map_err(|error| Self::AssetLoad {
                name: name.to_string(),
                error,
            })
    }

    /// Returns a built-in asset the registry returned, or reports it missing.
    fn require<T>(asset: Option<T>, name: &str) -> Result<T, Self> {
        asset.ok_or_else(|| Self::MissingAsset {
            name: name.to_string(),
        })
    }
}

struct PendingObjectIdReadback {
    receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    object_ids: Arc<[Uuid]>,
    pixel: (u32, u32),
}

#[derive(Clone, Copy)]
struct CompletedObjectPick {
    pixel: (u32, u32),
    game_object: Option<Uuid>,
}

struct SceneRendererAssets {
    cube: Ref<Mesh>,
    black_texture_2d: Ref<Texture>,
    black_texture_cube: Ref<Texture>,
    white_texture: Ref<Texture>,
}

/// High-level scene renderer that prepares assets and renders the scene into an
/// offscreen texture.
pub struct SceneRenderer {
    asset_context: ReadOnlyAssetContext,
    default_assets: SceneRendererAssets,
    options: SceneRendererOptions,
    scene_texture: Texture,
    scene_depth_texture: Texture,
    scene_texture_msaa: Texture,
    /// Single-sample resolve of `scene_texture_msaa`, the post-processing
    /// chain's input. `None` at one sample, where the scene target is
    /// single-sample and is the input itself.
    hdr_resolved: Option<Texture>,
    scene_object_id_texture: Texture,
    scene_object_id_depth_texture: Texture,
    scene_object_id_readback: wgpu::Buffer,
    mesh_renderer: MeshRenderer,
    grid_renderer: GridRenderer,
    skybox_renderer: SkyboxRenderer,
    camera_uniform_buffer: wgpu::Buffer,
    light_manager: LightManager,
    shadow_renderer: ShadowRenderer,
    gbuffer_renderer: GBufferRenderer,
    /// At one sample, a copy of the scene depth taken right after the
    /// geometry pass, the G-buffer's depth before any forward draw, skybox or
    /// particle writes it. Kept only while the chain reads
    /// [`SceneInput::GBufferDepth`]; above one sample the G-buffer has a
    /// depth of its own.
    gbuffer_depth_copy: Option<Texture>,
    /// Ambient occlusion from the G-buffer, read by the lighting pass.
    ssao: SsaoPass,
    deferred_lighting: DeferredLightingRenderer,
    gizmo_renderer: GizmoRenderer,
    outline_renderer: OutlineRenderer,
    particle_renderer: ParticleRenderer,
    assets: AssetRenderState,
    /// The draws [`Self::render_scene_base`] collects for this view alone,
    /// made on its first use: a view handed a shared [`DrawCollection`]
    /// never needs one.
    own_draws: Option<DrawCollection>,
    object_ids: Arc<[Uuid]>,
    object_ids_build: Vec<Uuid>,
    object_id_lookup: HashMap<Uuid, u32>,
    pending_object_id_readback: Option<PendingObjectIdReadback>,
    completed_object_pick: Option<CompletedObjectPick>,
    selected_game_object: Option<Uuid>,
    hovered_game_object: Option<Uuid>,
    sky_light_intensity: f32,
    post_process: PostProcessChain,
    post_process_settings: PostProcessSettings,
    last_post_process_plan: ChainPlan,
    overlay: OverlayCompositor,
}

impl SceneRenderer {
    /// Creates a scene renderer for `context` with `options`.
    ///
    /// The renderer draws on `context`'s render context, headless or not. It
    /// fails when a shader, mesh or texture it draws with does not load.
    pub fn new(
        context: &ReadOnlyAssetContext,
        mut options: SceneRendererOptions,
        initial_size: (u32, u32),
    ) -> Result<Self, SceneRendererError> {
        let device = context.render_context.device();
        let (width, height) = if initial_size.0 == 0 || initial_size.1 == 0 {
            log::warn!("SceneRenderer created before a valid render size was available; using 1x1 initial textures");
            (1, 1)
        } else {
            initial_size
        };
        options.samples = options.samples.max(1);
        log::info!(
            "Creating scene renderer: size={}x{}, samples={}, grid={}, gizmos={}",
            width,
            height,
            options.samples,
            options.grid,
            options.gizmos.is_some()
        );

        // Textures
        let (
            scene_texture,
            scene_texture_msaa,
            scene_depth_texture,
            scene_object_id_texture,
            scene_object_id_depth_texture,
        ) = Self::create_textures(
            context.render_context.clone(),
            width,
            height,
            options.samples,
        );
        let hdr_resolved = Self::create_hdr_resolved(
            context.render_context.clone(),
            width,
            height,
            options.samples,
        );
        let scene_object_id_readback = Self::create_object_id_readback_buffer(device);

        let mesh_renderer = MeshRenderer::new(context)?;
        let shadow_renderer = ShadowRenderer::new(context)?;
        let gbuffer_renderer = GBufferRenderer::new(context)?;
        let ssao = SsaoPass::new(context)?;
        let deferred_lighting = DeferredLightingRenderer::new(context)?;
        let skybox_renderer = SkyboxRenderer::new(context)?;

        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_uniform_buffer"),
            contents: bytemuck::cast_slice(&[CameraUniform::default()]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let grid_renderer = GridRenderer::new(context, &camera_uniform_buffer)?;

        let gizmo_renderer = GizmoRenderer::new(context, &camera_uniform_buffer, options.samples)?;
        let outline_renderer = OutlineRenderer::new(context)?;
        let particle_renderer = ParticleRenderer::new(context)?;
        let mut post_process = PostProcessChain::new(context)?;
        // Fog runs first in the Hdr stage, before depth of field and bloom,
        // which blur and spread the fogged light as a lens would see it. SSAO
        // is not a chain pass: the lighting pass applies it to the ambient
        // light, so occlusion never darkens the fog.
        post_process.push(Box::new(FogPass::new(context)?));
        post_process.push(Box::new(DepthOfFieldPass::new(context)?));
        post_process.push(Box::new(TonemapPass::new(context)?));
        post_process.push(Box::new(BloomPass::new(context)?));
        post_process.push(Box::new(ColorGradingPass::new(context)?));
        // Ink outlines draw on the graded image, so the ink shows as its
        // colour, and before FXAA, which smooths their stair steps.
        post_process.push(Box::new(OutlinePass::new(context)?));
        // Fog of war darkens the finished image, ink included, by what the
        // side sees of each surface; FXAA then smooths its hard edges.
        post_process.push(Box::new(FogOfWarPass::new(context)?));
        post_process.push(Box::new(FxaaPass::new(context)?));
        let overlay = OverlayCompositor::new(context)?;

        // Default assets
        let cube = SceneRendererError::load::<Mesh>(context, "meshes/cube")?;
        let white_texture = SceneRendererError::load::<Texture>(context, "textures/white")?;
        let asset_registry = context.registries.assets.read();
        let black_texture_2d = SceneRendererError::require(
            asset_registry.black_texture_2d(),
            AssetRegistry::BLACK_TEXTURE_2D,
        )?;
        let black_texture_cube = SceneRendererError::require(
            asset_registry.black_texture_cube(),
            AssetRegistry::BLACK_TEXTURE_CUBE,
        )?;
        drop(asset_registry);

        Ok(Self {
            asset_context: context.clone(),
            default_assets: SceneRendererAssets {
                cube,
                black_texture_2d,
                black_texture_cube,
                white_texture,
            },
            options,
            scene_texture_msaa,
            hdr_resolved,
            scene_texture,
            scene_depth_texture,
            scene_object_id_texture,
            scene_object_id_depth_texture,
            scene_object_id_readback,
            mesh_renderer,
            grid_renderer,
            skybox_renderer,
            camera_uniform_buffer,
            light_manager: Default::default(),
            shadow_renderer,
            gbuffer_renderer,
            gbuffer_depth_copy: None,
            ssao,
            deferred_lighting,
            gizmo_renderer,
            outline_renderer,
            particle_renderer,
            assets: Default::default(),
            own_draws: None,
            object_ids: Default::default(),
            object_ids_build: Default::default(),
            object_id_lookup: Default::default(),
            pending_object_id_readback: None,
            completed_object_pick: None,
            selected_game_object: None,
            hovered_game_object: None,
            sky_light_intensity: 0.0,
            post_process,
            post_process_settings: Default::default(),
            last_post_process_plan: Default::default(),
            overlay,
        })
    }

    /// Returns the particle renderer of this view.
    #[cfg(test)]
    pub(crate) fn particle_renderer(&self) -> &ParticleRenderer {
        &self.particle_renderer
    }

    /// What the last [`Self::render_scene_base`] drew into shadow maps.
    pub fn shadow_stats(&self) -> ShadowFrameStats {
        self.shadow_renderer.stats()
    }

    /// Whether the directional light's cascades and the spot light atlas
    /// are allocated. Each exists only while a light shadows into it.
    pub fn shadow_maps_allocated(&self) -> (bool, bool) {
        self.shadow_renderer.allocated()
    }

    /// The G-buffer the last [`Self::render_scene_base`] drew and shaded the
    /// PBR meshes from, or `None` before the first frame.
    pub fn gbuffer(&self) -> Option<&GBufferTargets> {
        self.gbuffer_renderer.targets()
    }

    /// Instances the last frame's geometry pass drew.
    pub fn gbuffer_drawn_instances(&self) -> u32 {
        self.gbuffer_renderer.drawn_instances()
    }

    /// Returns the current renderer options.
    pub fn options(&self) -> &SceneRendererOptions {
        &self.options
    }

    /// Returns the current renderer options mutably.
    pub fn options_mut(&mut self) -> &mut SceneRendererOptions {
        &mut self.options
    }

    /// Sets the hovered game object used for editor highlight overlays.
    pub fn set_hovered_game_object(&mut self, hovered_game_object: Option<Uuid>) {
        self.hovered_game_object = hovered_game_object;
    }

    /// Sets the selected game object used for editor highlight overlays.
    pub fn set_selected_game_object(&mut self, selected_game_object: Option<Uuid>) {
        self.selected_game_object = selected_game_object;
    }

    /// Renders the main scene content into the internal MSAA scene textures,
    /// on the render context the renderer was created with.
    ///
    /// Particles are drawn from `particles` when it simulates `scene`; any
    /// other scene, or none, draws no particles.
    ///
    /// The renderer collects the scene's draws itself. Views that render the
    /// same scene in a frame share one collection through
    /// [`Self::render_scene_with`] instead.
    pub fn render_scene_base(
        &mut self,
        camera: &Camera,
        camera_transform: &Transform,
        scene: &Scene,
        particles: Option<&ParticleSimulation>,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        self.render_scene(
            None,
            camera,
            camera_transform,
            scene,
            particles,
            physics_debug_pipeline,
        );
    }

    /// Renders `scene` as [`Self::render_scene_base`] does, drawing the draws
    /// `draws` collected from it, so views rendering the same scene collect
    /// its draws once. Collect `draws` after the last change to the scene
    /// before rendering it. Draws collected from another scene, or not yet
    /// collected, are not used: the renderer collects `scene`'s itself.
    pub fn render_scene_with(
        &mut self,
        draws: &DrawCollection,
        camera: &Camera,
        camera_transform: &Transform,
        scene: &Scene,
        particles: Option<&ParticleSimulation>,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        let draws = (draws.scene() == Some(scene.id())).then_some(draws);
        self.render_scene(
            draws,
            camera,
            camera_transform,
            scene,
            particles,
            physics_debug_pipeline,
        );
    }

    /// The draws of the renderer's own collection, made when it has none.
    fn take_own_draws(&mut self) -> DrawCollection {
        self.own_draws.take().unwrap_or_else(|| {
            DrawCollection::with_white_texture(
                self.asset_context.clone(),
                self.default_assets.white_texture.clone(),
            )
        })
    }

    fn render_scene(
        &mut self,
        shared_draws: Option<&DrawCollection>,
        camera: &Camera,
        camera_transform: &Transform,
        scene: &Scene,
        particles: Option<&ParticleSimulation>,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        let profile_name = self.options.profile_name;
        crate::profile_scope!("Render scene", profile_name);
        let own_draws = shared_draws.is_none().then(|| {
            crate::profile_scope!("Draw collection");
            let mut draws = self.take_own_draws();
            draws.collect(scene);
            draws
        });
        let draws = shared_draws.or(own_draws.as_ref()).expect("draws");
        let render_context = self.asset_context.render_context.clone();
        let queue = render_context.queue();
        let device = render_context.device();

        self.load_camera_uniforms(queue, camera, camera_transform);
        let camera_planes = CameraPlanes {
            near: camera.near_plane,
            far: camera.far_plane,
            tan_half_fov: CameraPlanes::tan_half_fov_of(&camera.projection),
        };
        self.post_process.set_camera(camera_planes);
        self.post_process
            .set_view_to_world(camera_transform.matrix());

        let mut frame_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("encoder"),
        });
        let mut encoder = GpuScope::new(&mut frame_encoder, profile_name);
        self.poll_object_id_readback();
        let options = PipelineOptions::builder()
            .samples(self.options.samples)
            .cull_mode(self.options.mesh_cull_mode)
            .fragment_targets(vec![Some(wgpu::ColorTargetState {
                format: self.scene_texture_msaa.descriptor.format,
                blend: None,
                write_mask: Default::default(),
            })])
            .build();
        {
            crate::profile_scope!("View assets");
            self.prepare_view(draws, scene);
        }
        {
            crate::profile_scope!("Post-process settings");
            self.post_process_settings
                .resolve_into(scene, &camera_transform.position);
        }
        if let Some(gizmo_options) = &self.options.gizmos {
            crate::profile_scope!("Gizmos");
            let object_id_lookup = &mut self.object_id_lookup;
            let object_ids_build = &mut self.object_ids_build;
            self.gizmo_renderer.draw_gizmos(
                device,
                queue,
                camera,
                camera_transform,
                [
                    self.scene_texture_msaa.descriptor.size.width as f32,
                    self.scene_texture_msaa.descriptor.size.height as f32,
                ],
                scene,
                gizmo_options,
                physics_debug_pipeline,
                |game_object_id| {
                    register_object_id(object_id_lookup, object_ids_build, game_object_id)
                },
            );
        }
        self.finish_object_ids();
        let draw_list = {
            crate::profile_scope!("Draw list");
            self.build_draw_list(&draws.draw_list)
        };
        // The camera passes skip the static batches out of the view; the
        // shadow maps, which see what the camera does not, draw them all.
        let view_draw_list = {
            crate::profile_scope!("Culling");
            let clip_from_world = camera.projection * camera_transform.inverse_matrix();
            draw_list
                .iter()
                .filter(|batch| {
                    draws
                        .batch_bounds
                        .get(&batch.mesh_id)
                        .is_none_or(|bounds| !outside_view(&clip_from_world, bounds))
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        {
            crate::profile_scope!("Mesh data");
            self.build_mesh_data(&render_context);
        }
        {
            crate::profile_scope!("Lights");
            let shadow_camera = self
                .options
                .shadows
                .then(|| ShadowCamera::new(camera, camera_transform));
            self.light_manager
                .build_data(&render_context, scene, shadow_camera.as_ref());
        }
        // Compute the skybox's environment maps before the meshes that are lit
        // by them and the skybox pass that draws its cubemap.
        {
            crate::profile_scope!("Skybox prepare");
            self.skybox_renderer.prepare(
                &render_context,
                &mut encoder.scope("Skybox prepare"),
                &self.assets,
            );
        }
        let assets = self.assets.lock();
        if let Some(outline) = self.post_process_settings.get::<ComponentOutline>() {
            warn_unknown_outline_styles(
                &self.asset_context,
                &assets,
                &draw_list,
                self.gbuffer_renderer.pbr_shader_id(),
                drawn_outline_styles(outline).len(),
            );
        }
        let material_bind_groups = {
            crate::profile_scope!("Materials");
            self.mesh_renderer.build_material_bind_groups(
                device,
                &self.asset_context,
                self.default_assets.white_texture.clone(),
                &assets,
            )
        };
        // Shadow maps render before the scene pass that samples them.
        {
            crate::profile_scope!("Shadows");
            self.shadow_renderer.render(
                device,
                queue,
                &mut encoder.scope("Shadows"),
                self.light_manager.shadow_plan(),
                &assets,
                &material_bind_groups,
                &draw_list,
                &draws.batch_bounds,
            );
        }
        let lighting = {
            crate::profile_scope!("Lighting bind groups");
            let black_texture_cube = self.default_assets.black_texture_cube.read();
            let black_texture_2d = self.default_assets.black_texture_2d.read();
            self.mesh_renderer.lighting_bind_groups(
                device,
                queue,
                &assets,
                MeshRenderDefaults {
                    black_texture_2d: &black_texture_2d,
                    black_texture_cube: &black_texture_cube,
                },
                &self.light_manager,
                &self.shadow_renderer.bindings(),
                &self.camera_uniform_buffer,
                self.skybox_renderer.skybox_id(),
                self.sky_light_intensity,
                self.light_manager.ambient_light(),
                assets
                    .shader(self.gbuffer_renderer.grass_shader_id())
                    .map(|shader| crate::render::GrassSceneInputs {
                        layout: shader.bind_group_layouts[0].clone(),
                        field: self.gbuffer_renderer.grass_field_buffer(),
                    }),
            )
        };
        let mesh_targets = MeshRenderTargets {
            color: &self.scene_texture_msaa,
            depth: &self.scene_depth_texture,
        };
        // The geometry pass draws the PBR meshes and owns the depth, SSAO
        // measures their ambient occlusion, the lighting pass shades them,
        // and the forward pass draws every other mesh after them, against the
        // deferred depth.
        crate::profile_scope!("Record passes");
        let grass = crate::render::gbuffer::GrassFieldUniform::of_scene(
            scene,
            self.gbuffer_renderer.grass_time(),
        );
        {
            crate::profile_scope!("G-buffer pass");
            self.gbuffer_renderer.render(
                &render_context,
                &mut encoder.scope("G-buffer"),
                &assets,
                &material_bind_groups,
                &view_draw_list,
                &self.camera_uniform_buffer,
                &self.scene_depth_texture,
                self.options.mesh_cull_mode,
                &grass,
            );
        }
        let gbuffer = self
            .gbuffer_renderer
            .targets()
            .expect("the geometry pass just drew the G-buffer");
        let gbuffer_depth = gbuffer.own_depth().unwrap_or(&self.scene_depth_texture);
        // At one sample the forward draws, the skybox and the particles write
        // into the G-buffer's depth next, so a chain pass that reads it gets
        // a copy taken now.
        let keep_gbuffer_depth = gbuffer.own_depth().is_none()
            && self
                .post_process
                .reads(&self.post_process_settings, SceneInput::GBufferDepth);
        if keep_gbuffer_depth {
            let copy = Self::gbuffer_depth_copy(
                &mut self.gbuffer_depth_copy,
                &render_context,
                &self.scene_depth_texture,
            );
            encoder.copy_texture_to_texture(
                self.scene_depth_texture.texture.as_image_copy(),
                copy.texture.as_image_copy(),
                self.scene_depth_texture.descriptor.size,
            );
        } else {
            self.gbuffer_depth_copy = None;
        }
        // SSAO reads the G-buffer before anything else draws into its depth,
        // so the depth, normals and occlusion all describe the PBR surfaces.
        let occlusion = self.ssao.render(
            &render_context,
            &mut encoder.scope("SSAO"),
            self.post_process_settings
                .get::<ComponentAmbientOcclusion>(),
            SsaoInputs {
                depth: gbuffer_depth,
                normals: &gbuffer.normals,
                camera: camera_planes,
            },
        );
        self.deferred_lighting.render(
            &render_context,
            &mut encoder.scope("Lighting"),
            gbuffer,
            gbuffer_depth,
            &lighting,
            occlusion,
            &self.scene_texture_msaa,
            &self.scene_depth_texture,
            self.options.clear_color,
        );
        {
            crate::profile_scope!("Forward pass");
            self.mesh_renderer.render(
                &mut encoder.scope("Forward"),
                &assets,
                &material_bind_groups,
                &lighting,
                mesh_targets,
                &options,
                &view_draw_list,
                &self.gbuffer_renderer.deferred_shaders(),
            );
        }
        if grass.fades() {
            // The grass fading around revealing benders, blended over
            // everything opaque.
            crate::profile_scope!("Transparent grass pass");
            self.mesh_renderer.render_grass(
                &render_context,
                &mut encoder.scope("Transparent grass"),
                &assets,
                &material_bind_groups,
                &lighting,
                MeshRenderTargets {
                    color: &self.scene_texture_msaa,
                    depth: &self.scene_depth_texture,
                },
                self.options.samples,
                self.options.mesh_cull_mode,
                &view_draw_list,
                self.gbuffer_renderer.grass_shader_id(),
            );
        }
        if self.options.object_ids {
            crate::profile_scope!("Object id pass");
            self.mesh_renderer.render_object_ids(
                device,
                &mut encoder.scope("Object ids"),
                &assets,
                MeshRenderTargets {
                    color: &self.scene_object_id_texture,
                    depth: &self.scene_object_id_depth_texture,
                },
                &self.camera_uniform_buffer,
                &view_draw_list,
                self.options
                    .gizmos
                    .is_some()
                    .then_some(&mut self.gizmo_renderer),
            );
        }
        drop(assets);
        self.skybox_renderer.render(
            &render_context,
            &mut encoder.scope("Skybox"),
            &self.assets,
            &self.default_assets.cube,
            &self.scene_texture_msaa,
            &self.scene_depth_texture,
            &self.camera_uniform_buffer,
            self.options.samples,
        );
        self.particle_renderer.render(
            &render_context,
            &mut encoder.scope("Particles"),
            &self.asset_context,
            scene,
            particles,
            &camera_transform.position,
            &self.camera_uniform_buffer,
            &self.scene_texture_msaa,
            &self.scene_depth_texture,
            self.options.samples,
            self.post_process
                .reads(&self.post_process_settings, SceneInput::ParticleDepth),
        );
        drop(encoder);
        queue.submit(Some(frame_encoder.finish()));
        if own_draws.is_some() {
            self.own_draws = own_draws;
        }
    }

    /// Resolves the scene, runs the post-processing chain into the scene
    /// texture, then composites the editor overlays over it for
    /// presentation.
    ///
    /// The chain uses the settings [`Self::render_scene_base`] resolved from
    /// its scene. The overlays (grid, gizmos, and the outline of the current
    /// hovered and selected objects) draw after the whole chain, so
    /// post-processing never changes their colors. Call it once after each
    /// [`Self::render_scene_base`]: above one sample the resolve discards the
    /// multisampled surface.
    ///
    /// With a G-buffer view selected, the view of the G-buffer replaces the
    /// post-processed image, and the chain does not run.
    pub fn finalize_scene(&mut self) {
        let profile_name = self.options.profile_name;
        crate::profile_scope!("Finalize scene", profile_name);
        let render_context = self.asset_context.render_context.clone();
        let device = render_context.device();
        let queue = render_context.queue();
        let mut finalize_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene_finalize_encoder"),
        });
        let mut encoder =
            GpuScope::new_with(&mut finalize_encoder, || format!("{profile_name} finalize"));
        if let Some(view) = self.options.gbuffer_view {
            if self.gbuffer_renderer.render_view(
                &render_context,
                &mut encoder,
                view,
                &self.scene_texture,
            ) {
                self.last_post_process_plan = Default::default();
                self.render_overlays(&render_context, &mut encoder.scope("Overlays"));
                drop(encoder);
                queue.submit(Some(finalize_encoder.finish()));
                return;
            }
        }
        let source = match &self.hdr_resolved {
            Some(hdr_resolved) => {
                Self::resolve_scene_texture(
                    &self.scene_texture_msaa,
                    hdr_resolved,
                    &mut encoder.scope("Resolve"),
                );
                hdr_resolved
            }
            None => &self.scene_texture_msaa,
        };
        let mut post_process = encoder.scope("Post-process");
        // The G-buffer is single-sample at every sample count, so passes read
        // its targets as they are: nothing is resolved or copied for them.
        let gbuffer = self.gbuffer_renderer.targets();
        self.last_post_process_plan = self.post_process.run(
            &render_context,
            &mut post_process,
            &self.post_process_settings,
            ChainTargets {
                source,
                output: &self.scene_texture,
                depth: &self.scene_depth_texture,
                particle_depth: self.particle_renderer.particle_depth(),
                normals: gbuffer.map(|gbuffer| &gbuffer.normals),
                albedo: gbuffer.map(|gbuffer| &gbuffer.albedo),
                gbuffer_depth: gbuffer
                    .and_then(GBufferTargets::own_depth)
                    .or(self.gbuffer_depth_copy.as_ref()),
                material: gbuffer.map(|gbuffer| &gbuffer.material),
            },
        );
        drop(post_process);
        self.render_overlays(&render_context, &mut encoder.scope("Overlays"));
        drop(encoder);
        queue.submit(Some(finalize_encoder.finish()));
    }

    /// Drops resolved effects before unloading an assembly that may define them.
    pub fn clear_post_process_settings(&mut self) {
        self.post_process_settings.clear();
    }

    /// Drops every asset the last render kept to draw again: the meshes,
    /// materials, textures, shaders and skyboxes it resolved, and the meshes
    /// and prefab parts of its own collection's terrain and foliage caches.
    /// The next render resolves them again. A renderer that draws one scene
    /// once and is kept for the next, as the thumbnail generator's is, calls
    /// it after each render, so it holds none of the scene's assets in
    /// between. A shared [`DrawCollection`] releases its own.
    pub fn release_assets(&mut self) {
        self.assets.clear();
        self.own_draws = None;
        self.skybox_renderer.set_skybox(None);
    }

    /// The settings resolved by the most recent scene render.
    pub fn post_process_settings(&self) -> &PostProcessSettings {
        &self.post_process_settings
    }

    /// The ambient occlusion pass, which runs before the lighting pass.
    pub fn ambient_occlusion(&self) -> &SsaoPass {
        &self.ssao
    }

    /// The post-processing chain this renderer runs.
    pub fn post_process(&self) -> &PostProcessChain {
        &self.post_process
    }

    /// The post-processing chain, to add a pass to it in tests.
    #[cfg(test)]
    pub(crate) fn post_process_mut(&mut self) -> &mut PostProcessChain {
        &mut self.post_process
    }

    /// The plan the last [`Self::finalize_scene`] ran.
    pub fn last_post_process_plan(&self) -> &ChainPlan {
        &self.last_post_process_plan
    }

    /// Batches the frame's draw-list elements, appending their instances to
    /// each mesh's instance list. A batch is a run of one shader, material
    /// and mesh that all cast shadows or all do not, so the shadow pass can
    /// skip whole batches.
    fn build_draw_list(&mut self, elements: &[DrawListElement]) -> Vec<DrawBatch> {
        let mut last: (AssetId, AssetId, AssetId, bool) = Default::default();
        let mut mesh_instances: HashMap<AssetId, u32> = Default::default();
        let mut instance_count: u32 = 0;
        let mut mesh = None;
        let mut draw_list: Vec<DrawBatch> = Default::default();
        let mut insert_instance_list =
            |last: (AssetId, AssetId, AssetId, bool), instance_count: u32| {
                let (shader_id, mat_id, mesh_id, cast_shadows) = last;
                if (shader_id, mat_id, mesh_id) != Default::default() && instance_count > 0 {
                    let start = mesh_instances.entry(mesh_id).or_default();
                    draw_list.push(DrawBatch {
                        shader_id,
                        mat_id,
                        mesh_id,
                        instances: *start..(*start + instance_count),
                        cast_shadows,
                    });
                    *start += instance_count;
                }
            };
        for &DrawListElement {
            shader_id,
            mat_id,
            mesh_id,
            bone_transform_index,
            object_id,
            transform,
            cast_shadows,
        } in elements
        {
            let key = (shader_id, mat_id, mesh_id, cast_shadows);
            if key != last {
                // Release the previous batch's mesh before locking the next:
                // batches that differ only in material share their mesh.
                drop(mesh.take());
                mesh = self.assets.mesh(mesh_id).map(|mesh| mesh.write());
                insert_instance_list(last, instance_count);
                instance_count = 0;
            }
            if let Some(ref mut mesh) = &mut mesh {
                mesh.instances.push(Instance {
                    bone_transform_index,
                    object_id,
                    _padding: Default::default(),
                    transform,
                });
                instance_count += 1;
            }
            last = key;
        }
        insert_instance_list(last, instance_count);
        draw_list
    }

    fn object_id_for_game_object(&self, game_object_id: Option<Uuid>) -> u32 {
        let Some(game_object_id) = game_object_id else {
            return 0;
        };
        self.object_id_lookup
            .get(&game_object_id)
            .copied()
            .unwrap_or_default()
    }

    /// Draws the enabled overlays, in order grid, gizmos, outline, over the
    /// post-processed scene texture, depth-tested against the multisampled
    /// scene depth.
    fn render_overlays(
        &mut self,
        render_context: &Arc<RenderContext>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        self.outline_renderer
            .set_selected_object_id(self.object_id_for_game_object(self.selected_game_object));
        self.outline_renderer
            .set_hovered_object_id(self.object_id_for_game_object(self.hovered_game_object));
        let context = OverlayContext {
            render_context,
            options: &self.options,
            format: OverlayCompositor::FORMAT,
            samples: self.options.samples,
            object_ids: &self.scene_object_id_texture,
        };
        let overlays: &mut [&mut dyn OverlayPass] = &mut [
            &mut self.grid_renderer,
            &mut self.gizmo_renderer,
            &mut self.outline_renderer,
        ];
        self.overlay.run(
            encoder,
            overlays,
            &context,
            &self.scene_depth_texture,
            &self.scene_texture,
        );
    }

    /// The overlay compositor, whose target exists only while an overlay is
    /// enabled.
    pub fn overlay(&self) -> &OverlayCompositor {
        &self.overlay
    }

    /// Collects `scene`'s draws into the renderer's own collection and
    /// prepares this view to draw them, as a render without shared draws
    /// does.
    #[cfg(test)]
    fn build_asset_data(&mut self, scene: &Scene) {
        let mut draws = self.take_own_draws();
        draws.collect(scene);
        self.prepare_view(&draws, scene);
        self.own_draws = Some(draws);
    }

    /// Batches the draws of the renderer's own collection.
    #[cfg(test)]
    fn build_own_draw_list(&mut self) -> Vec<DrawBatch> {
        let draws = self
            .own_draws
            .take()
            .expect("the renderer collected its draws");
        let draw_list = self.build_draw_list(&draws.draw_list);
        self.own_draws = Some(draws);
        draw_list
    }

    /// The draws of the renderer's own collection.
    #[cfg(test)]
    fn draws(&self) -> &DrawCollection {
        self.own_draws
            .as_ref()
            .expect("the renderer collected its draws")
    }

    /// Prepares this view to draw `draws`, collected from `scene`: takes
    /// their assets and object ids, and adds the view's own sky.
    fn prepare_view(&mut self, draws: &DrawCollection, scene: &Scene) {
        let world = scene.world();
        self.assets.clear();
        self.assets.meshes.clone_from(&draws.assets.meshes);
        self.assets.materials.clone_from(&draws.assets.materials);
        self.assets.shaders.clone_from(&draws.assets.shaders);
        self.assets.textures.clone_from(&draws.assets.textures);
        self.object_ids_build.clone_from(&draws.object_ids);
        self.object_id_lookup.clone_from(&draws.object_id_lookup);
        let mut query = <(Entity, &ComponentSkyLight)>::query();
        let mut skybox = None;
        let mut sky_light_intensity = 0.0;
        // A view's own environment replaces the scene's sky lights.
        let view_environment = self.options.environment.as_ref().map(|environment| {
            (
                environment.skybox.clone(),
                environment.intensity,
                environment.draw_sky,
            )
        });
        let scene_sky_lights = query
            .iter(world)
            .filter(|(entity, _)| {
                view_environment.is_none()
                    && scene
                        .game_object_from_entity(**entity)
                        .is_some_and(|go| scene.is_component_enabled::<ComponentSkyLight>(go))
            })
            .map(|(_, sky_light)| sky_light);
        if let Some((skybox_ref, intensity, _)) = &view_environment {
            let skybox_id = skybox_ref.id();
            self.assets
                .skyboxes
                .entry(skybox_id)
                .or_insert(skybox_ref.clone());
            self.assets
                .meshes
                .entry(self.default_assets.cube.id())
                .or_insert(self.default_assets.cube.clone());
            skybox = Some(skybox_id);
            sky_light_intensity = intensity.max(0.0);
        }
        for c_sky_light in scene_sky_lights {
            // Decoding a large HDR skybox takes far longer than a frame, so it
            // loads in the background and the sky appears once it is ready.
            let BackgroundLoad::Loaded(skybox_ref) = c_sky_light
                .skybox
                .get_ref_in_background(&self.asset_context.registries)
            else {
                continue;
            };
            let skybox_id = skybox_ref.id();
            self.assets
                .skyboxes
                .entry(skybox_id)
                .or_insert(skybox_ref.clone());
            self.assets
                .meshes
                .entry(self.default_assets.cube.id())
                .or_insert(self.default_assets.cube.clone());
            skybox = Some(skybox_id);
            sky_light_intensity = c_sky_light.intensity.max(0.0);
        }
        self.skybox_renderer.set_skybox(skybox);
        self.skybox_renderer.set_visible(
            view_environment
                .as_ref()
                .is_none_or(|(_, _, draw_sky)| *draw_sky),
        );
        self.sky_light_intensity = sky_light_intensity;
        // The view batches the draws into each mesh's instance list again.
        for mesh in self.assets.meshes.values() {
            mesh.write().instances.clear();
        }
    }

    fn finish_object_ids(&mut self) {
        self.object_ids = Arc::from(std::mem::take(&mut self.object_ids_build));
    }

    fn build_mesh_data(&mut self, render_context: &RenderContext) {
        for mesh in self.assets.meshes.values() {
            RenderUtils::rebuild_mesh_data(
                render_context.device(),
                render_context.queue(),
                &mut mesh.write(),
            );
        }
    }

    /// Returns the resolved scene color texture.
    pub fn scene_texture(&self) -> &Texture {
        &self.scene_texture
    }

    /// Returns the egui texture handle for the scene color texture, when
    /// available.
    pub fn scene_texture_handle(&self) -> Option<&egui::TextureHandle> {
        self.scene_texture.handle.as_ref()
    }

    /// Returns the current pixel size of the scene render target.
    pub fn scene_texture_size(&self) -> (u32, u32) {
        (
            self.scene_texture.descriptor.size.width,
            self.scene_texture.descriptor.size.height,
        )
    }

    /// Requests an object-id readback for the scene-texture pixel and returns
    /// the latest completed value while keeping one readback in flight.
    pub fn request_pick_game_object(&mut self, x: u32, y: u32) -> Option<Uuid> {
        self.finish_pending_object_id_readback(false);

        let (width, height) = self.scene_texture_size();
        if x >= width || y >= height {
            return None;
        }

        if self.pending_object_id_readback.is_none() {
            self.submit_object_id_readback(x, y);
        }

        self.completed_object_pick
            .and_then(|CompletedObjectPick { game_object, .. }| game_object)
    }

    /// Returns the rendered game object under the given scene-texture pixel.
    ///
    /// This waits for the readback to complete, so use it for authoritative
    /// click selection rather than per-frame hover updates.
    pub fn pick_game_object(&mut self, x: u32, y: u32) -> Option<Uuid> {
        let (width, height) = self.scene_texture_size();
        if x >= width || y >= height {
            return None;
        }

        self.finish_pending_object_id_readback(true);
        self.submit_object_id_readback(x, y);
        self.finish_pending_object_id_readback(true);

        match self.completed_object_pick {
            Some(CompletedObjectPick { pixel, game_object }) if pixel == (x, y) => game_object,
            _ => None,
        }
    }

    fn submit_object_id_readback(&mut self, x: u32, y: u32) {
        let device = self.asset_context.render_context.device();
        let queue = self.asset_context.render_context.queue();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene_object_id_readback"),
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.scene_object_id_texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.scene_object_id_readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = self.scene_object_id_readback.slice(..4);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        self.pending_object_id_readback = Some(PendingObjectIdReadback {
            receiver: rx,
            object_ids: Arc::clone(&self.object_ids),
            pixel: (x, y),
        });
    }

    fn poll_object_id_readback(&mut self) {
        self.finish_pending_object_id_readback(false);
    }

    fn cancel_pending_object_id_readback(&mut self) {
        if self.pending_object_id_readback.take().is_some() {
            self.scene_object_id_readback.unmap();
        }
        self.completed_object_pick = None;
    }

    fn finish_pending_object_id_readback(&mut self, wait: bool) {
        let device = self.asset_context.render_context.device();
        loop {
            let Some(pending) = self.pending_object_id_readback.as_ref() else {
                return;
            };

            let _ = device.poll(wgpu::Maintain::Poll);
            match pending.receiver.try_recv() {
                Ok(Ok(())) => break,
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    log::warn!("Object id readback failed or disconnected");
                    self.pending_object_id_readback = None;
                    self.completed_object_pick = None;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) if wait => continue,
                Err(mpsc::TryRecvError::Empty) => return,
            }
        }

        let pending = self.pending_object_id_readback.take().unwrap();
        let slice = self.scene_object_id_readback.slice(..4);
        let data = slice.get_mapped_range();
        let object_id = data
            .get(..size_of::<u32>())
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_ne_bytes)
            .unwrap_or_default();
        drop(data);
        self.scene_object_id_readback.unmap();

        self.completed_object_pick = Some(CompletedObjectPick {
            pixel: pending.pixel,
            game_object: if object_id == 0 {
                None
            } else {
                pending.object_ids.get((object_id - 1) as usize).copied()
            },
        });
    }

    /// Submits an asynchronous copy of the resolved scene color texture into a
    /// CPU-readable buffer, or `None` when the texture has no size yet.
    ///
    /// Poll the returned [`TextureReadback`] with
    /// [`TextureReadback::try_finish`] on subsequent frames.
    pub fn submit_scene_texture_readback(&self) -> Option<TextureReadback> {
        let (width, height) = self.scene_texture_size();
        if width == 0 || height == 0 {
            return None;
        }

        let unpadded_bytes_per_row = width * TextureReadback::BYTES_PER_PIXEL;
        let padded_bytes_per_row = unpadded_bytes_per_row
            .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

        let device = self.asset_context.render_context.device();
        let queue = self.asset_context.render_context.queue();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene_texture_readback"),
            size: padded_bytes_per_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene_texture_readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.scene_texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        Some(TextureReadback {
            buffer,
            receiver: rx,
            width,
            height,
            padded_bytes_per_row,
        })
    }

    fn create_textures(
        render_context: Arc<RenderContext>,
        width: u32,
        height: u32,
        samples: u32,
    ) -> (Texture, Texture, Texture, Texture, Texture) {
        let scene_texture = Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("scene_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            true,
        );
        let scene_texture_msaa = Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("scene_texture_msaa"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            false,
        );
        let scene_depth_texture = Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("scene_depth_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                // Post-processing passes that read depth bind it, and the
                // particle depth starts as a copy of it.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            },
            None,
            None,
            false,
        );
        let scene_object_id_texture = Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("scene_object_id_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Uint,
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            false,
        );
        let scene_object_id_depth_texture = Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("scene_object_id_depth_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
            None,
            None,
            false,
        );
        (
            scene_texture,
            scene_texture_msaa,
            scene_depth_texture,
            scene_object_id_texture,
            scene_object_id_depth_texture,
        )
    }

    fn create_object_id_readback_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene_object_id_readback"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        })
    }

    fn load_camera_uniforms(
        &self,
        queue: &wgpu::Queue,
        camera: &Camera,
        camera_transform: &Transform,
    ) {
        let camera_uniform = CameraUniform::new(
            &camera_transform.inverse_matrix(),
            &camera.projection,
            camera.near_plane,
            camera.far_plane,
            [
                self.scene_texture_msaa.descriptor.size.width as f32,
                self.scene_texture_msaa.descriptor.size.height as f32,
            ],
        );
        queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::cast_slice(&[camera_uniform]),
        );
    }

    /// Resolves the multisampled scene color into the single-sample
    /// `hdr_resolved`, through a render pass whose resolve target it is: a
    /// multisampled texture cannot be the source of a texture copy.
    fn resolve_scene_texture(
        scene_texture_msaa: &Texture,
        hdr_resolved: &Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Scene Resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &scene_texture_msaa.view,
                resolve_target: Some(&hdr_resolved.view),
                // This is the multisampled surface's last use: the next
                // `render_scene_base` clears it, so only the resolve is kept.
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    /// The single-sample resolve target of a `samples`-sample scene, or
    /// `None` at one sample, where the scene target is single-sample already.
    fn create_hdr_resolved(
        render_context: Arc<RenderContext>,
        width: u32,
        height: u32,
        samples: u32,
    ) -> Option<Texture> {
        (samples > 1).then(|| {
            Texture::new(
                render_context,
                &wgpu::TextureDescriptor {
                    label: Some("scene_hdr_resolved"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                None,
                false,
            )
        })
    }

    /// `copy`, recreated when it is missing or no longer matches `depth`'s
    /// size: the single-sample texture the G-buffer's depth is copied into.
    fn gbuffer_depth_copy<'a>(
        copy: &'a mut Option<Texture>,
        render_context: &Arc<RenderContext>,
        depth: &Texture,
    ) -> &'a Texture {
        let size = depth.descriptor.size;
        if copy
            .as_ref()
            .is_none_or(|texture| texture.descriptor.size != size)
        {
            *copy = Some(Texture::new(
                render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some("gbuffer_depth_copy"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Depth32Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                None,
                None,
                false,
            ));
        }
        copy.as_ref().expect("the copy was just created")
    }

    /// Resizes the internal scene color and depth textures.
    pub fn resize_textures(&mut self, width: u32, height: u32) {
        if self.scene_texture.descriptor.size.width == width
            && self.scene_texture.descriptor.size.height == height
        {
            return;
        }
        log::trace!(
            "Resizing scene renderer textures from {}x{} to {}x{}",
            self.scene_texture.descriptor.size.width,
            self.scene_texture.descriptor.size.height,
            width,
            height
        );
        self.cancel_pending_object_id_readback();
        (
            self.scene_texture,
            self.scene_texture_msaa,
            self.scene_depth_texture,
            self.scene_object_id_texture,
            self.scene_object_id_depth_texture,
        ) = Self::create_textures(
            self.asset_context.render_context.clone(),
            width,
            height,
            self.options.samples,
        );
        self.hdr_resolved = Self::create_hdr_resolved(
            self.asset_context.render_context.clone(),
            width,
            height,
            self.options.samples,
        );
        self.gbuffer_renderer
            .resize(&self.asset_context.render_context, (width, height));
        self.ssao.release_targets();
        self.gbuffer_depth_copy = None;
        self.post_process.clear_targets();
        self.overlay.clear_targets();
    }
}

/// Poll state of an asynchronous scene texture readback.
pub enum ReadbackPoll {
    /// The GPU copy or buffer mapping has not completed yet.
    Pending,
    /// The image is ready.
    Ready(image::RgbaImage),
    /// The readback failed and will never complete.
    Failed(String),
}

/// An in-flight asynchronous copy of the resolved scene color texture
/// (`Rgba16Float`) into CPU memory, created by
/// [`SceneRenderer::submit_scene_texture_readback`].
pub struct TextureReadback {
    buffer: wgpu::Buffer,
    receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
}

impl TextureReadback {
    const BYTES_PER_PIXEL: u32 = 8; // Rgba16Float

    /// Polls the readback without blocking. Once this returns
    /// [`ReadbackPoll::Ready`] or [`ReadbackPoll::Failed`] the readback is
    /// finished and must not be polled again.
    pub fn try_finish(&mut self, render_context: &RenderContext) -> ReadbackPoll {
        let _ = render_context.device().poll(wgpu::Maintain::Poll);
        match self.receiver.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return ReadbackPoll::Pending,
            Ok(Err(error)) => return ReadbackPoll::Failed(error.to_string()),
            Err(mpsc::TryRecvError::Disconnected) => {
                return ReadbackPoll::Failed("readback channel disconnected".into())
            }
            Ok(Ok(())) => {}
        }

        let data = self.buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for row in 0..self.height {
            let start = (row * self.padded_bytes_per_row) as usize;
            let end = start + (self.width * Self::BYTES_PER_PIXEL) as usize;
            for texel in data[start..end]
                .as_chunks::<{ Self::BYTES_PER_PIXEL as usize }>()
                .0
            {
                for (channel, bytes) in texel.as_chunks::<2>().0.iter().enumerate() {
                    let value = half_to_f32(u16::from_le_bytes(*bytes));
                    // Alpha is coverage, not color; only color channels are
                    // sRGB-encoded for display.
                    pixels.push(if channel == 3 {
                        (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                    } else {
                        linear_to_srgb(value)
                    });
                }
            }
        }
        drop(data);
        self.buffer.unmap();

        match image::RgbaImage::from_raw(self.width, self.height, pixels) {
            Some(image) => ReadbackPoll::Ready(image),
            None => ReadbackPoll::Failed("readback produced a malformed image".into()),
        }
    }
}

pub(crate) fn half_to_f32(half: u16) -> f32 {
    let sign = (half >> 15) as u32;
    let exponent = ((half >> 10) & 0x1f) as u32;
    let fraction = (half & 0x3ff) as u32;
    let bits = if exponent == 0 {
        if fraction == 0 {
            sign << 31
        } else {
            // Subnormal half: renormalize into an f32 exponent.
            let shift = fraction.leading_zeros() - 21;
            let fraction = (fraction << (shift + 1)) & 0x3ff;
            (sign << 31) | ((113 - shift) << 23) | (fraction << 13)
        }
    } else if exponent == 0x1f {
        (sign << 31) | (0xff << 23) | (fraction << 13)
    } else {
        (sign << 31) | ((exponent + 112) << 23) | (fraction << 13)
    };
    f32::from_bits(bits)
}

pub(crate) fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0 + 0.5) as u8
}

/// Warns, once per material and style, about each PBR material drawn this
/// frame whose `outline_style` is past the end of the Outline effect's
/// `count` styles: its surfaces draw no ink.
fn warn_unknown_outline_styles(
    context: &ReadOnlyAssetContext,
    assets: &LockedAssetRenderState<'_>,
    draw_list: &[DrawBatch],
    pbr_shader_id: AssetId,
    count: usize,
) {
    for batch in draw_list
        .iter()
        .filter(|batch| batch.shader_id == pbr_shader_id)
    {
        let Some(material) = assets.material(batch.mat_id) else {
            continue;
        };
        let style = material.outline_style();
        if style as usize > count {
            crate::logging::warn_once(format!("outline-style:{}:{style}", batch.mat_id), || {
                format!(
                        "Material `{}` uses outline style {style}, but the Outline effect has {count}                          styles; it draws no outline",
                        context.registries.assets.read().asset_name(batch.mat_id)
                    )
            });
        }
    }
}

#[cfg(test)]
mod tests;
