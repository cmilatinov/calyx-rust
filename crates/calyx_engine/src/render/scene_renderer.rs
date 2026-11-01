use super::RenderContext;
use crate::assets::error::AssetError;
use crate::assets::material::Material;
use crate::assets::mesh::{Instance, Mesh};
use crate::assets::skybox::Skybox;
use crate::assets::texture::Texture;
use crate::assets::{Asset, AssetId, AssetRegistry, BackgroundLoad};
use crate::component::{ComponentMesh, ComponentSkinnedMesh, ComponentSkyLight};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::Transform;
use crate::render::asset_render_state::AssetRenderState;
use crate::render::overlay::{OverlayCompositor, OverlayContext, OverlayPass};
use crate::render::postprocess::{
    BloomPass, CameraPlanes, ChainPlan, ChainTargets, ColorGradingPass, DepthOfFieldPass, FxaaPass,
    PostProcessChain, PostProcessSettings, SceneInput, SsaoPass, TonemapPass,
};
use crate::render::render_utils::RenderUtils;
use crate::render::{
    Camera, DrawBatch, GBufferRenderer, GBufferTargets, GBufferView, GizmoRenderer, GridRenderer,
    LightManager, MeshRenderDefaults, MeshRenderTargets, MeshRenderer, OutlineRenderer,
    ParticleRenderer, ParticleSimulation, PipelineOptions, ShadowCamera, ShadowFrameStats,
    ShadowRenderer, SkyboxRenderer,
};
use crate::scene::{GameObject, Scene};
use crate::utils::TypeUuid;
use egui::Color32;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use legion::{Entity, IntoQuery};
use nalgebra_glm as glm;
use nalgebra_glm::Mat4;
use rapier3d::pipeline::DebugRenderPipeline;
use std::collections::{HashMap, HashSet};
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

/// Scene renderer feature toggles and default clear settings.
pub struct SceneRendererOptions {
    /// Whether to draw the editor grid.
    pub grid: bool,
    /// Whether to draw debug gizmos.
    pub gizmos: bool,
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
    /// `None` for the lit image. While it is `None` the geometry pass does
    /// not run and the G-buffer is not allocated.
    pub gbuffer_view: Option<GBufferView>,
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
            gizmos: false,
            clear_color: Color32::TRANSPARENT,
            mesh_cull_mode: Some(wgpu::Face::Back),
            samples: 0,
            environment: None,
            shadows: false,
            gbuffer_view: None,
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

/// One draw-list entry emitted while collecting scene meshes.
pub struct DrawListElement {
    shader_id: AssetId,
    mat_id: AssetId,
    mesh_id: AssetId,
    bone_transform_index: i32,
    object_id: u32,
    transform: [[f32; 4]; 4],
    cast_shadows: bool,
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
    gizmo_renderer: GizmoRenderer,
    outline_renderer: OutlineRenderer,
    particle_renderer: ParticleRenderer,
    assets: AssetRenderState,
    draw_list: Vec<DrawListElement>,
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

/// Instance index of the next skinned draw in a mesh's shared bone buffer, or `None` for a mesh without bones.
fn skinning_instance_index(uploaded_bone_transforms: usize, num_bones: usize) -> Option<usize> {
    uploaded_bone_transforms.checked_div(num_bones)
}

/// World transform that places a skinned mesh, as [`ComponentSkinnedMesh::world_transform`] says.
fn skinned_mesh_world_transform(
    scene: &Scene,
    game_object: GameObject,
    skinned_mesh: &ComponentSkinnedMesh,
) -> Transform {
    skinned_mesh.world_transform(scene, game_object)
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
            options.gizmos
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
        // SSAO runs first in the Hdr stage, on the unblurred scene.
        post_process.push(Box::new(SsaoPass::new(context)?));
        // Depth of field runs after SSAO, before bloom, so bloom
        // spreads already blurred highlights.
        post_process.push(Box::new(DepthOfFieldPass::new(context)?));
        post_process.push(Box::new(TonemapPass::new(context)?));
        post_process.push(Box::new(BloomPass::new(context)?));
        post_process.push(Box::new(ColorGradingPass::new(context)?));
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
            gizmo_renderer,
            outline_renderer,
            particle_renderer,
            assets: Default::default(),
            draw_list: Default::default(),
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

    /// The G-buffer the last [`Self::render_scene_base`] drew, or `None`
    /// when no G-buffer view was selected, and the geometry pass did not run.
    pub fn gbuffer(&self) -> Option<&GBufferTargets> {
        self.gbuffer_renderer.targets()
    }

    /// Instances the last frame's geometry pass drew.
    pub fn gbuffer_drawn_instances(&self) -> u32 {
        self.gbuffer_renderer.drawn_instances()
    }

    /// Whether this frame needs the G-buffer: only a G-buffer view reads it
    /// until the deferred lighting pass lands.
    fn gbuffer_needed(&self) -> bool {
        self.options.gbuffer_view.is_some()
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
    pub fn render_scene_base(
        &mut self,
        camera: &Camera,
        camera_transform: &Transform,
        scene: &Scene,
        particles: Option<&ParticleSimulation>,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        let render_context = self.asset_context.render_context.clone();
        let queue = render_context.queue();
        let device = render_context.device();

        self.load_camera_uniforms(queue, camera, camera_transform);
        self.post_process.set_camera(CameraPlanes {
            near: camera.near_plane,
            far: camera.far_plane,
            tan_half_fov: CameraPlanes::tan_half_fov_of(&camera.projection),
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("encoder"),
        });
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
        self.build_asset_data(&render_context, scene);
        self.post_process_settings = PostProcessSettings::resolve(scene);
        if self.options.gizmos {
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
                physics_debug_pipeline,
                |game_object_id| {
                    register_object_id(object_id_lookup, object_ids_build, game_object_id)
                },
            );
        }
        self.finish_object_ids();
        let draw_list = self.build_draw_list();
        self.build_mesh_data(&render_context);
        let shadow_camera = self
            .options
            .shadows
            .then(|| ShadowCamera::new(camera, camera_transform));
        self.light_manager
            .build_data(&render_context, scene, shadow_camera.as_ref());
        // Compute the skybox's environment maps before the meshes that are lit
        // by them and the skybox pass that draws its cubemap.
        self.skybox_renderer
            .prepare(&render_context, &mut encoder, &self.assets);
        let assets = self.assets.lock();
        let material_bind_groups = self.mesh_renderer.build_material_bind_groups(
            device,
            &self.asset_context,
            self.default_assets.white_texture.clone(),
            &assets,
        );
        // Shadow maps render before the scene pass that samples them.
        self.shadow_renderer.render(
            device,
            queue,
            &mut encoder,
            self.light_manager.shadow_plan(),
            &assets,
            &material_bind_groups,
            &draw_list,
        );
        {
            let black_texture_cube = self.default_assets.black_texture_cube.read();
            let black_texture_2d = self.default_assets.black_texture_2d.read();
            self.mesh_renderer.render(
                device,
                &mut encoder,
                &assets,
                &material_bind_groups,
                MeshRenderDefaults {
                    black_texture_2d: &black_texture_2d,
                    black_texture_cube: &black_texture_cube,
                },
                MeshRenderTargets {
                    color: &self.scene_texture_msaa,
                    depth: &self.scene_depth_texture,
                },
                queue,
                &self.light_manager,
                &self.shadow_renderer.bindings(),
                &self.camera_uniform_buffer,
                self.options.clear_color,
                &options,
                self.skybox_renderer.skybox_id(),
                self.sky_light_intensity,
                self.light_manager.ambient_light(),
                &draw_list,
            );
        }
        // The geometry pass tests against the depth the forward pass just
        // wrote, so the G-buffer holds the surfaces the lit image shows.
        if self.gbuffer_needed() {
            self.gbuffer_renderer.render(
                &render_context,
                &mut encoder,
                &assets,
                &material_bind_groups,
                &draw_list,
                &self.camera_uniform_buffer,
                &self.scene_depth_texture,
                self.options.mesh_cull_mode,
            );
        } else {
            self.gbuffer_renderer.release();
        }
        self.mesh_renderer.render_object_ids(
            device,
            &mut encoder,
            &assets,
            MeshRenderTargets {
                color: &self.scene_object_id_texture,
                depth: &self.scene_object_id_depth_texture,
            },
            &self.camera_uniform_buffer,
            &draw_list,
            self.options.gizmos.then_some(&mut self.gizmo_renderer),
        );
        drop(assets);
        self.skybox_renderer.render(
            &render_context,
            &mut encoder,
            &self.assets,
            &self.default_assets.cube,
            &self.scene_texture_msaa,
            &self.scene_depth_texture,
            self.grid_renderer.camera_bind_group(),
            self.options.samples,
        );
        self.particle_renderer.render(
            &render_context,
            &mut encoder,
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
        queue.submit(Some(encoder.finish()));
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
        let render_context = self.asset_context.render_context.clone();
        let device = render_context.device();
        let queue = render_context.queue();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene_finalize_encoder"),
        });
        if let Some(view) = self.options.gbuffer_view {
            if self.gbuffer_renderer.render_view(
                &render_context,
                &mut encoder,
                view,
                &self.scene_texture,
            ) {
                self.last_post_process_plan = Default::default();
                self.render_overlays(&render_context, &mut encoder);
                queue.submit(Some(encoder.finish()));
                return;
            }
        }
        let source = match &self.hdr_resolved {
            Some(hdr_resolved) => {
                Self::resolve_scene_texture(&self.scene_texture_msaa, hdr_resolved, &mut encoder);
                hdr_resolved
            }
            None => &self.scene_texture_msaa,
        };
        self.last_post_process_plan = self.post_process.run(
            &render_context,
            &mut encoder,
            &self.post_process_settings,
            ChainTargets {
                source,
                output: &self.scene_texture,
                depth: &self.scene_depth_texture,
                particle_depth: self.particle_renderer.particle_depth(),
            },
        );
        self.render_overlays(&render_context, &mut encoder);
        queue.submit(Some(encoder.finish()));
    }

    /// The post-processing settings the last [`Self::render_scene_base`]
    /// resolved from its scene.
    pub fn post_process_settings(&self) -> &PostProcessSettings {
        &self.post_process_settings
    }

    /// The post-processing chain this renderer runs.
    pub fn post_process(&self) -> &PostProcessChain {
        &self.post_process
    }

    /// The plan the last [`Self::finalize_scene`] ran.
    pub fn last_post_process_plan(&self) -> &ChainPlan {
        &self.last_post_process_plan
    }

    /// Batches the frame's draw-list elements, appending their instances to
    /// each mesh's instance list. A batch is a run of one shader, material
    /// and mesh that all cast shadows or all do not, so the shadow pass can
    /// skip whole batches.
    fn build_draw_list(&mut self) -> Vec<DrawBatch> {
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
        for DrawListElement {
            shader_id,
            mat_id,
            mesh_id,
            bone_transform_index,
            object_id,
            transform,
            cast_shadows,
        } in self.draw_list.drain(0..)
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

    fn insert_draw_list_entry(
        &mut self,
        mesh_ref: &Ref<Mesh>,
        mat_ref: &Ref<Material>,
        game_object_id: Uuid,
        bone_transform_index: Option<i32>,
        transform: [[f32; 4]; 4],
        cast_shadows: bool,
    ) {
        let Some(shader_ref) = mat_ref
            .read()
            .shader
            .get_ref(&self.asset_context.registries)
        else {
            return;
        };
        let object_id = self.register_object_id(game_object_id);
        self.draw_list.push(DrawListElement {
            shader_id: shader_ref.id(),
            mat_id: mat_ref.id(),
            mesh_id: mesh_ref.id(),
            bone_transform_index: bone_transform_index.unwrap_or(-1),
            object_id,
            transform,
            cast_shadows,
        });
        self.assets
            .meshes
            .entry(mesh_ref.id())
            .or_insert(mesh_ref.clone());
        self.assets
            .materials
            .entry(mat_ref.id())
            .or_insert(mat_ref.clone());
        self.assets
            .shaders
            .entry(shader_ref.id())
            .or_insert(shader_ref);
    }

    fn register_object_id(&mut self, game_object_id: Uuid) -> u32 {
        register_object_id(
            &mut self.object_id_lookup,
            &mut self.object_ids_build,
            game_object_id,
        )
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

    fn build_asset_data(&mut self, render_context: &RenderContext, scene: &Scene) {
        let world = scene.world();
        self.assets.clear();
        self.draw_list.clear();
        self.object_ids_build.clear();
        self.object_id_lookup.clear();
        let mut query = <(Entity, &ComponentMesh)>::query();
        for (entity, c_mesh) in query.iter(world) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            if !scene.is_visible_in_hierarchy(game_object)
                || !scene.is_component_enabled::<ComponentMesh>(game_object)
            {
                continue;
            }
            let Some(mesh_ref) = c_mesh.mesh.get_ref(&self.asset_context.registries) else {
                continue;
            };
            let Some(mat_ref) = c_mesh.material.get_ref(&self.asset_context.registries) else {
                continue;
            };
            let transform = scene.world_transform(game_object);
            self.insert_draw_list_entry(
                &mesh_ref,
                &mat_ref,
                scene.uuid(game_object),
                None,
                transform.matrix().into(),
                c_mesh.cast_shadows,
            );
        }
        let mut skinned_meshes: HashSet<Uuid> = Default::default();
        let mut rest_poses: HashMap<GameObject, HashMap<String, Mat4>> = HashMap::new();
        let no_bones = HashMap::new();
        let mut rest_palette = Vec::new();
        let mut query = <(Entity, &ComponentSkinnedMesh)>::query();
        for (entity, c_skinned_mesh) in query.iter(world) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            if !scene.is_visible_in_hierarchy(game_object)
                || !scene.is_component_enabled::<ComponentSkinnedMesh>(game_object)
            {
                continue;
            }
            let Some(mesh_ref) = c_skinned_mesh.mesh.get_ref(&self.asset_context.registries) else {
                continue;
            };
            let Some(mat_ref) = c_skinned_mesh
                .material
                .get_ref(&self.asset_context.registries)
            else {
                continue;
            };
            let mesh_id = mesh_ref.id();
            let transform = skinned_mesh_world_transform(scene, game_object, c_skinned_mesh);
            let bone_transform_index;
            {
                let mut mesh = mesh_ref.write();
                if !skinned_meshes.contains(&mesh_id) {
                    mesh.bone_transforms.clear();
                }
                let num_bones = mesh.bones.len();
                let Some(index) = skinning_instance_index(mesh.bone_transforms.len(), num_bones)
                else {
                    crate::logging::warn_once(format!("boneless-skinned-mesh:{mesh_id}"), || {
                        format!("Skinned mesh {mesh_id} has no bones; skipping skinning upload")
                    });
                    continue;
                };
                skinned_meshes.insert(mesh_id);
                bone_transform_index = index;
                // Each instance takes exactly `num_bones` slots, so the next instance's index
                // holds. A palette the animator has not filled, as in edit mode, draws the rest
                // pose the bone objects hold instead of collapsing every vertex.
                if c_skinned_mesh.bone_transforms.len() >= num_bones {
                    mesh.bone_transforms
                        .extend_from_slice(&c_skinned_mesh.bone_transforms[..num_bones]);
                } else {
                    let bones = c_skinned_mesh
                        .root_bone
                        .game_object(scene)
                        .map(|root| {
                            &*rest_poses
                                .entry(root)
                                .or_insert_with(|| ComponentSkinnedMesh::bone_poses(scene, root))
                        })
                        .unwrap_or(&no_bones);
                    ComponentSkinnedMesh::skinning_palette(&mesh, bones, &mut rest_palette);
                    mesh.bone_transforms.extend_from_slice(&rest_palette);
                }
            }
            self.insert_draw_list_entry(
                &mesh_ref,
                &mat_ref,
                scene.uuid(game_object),
                Some(bone_transform_index as i32),
                transform.matrix().into(),
                c_skinned_mesh.cast_shadows,
            );
        }
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
        for mesh in self.assets.meshes.values() {
            mesh.write().instances.clear();
        }
        let AssetRenderState {
            materials,
            textures,
            ..
        } = &mut self.assets;
        for material in materials.values() {
            let mut material = material.write();
            material.load_buffers(render_context);
            material.collect_textures(
                &self.asset_context,
                textures,
                self.default_assets.white_texture.clone(),
            );
        }
        self.draw_list.sort_by_key(
            |DrawListElement {
                 shader_id,
                 mat_id,
                 mesh_id,
                 cast_shadows,
                 ..
             }| (*shader_id, *mat_id, *mesh_id, *cast_shadows),
        );
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

fn register_object_id(
    object_id_lookup: &mut HashMap<Uuid, u32>,
    object_ids_build: &mut Vec<Uuid>,
    game_object_id: Uuid,
) -> u32 {
    if let Some(object_id) = object_id_lookup.get(&game_object_id) {
        return *object_id;
    }
    object_ids_build.push(game_object_id);
    let object_id = object_ids_build.len() as u32;
    object_id_lookup.insert(game_object_id, object_id);
    object_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::material::{MaterialTexture, ShaderVariableValue};
    use crate::assets::skybox::Skybox;
    use crate::assets::AssetRef;
    use crate::component::{
        ComponentDepthOfField, ComponentGlobalPostProcessVolume, ComponentID, ComponentMesh,
        ComponentParticleSystem, ComponentTonemap, ParticleBlendMode, ParticleColorOverLifetime,
        ParticleEmission, ParticleForces, ParticleScalarRange, ParticleSizeOverLifetime,
        ParticleSpawnShape, ParticleVelocity,
    };
    use crate::math::Transform;
    use crate::render::{Camera, Shader, SHADOW_CASCADE_COUNT};
    use crate::test_support::{
        test_adapter, test_asset_context_with_assets, SceneBuilder, TestAssets,
    };
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec3, vec4, Vec3};
    use std::path::Path;

    const VIEWPORT: u32 = 512;

    /// Creates a PBR material registered as `name`.
    fn pbr_material(context: &ReadOnlyAssetContext, name: &str) -> Ref<Material> {
        let registry = context.registries.assets.read();
        let shader = registry
            .load::<Shader>("shaders/mesh/pbr")
            .expect("missing pbr shader");
        registry
            .create_or_update(name.into(), Material::from_shader(context, shader))
            .expect("material should be created")
    }

    /// Spawns a game object drawing `mesh` with `material` at `position`.
    fn spawn_mesh(
        builder: &mut SceneBuilder,
        name: &str,
        mesh: &Ref<Mesh>,
        material: &Ref<Material>,
        position: Vec3,
    ) -> Uuid {
        let game_object = builder.spawn(name);
        builder.add_component(
            game_object,
            ComponentMesh {
                mesh: AssetRef::from_id(mesh.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        builder.set_transform(game_object, glm::translation(&position));
        builder.scene().uuid(game_object)
    }

    /// A camera on the negative z axis looking at the origin.
    fn front_camera(aspect: f32, distance: f32) -> (Camera, Transform) {
        let camera = Camera::new(aspect, 60f32.to_radians(), 0.1, 200.0);
        let transform = Transform::from_components(
            vec3(0.0, 0.0, -distance),
            UnitQuaternion::face_towards(&vec3(0.0, 0.0, 1.0), &vec3(0.0, 1.0, 0.0)),
            vec3(1.0, 1.0, 1.0),
        );
        (camera, transform)
    }

    /// The pixel `position` lands on in a `width` by `height` target.
    fn project(
        camera: &Camera,
        transform: &Transform,
        position: Vec3,
        (width, height): (u32, u32),
    ) -> (u32, u32) {
        let clip = camera.projection
            * transform.inverse_matrix()
            * vec4(position.x, position.y, position.z, 1.0);
        let ndc = clip.xyz() / clip.w;
        (
            ((ndc.x * 0.5 + 0.5) * width as f32) as u32,
            ((0.5 - ndc.y * 0.5) * height as f32) as u32,
        )
    }

    fn renderer(context: &ReadOnlyAssetContext, samples: u32, size: (u32, u32)) -> SceneRenderer {
        SceneRenderer::new(
            context,
            SceneRendererOptions {
                samples,
                ..Default::default()
            },
            size,
        )
        .expect("scene renderer should load its assets")
    }

    /// Spawns an emitter at `position` that emits one particle of `color`
    /// and `size` on the spot, on its first simulated frame.
    fn spawn_particle(
        builder: &mut SceneBuilder,
        name: &str,
        position: Vec3,
        color: Color32,
        size: f32,
    ) -> GameObject {
        let game_object = builder.spawn(name);
        let mut system = ComponentParticleSystem::default();
        system.emission = ParticleEmission {
            rate: 0.0,
            burst_count: 1,
            max_particles: 1,
            ..Default::default()
        };
        system.lifetime = ParticleScalarRange {
            min: 10.0,
            max: 10.0,
        };
        system.shape = ParticleSpawnShape::Sphere { radius: 0.0 };
        system.velocity = ParticleVelocity {
            initial: Vec3::zeros(),
            randomness: Vec3::zeros(),
        };
        system.forces = ParticleForces {
            acceleration: Vec3::zeros(),
        };
        system.size = ParticleSizeOverLifetime::linear(size, size, 0.0);
        system.color = ParticleColorOverLifetime::linear(color, color);
        builder.add_component(game_object, system);
        builder.set_transform(game_object, glm::translation(&position));
        game_object
    }

    fn read_scene_texture(
        renderer: &SceneRenderer,
        render_context: &RenderContext,
    ) -> image::RgbaImage {
        let mut readback = renderer
            .submit_scene_texture_readback()
            .expect("scene texture has a size");
        loop {
            let _ = render_context.device().poll(wgpu::Maintain::Wait);
            match readback.try_finish(render_context) {
                ReadbackPoll::Pending => continue,
                ReadbackPoll::Ready(image) => break image,
                ReadbackPoll::Failed(error) => panic!("scene readback failed: {error}"),
            }
        }
    }

    #[test]
    fn each_particle_system_draws_its_own_particles() {
        const SIZE: (u32, u32) = (64, 64);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let left = vec3(-1.5, 0.0, 0.0);
        let right = vec3(1.5, 0.0, 0.0);
        spawn_particle(&mut builder, "red", left, Color32::RED, 1.0);
        spawn_particle(&mut builder, "green", right, Color32::GREEN, 1.0);
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);
        let mut particles = ParticleSimulation::new();
        particles.simulate(&scene, &render_context, 0.0);

        let mut renderer = SceneRenderer::new(
            &read_only_context,
            SceneRendererOptions {
                samples: 1,
                clear_color: Color32::BLACK,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        renderer.render_scene_base(&camera, &transform, &scene, Some(&particles), None);
        renderer.finalize_scene();
        let image = read_scene_texture(&renderer, &render_context);

        let (x, y) = project(&camera, &transform, left, SIZE);
        let pixel = image.get_pixel(x, y);
        // ACES desaturates pure primaries a little, so only the dominant
        // channel tells the two systems apart.
        assert!(
            pixel[0] > 200 && pixel[0] > pixel[1] + 100,
            "the red system should draw its particle on the left, got {pixel:?}"
        );
        let (x, y) = project(&camera, &transform, right, SIZE);
        let pixel = image.get_pixel(x, y);
        assert!(
            pixel[1] > 200 && pixel[1] > pixel[0] + 100,
            "the green system should draw its particle on the right, got {pixel:?}"
        );
    }

    /// Renders a white particle of diameter 2 at the origin, 5 units in front
    /// of the camera, over the cleared background at the far plane, with
    /// depth of field focused on the particle when `focus` is set and no
    /// post-process volume otherwise.
    fn render_particle_over_far_background(
        context: &ReadOnlyAssetContext,
        blend_mode: ParticleBlendMode,
        samples: u32,
        focus: bool,
    ) -> image::RgbaImage {
        let render_context = context.render_context.clone();
        let mut builder = SceneBuilder::new(context.registries.clone());
        let particle = spawn_particle(&mut builder, "particle", Vec3::zeros(), Color32::WHITE, 2.0);
        builder
            .scene_mut()
            .write_component(particle, |system: &mut ComponentParticleSystem| {
                system.rendering.blend_mode = blend_mode;
            })
            .expect("the particle has a system");
        if focus {
            let volume = builder.spawn("volume");
            builder.add_component(volume, ComponentGlobalPostProcessVolume::default());
            builder.add_component(
                volume,
                ComponentDepthOfField {
                    focus_distance: 5.0,
                    focus_range: 1.0,
                    max_blur: 8.0,
                },
            );
        }
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);
        let mut particles = ParticleSimulation::new();
        particles.simulate(&scene, &render_context, 0.0);

        let mut renderer = SceneRenderer::new(
            context,
            SceneRendererOptions {
                samples,
                clear_color: Color32::BLACK,
                ..Default::default()
            },
            (64, 64),
        )
        .expect("scene renderer should load its assets");
        renderer.render_scene_base(&camera, &transform, &scene, Some(&particles), None);
        renderer.finalize_scene();
        if focus {
            let names: Vec<_> = renderer
                .last_post_process_plan()
                .passes
                .iter()
                .map(|planned| renderer.post_process().passes()[planned.pass].name())
                .collect();
            assert!(names.contains(&"depth_of_field"), "{names:?}");
        }
        read_scene_texture(&renderer, &render_context)
    }

    #[test]
    fn a_particle_in_focus_is_not_blurred_as_the_background_behind_it() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let (camera, transform) = front_camera(1.0, 5.0);
        let center = project(&camera, &transform, Vec3::zeros(), (64, 64));
        let edge = project(&camera, &transform, vec3(1.0, 0.0, 0.0), (64, 64));
        let radius = center.0.abs_diff(edge.0) as f32;
        assert!(radius > 8.0, "the particle is wider than the blur");

        for samples in [1, 4] {
            if samples > 1
                && (!supports_samples(
                    &render_context,
                    wgpu::TextureFormat::Rgba16Float,
                    samples,
                    true,
                ) || !supports_samples(
                    &render_context,
                    wgpu::TextureFormat::Depth32Float,
                    samples,
                    false,
                ))
            {
                eprintln!("skipping {samples}x MSAA: the test adapter does not support it");
                continue;
            }
            for blend_mode in [ParticleBlendMode::Alpha, ParticleBlendMode::Additive] {
                let sharp = render_particle_over_far_background(
                    &read_only_context,
                    blend_mode,
                    samples,
                    false,
                );
                let focused = render_particle_over_far_background(
                    &read_only_context,
                    blend_mode,
                    samples,
                    true,
                );
                // The background is fully blurred, the particle is on the
                // focus plane: inside its disc the image matches the frame
                // without depth of field.
                for (x, y, pixel) in focused.enumerate_pixels() {
                    let dx = x as f32 + 0.5 - center.0 as f32;
                    let dy = y as f32 + 0.5 - center.1 as f32;
                    if (dx * dx + dy * dy).sqrt() > radius - 2.0 {
                        continue;
                    }
                    let expected = sharp.get_pixel(x, y);
                    assert!(
                        (0..3).all(|channel| pixel[channel].abs_diff(expected[channel]) <= 2),
                        "{blend_mode:?} particle at {samples}x: pixel ({x}, {y}) is blurred: \
                         {pixel:?}, sharp {expected:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn construction_reports_missing_assets_instead_of_panicking() {
        let empty = std::env::temp_dir().join(format!("calyx-empty-assets-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&empty).unwrap();
        let context = test_asset_context_with_assets(vec![empty.clone()]);

        let result = SceneRenderer::new(&context.lock_read(), Default::default(), (16, 16));

        match result {
            Err(SceneRendererError::AssetLoad { name, .. }) => {
                assert_eq!(name, "shaders/mesh/pbr");
            }
            Err(error) => panic!("expected the pbr shader to fail to load, got {error:?}"),
            Ok(_) => panic!("a registry without shaders cannot build a scene renderer"),
        }
        std::fs::remove_dir_all(empty).unwrap();
    }

    #[test]
    fn construction_reports_why_a_default_mesh_failed_to_load() {
        let assets = TestAssets::engine();
        std::fs::write(
            assets.engine_root().join("meshes/cube.obj"),
            [0xff, 0xfe, 0x00],
        )
        .unwrap();
        let context = assets.asset_context();

        let result = SceneRenderer::new(&context.lock_read(), Default::default(), (16, 16));

        match result {
            Err(SceneRendererError::AssetLoad { name, .. }) => assert_eq!(name, "meshes/cube"),
            Err(error) => panic!("expected the cube's load error, got {error:?}"),
            Ok(_) => panic!("a malformed cube mesh cannot build a scene renderer"),
        }
    }

    #[test]
    fn construction_errors_describe_the_asset_and_chain_the_load_error() {
        use std::error::Error;

        let load_error = AssetError::NotFound.with_source("asset name `shaders/mesh/pbr`");
        let failed = SceneRendererError::AssetLoad {
            name: "shaders/mesh/pbr".into(),
            error: load_error.clone(),
        };
        assert_eq!(
            failed.to_string(),
            "scene renderer asset `shaders/mesh/pbr` failed to load"
        );
        let source = failed
            .source()
            .and_then(|source| source.downcast_ref::<AssetError>());
        assert_eq!(source, Some(&load_error));

        let missing = SceneRendererError::MissingAsset {
            name: "meshes/cube".into(),
        };
        assert_eq!(
            missing.to_string(),
            "scene renderer asset `meshes/cube` is missing"
        );
        assert!(missing.source().is_none());
    }

    #[test]
    fn draw_list_batches_instances_by_shader_material_and_mesh() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let (cube, sphere) = {
            let registry = read_only_context.registries.assets.read();
            (
                registry.cube().expect("missing cube mesh"),
                registry
                    .load::<Mesh>("meshes/sphere")
                    .expect("missing sphere mesh"),
            )
        };
        let first = pbr_material(&read_only_context, "test/batch_first");
        let second = pbr_material(&read_only_context, "test/batch_second");

        // Interleaved, so batching has to regroup what the scene holds apart.
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let layout = [
            (&cube, &first),
            (&sphere, &first),
            (&cube, &second),
            (&cube, &first),
            (&sphere, &first),
            (&cube, &second),
            (&cube, &first),
        ];
        for (index, (mesh, material)) in layout.iter().enumerate() {
            spawn_mesh(
                &mut builder,
                &format!("object_{index}"),
                mesh,
                material,
                vec3(index as f32, 0.0, 0.0),
            );
        }
        let hidden = builder.scene_mut().create(
            Some(ComponentID {
                name: "hidden".into(),
                visible: false,
                ..Default::default()
            }),
            None,
        );
        builder.add_component(
            hidden,
            ComponentMesh {
                mesh: AssetRef::from_id(cube.id()),
                material: AssetRef::from_id(first.id()),
                ..Default::default()
            },
        );
        let scene = builder.finish();

        let mut renderer = renderer(&read_only_context, 1, (16, 16));
        let render_context = read_only_context.render_context.clone();
        renderer.build_asset_data(&render_context, &scene);
        renderer.finish_object_ids();
        let draw_list = renderer.build_draw_list();

        let mut batches: Vec<_> = draw_list
            .iter()
            .map(|batch| (batch.mat_id, batch.mesh_id, batch.instances.len()))
            .collect();
        batches.sort();
        let mut expected = vec![
            (first.id(), cube.id(), 3),
            (first.id(), sphere.id(), 2),
            (second.id(), cube.id(), 2),
        ];
        expected.sort();
        assert_eq!(batches, expected, "one batch per shader, material and mesh");

        // Batches of one mesh take consecutive slices of its instance buffer.
        for mesh in [&cube, &sphere] {
            let mut ranges: Vec<_> = draw_list
                .iter()
                .filter(|batch| batch.mesh_id == mesh.id())
                .map(|batch| batch.instances.clone())
                .collect();
            ranges.sort_by_key(|range| range.start);
            let instances = mesh.read().instances.len() as u32;
            assert_eq!(ranges.first().map(|range| range.start), Some(0));
            assert_eq!(ranges.last().map(|range| range.end), Some(instances));
            for pair in ranges.windows(2) {
                assert_eq!(pair[0].end, pair[1].start);
            }
        }
        assert_eq!(
            renderer.object_ids.len(),
            layout.len(),
            "every visible object, and only those, gets an object id"
        );
    }

    #[test]
    fn picks_the_object_drawn_at_each_pixel() {
        const SIZE: (u32, u32) = (128, 64);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let first = pbr_material(&read_only_context, "test/pick_first");
        let second = pbr_material(&read_only_context, "test/pick_second");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let left = vec3(-3.0, 0.0, 0.0);
        let right = vec3(3.0, 0.0, 0.0);
        let left_uuid = spawn_mesh(&mut builder, "left", &cube, &first, left);
        let right_uuid = spawn_mesh(&mut builder, "right", &cube, &second, right);
        let scene = builder.finish();
        let (camera, transform) = front_camera(2.0, 10.0);

        let mut renderer = renderer(&read_only_context, 1, SIZE);
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();

        let (x, y) = project(&camera, &transform, left, SIZE);
        assert_eq!(renderer.pick_game_object(x, y), Some(left_uuid));
        let (x, y) = project(&camera, &transform, right, SIZE);
        assert_eq!(renderer.pick_game_object(x, y), Some(right_uuid));
        assert_eq!(renderer.pick_game_object(0, 0), None, "background");
        assert_eq!(renderer.pick_game_object(SIZE.0, 0), None, "out of bounds");

        // The non-blocking pick answers once its readback lands.
        let (x, y) = project(&camera, &transform, left, SIZE);
        let device = read_only_context.render_context.device();
        let mut picked = None;
        for _ in 0..100 {
            picked = renderer.request_pick_game_object(x, y);
            if picked == Some(left_uuid) {
                break;
            }
            let _ = device.poll(wgpu::Maintain::Wait);
        }
        assert_eq!(picked, Some(left_uuid));
    }

    /// Registers a copy of the cube mesh skinned entirely to one bone, `bone`, whose bind pose is
    /// `bind`.
    fn skinned_cube(
        context: &ReadOnlyAssetContext,
        name: &str,
        bone: &str,
        bind: Mat4,
    ) -> Ref<Mesh> {
        let registry = context.registries.assets.read();
        let cube = registry.cube().expect("missing cube mesh");
        let mut mesh = Mesh::new(&context.render_context);
        {
            let cube = cube.read();
            mesh.indices = cube.indices.clone();
            mesh.vertices = cube.vertices.clone();
            mesh.normals = cube.normals.clone();
            mesh.tangents = cube.tangents.clone();
            mesh.uvs = cube.uvs.clone();
        }
        mesh.bone_indices = vec![glm::IVec4::new(0, -1, -1, -1); mesh.vertices.len()];
        mesh.bone_weights = vec![vec4(1.0, 0.0, 0.0, 0.0); mesh.vertices.len()];
        mesh.bones.insert(
            bone.into(),
            crate::assets::mesh::BoneInfo {
                index: 0,
                inverse_bind_transform: glm::inverse(&bind),
            },
        );
        mesh.mark_dirty();
        registry
            .create_or_update(name.into(), mesh)
            .expect("skinned mesh should be created")
    }

    /// A skinned model as imported models are laid out: a root holding the bone `hip` at local
    /// transform `hip`, and a skinned object drawing `mesh` with `palette`, placed by the root.
    /// Returns the root, the bone and the skinned object.
    fn spawn_skinned_model(
        builder: &mut SceneBuilder,
        mesh: &Ref<Mesh>,
        material: &Ref<Material>,
        root: Mat4,
        hip: Mat4,
        palette: Vec<crate::assets::mesh::BoneTransform>,
    ) -> (GameObject, GameObject, GameObject) {
        let root_object = builder.spawn("model");
        builder.set_transform(root_object, root);
        let hip_object = builder.spawn_child(root_object, "hip");
        builder.set_transform(hip_object, hip);
        builder.add_component(
            hip_object,
            crate::component::ComponentBone {
                name: "hip".into(),
                bind_pose: None,
            },
        );
        let mesh_object = builder.spawn_child(root_object, "mesh");
        let root_uuid = builder.scene().uuid(root_object);
        builder.add_component(
            mesh_object,
            ComponentSkinnedMesh {
                mesh: AssetRef::from_id(mesh.id()),
                material: AssetRef::from_id(material.id()),
                root_bone: crate::scene::GameObjectRef::new(root_uuid),
                bone_transforms: palette,
                ..Default::default()
            },
        );
        (root_object, hip_object, mesh_object)
    }

    /// The slot in its mesh's bone buffer that `game_object`'s skinned draw reads.
    fn bone_slot(renderer: &SceneRenderer, scene: &Scene, game_object: GameObject) -> usize {
        let object_id = renderer.object_id_lookup[&scene.uuid(game_object)];
        let draw = renderer
            .draw_list
            .iter()
            .find(|draw| draw.object_id == object_id)
            .expect("the skinned mesh should be queued");
        usize::try_from(draw.bone_transform_index).expect("a skinned draw has a bone slot")
    }

    #[test]
    fn skinned_mesh_draws_its_rest_pose_before_any_animator_update() {
        const SIZE: (u32, u32) = (32, 32);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let bind = glm::translation(&vec3(1.0, 0.0, 0.0));
        let mesh = skinned_cube(&read_only_context, "test/skinned_cube", "hip", bind);
        let material = pbr_material(&read_only_context, "test/skinned");
        // No animator and no palette: the scene as loaded in edit mode. The bone holds its bind
        // pose, so the rest pose draws the cube where the mesh models it, at the origin.
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let (_, _, mesh_object) = spawn_skinned_model(
            &mut builder,
            &mesh,
            &material,
            Mat4::identity(),
            bind,
            Vec::new(),
        );
        let mesh_uuid = builder.scene().uuid(mesh_object);
        let scene = builder.finish();

        let mut renderer = renderer(&read_only_context, 1, SIZE);
        let (camera, transform) = front_camera(1.0, 5.0);
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();

        assert_eq!(
            renderer.pick_game_object(SIZE.0 / 2, SIZE.1 / 2),
            Some(mesh_uuid),
            "the skinned mesh should draw in its rest pose at the origin"
        );
    }

    /// Renders `scene` and picks the pixel at each of `pixels`.
    fn render_and_pick<const N: usize>(
        renderer: &mut SceneRenderer,
        scene: &Scene,
        (camera, transform): &(Camera, Transform),
        pixels: [(u32, u32); N],
    ) -> [Option<Uuid>; N] {
        renderer.render_scene_base(camera, transform, scene, None, None);
        renderer.finalize_scene();
        pixels.map(|(x, y)| renderer.pick_game_object(x, y))
    }

    #[test]
    fn disabled_meshes_are_neither_drawn_nor_picked_until_re_enabled() {
        const SIZE: (u32, u32) = (128, 64);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/disabled_meshes");
        let bind = glm::translation(&vec3(1.0, 0.0, 0.0));
        let skinned = skinned_cube(&read_only_context, "test/disabled_skinned", "hip", bind);
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let left = vec3(-3.0, 0.0, 0.0);
        let mesh_uuid = spawn_mesh(&mut builder, "mesh", &cube, &material, left);
        // The skinned cube draws in its rest pose at the origin.
        let (_, _, skinned_object) = spawn_skinned_model(
            &mut builder,
            &skinned,
            &material,
            Mat4::identity(),
            bind,
            Vec::new(),
        );
        let skinned_uuid = builder.scene().uuid(skinned_object);
        let mut scene = builder.finish();
        let mesh_object = scene.find(mesh_uuid).unwrap();
        let view = front_camera(2.0, 10.0);
        let pixels = [
            project(&view.0, &view.1, left, SIZE),
            project(&view.0, &view.1, Vec3::zeros(), SIZE),
        ];
        let mut renderer = renderer(&read_only_context, 1, SIZE);

        assert_eq!(
            render_and_pick(&mut renderer, &scene, &view, pixels),
            [Some(mesh_uuid), Some(skinned_uuid)]
        );

        assert!(scene.set_component_enabled::<ComponentMesh>(mesh_object, false));
        assert!(scene.set_component_enabled::<ComponentSkinnedMesh>(skinned_object, false));
        assert_eq!(
            render_and_pick(&mut renderer, &scene, &view, pixels),
            [None, None]
        );
        assert!(
            renderer.draw_list.is_empty(),
            "a disabled mesh queues no draw, so it is neither drawn nor outlined"
        );

        assert!(scene.set_component_enabled::<ComponentMesh>(mesh_object, true));
        assert!(scene.set_component_enabled::<ComponentSkinnedMesh>(skinned_object, true));
        assert_eq!(
            render_and_pick(&mut renderer, &scene, &view, pixels),
            [Some(mesh_uuid), Some(skinned_uuid)]
        );
    }

    #[test]
    fn a_skinned_normal_mapped_mesh_skins_its_tangents() {
        const SIZE: (u32, u32) = (32, 32);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        // A normal texture that bends every normal far along the tangent, lit from one side,
        // so the shading of the face towards the camera follows where its tangent points.
        let material = {
            let registry = read_only_context.registries.assets.read();
            let shader = registry
                .load::<Shader>("shaders/mesh/pbr")
                .expect("missing pbr shader");
            let mut material = Material::from_shader(&read_only_context, shader);
            for variable in material.variables.iter_mut() {
                match variable.name.as_str() {
                    "normal_texture" => {
                        variable.value = ShaderVariableValue::Texture2D(MaterialTexture::Color([
                            0.85, 0.5, 0.7, 1.0,
                        ]))
                    }
                    "roughness" => variable.value = ShaderVariableValue::Float(1.0),
                    _ => {}
                }
            }
            registry
                .create_or_update("test/bent_normals".into(), material)
                .expect("material should be created")
        };
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let skinned = skinned_cube(
            &read_only_context,
            "test/skinned_tangents",
            "hip",
            Mat4::identity(),
        );
        let turn = glm::rotation(90f32.to_radians(), &Vec3::z());
        let light = glm::rotation(40f32.to_radians(), &Vec3::y());
        let (camera, transform) = front_camera(1.0, 5.0);
        let center = |build: &dyn Fn(&mut SceneBuilder)| {
            let mut builder = SceneBuilder::new(read_only_context.registries.clone());
            let sun = builder.spawn("sun");
            builder.add_component(
                sun,
                crate::component::ComponentDirectionalLight {
                    intensity: 1.0,
                    ..Default::default()
                },
            );
            builder.set_transform(sun, light);
            build(&mut builder);
            let scene = builder.finish();
            let mut renderer = renderer(&read_only_context, 1, SIZE);
            renderer.render_scene_base(&camera, &transform, &scene, None, None);
            renderer.finalize_scene();
            read_scene_texture(&renderer, &render_context)
                .get_pixel(SIZE.0 / 2, SIZE.1 / 2)
                .0
        };

        let still = center(&|builder| {
            spawn_mesh(builder, "cube", &cube, &material, Vec3::zeros());
        });
        let turned = center(&|builder| {
            let object = builder.spawn("cube");
            builder.add_component(
                object,
                ComponentMesh {
                    mesh: AssetRef::from_id(cube.id()),
                    material: AssetRef::from_id(material.id()),
                    ..Default::default()
                },
            );
            builder.set_transform(object, turn);
        });
        // The bone holds the quarter turn, so the rest pose turns the cube as the object above.
        let skinned_turned = center(&|builder| {
            spawn_skinned_model(
                builder,
                &skinned,
                &material,
                Mat4::identity(),
                turn,
                Vec::new(),
            );
        });

        let difference = |a: [u8; 4], b: [u8; 4]| {
            a.iter()
                .zip(b)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap_or(0)
        };
        assert!(
            difference(still, turned) > 10,
            "turning the tangents changes the shading: {still:?} and {turned:?}"
        );
        assert!(
            difference(turned, skinned_turned) <= 2,
            "skinning turns the tangents as the object transform does: {turned:?} and \
             {skinned_turned:?}"
        );
    }

    #[test]
    fn rest_pose_palette_is_each_bone_in_root_space_times_its_inverse_bind() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let bind = glm::translation(&vec3(1.0, 0.0, 0.0));
        let mesh = skinned_cube(&read_only_context, "test/rest_palette", "hip", bind);
        let material = pbr_material(&read_only_context, "test/rest_palette_material");
        let root =
            glm::translation(&vec3(5.0, 0.0, 0.0)) * glm::rotation(90f32.to_radians(), &Vec3::y());
        let hip =
            glm::translation(&vec3(0.0, 2.0, 0.0)) * glm::rotation(30f32.to_radians(), &Vec3::z());
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let (root_object, hip_object, mesh_object) =
            spawn_skinned_model(&mut builder, &mesh, &material, root, hip, Vec::new());
        let scene = builder.finish();
        let hip_before = scene.transform(hip_object).matrix();

        let mut renderer = renderer(&read_only_context, 1, (16, 16));
        renderer.build_asset_data(&read_only_context.render_context, &scene);

        let expected = scene.world_transform(root_object).inverse_matrix()
            * scene.world_transform(hip_object).matrix()
            * glm::inverse(&bind);
        let slot = bone_slot(&renderer, &scene, mesh_object);
        let uploaded: Mat4 = mesh.read().bone_transforms[slot].transform.into();
        assert!(
            (uploaded - expected).abs().max() < 1e-5,
            "expected {expected}, uploaded {uploaded}"
        );
        // Drawing the rest pose reads the scene and writes nothing back into it.
        let palette_untouched = scene
            .read_component::<ComponentSkinnedMesh, _, _>(mesh_object, |c| {
                c.bone_transforms.is_empty()
            })
            .unwrap();
        assert!(palette_untouched);
        assert_eq!(scene.transform(hip_object).matrix(), hip_before);
        let bind_pose_untouched = scene
            .read_component::<crate::component::ComponentBone, _, _>(hip_object, |c| {
                c.bind_pose.is_none()
            })
            .unwrap();
        assert!(bind_pose_untouched);
    }

    #[test]
    fn rest_pose_keeps_the_shear_of_a_non_uniformly_scaled_root() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        // A rotated bone under a non-uniformly scaled root has a sheared world matrix. The bone
        // sits exactly at its bind pose in root space, so its palette entry is the identity.
        let hip =
            glm::translation(&vec3(0.0, 1.0, 0.0)) * glm::rotation(45f32.to_radians(), &Vec3::z());
        let mesh = skinned_cube(&read_only_context, "test/sheared_rest", "hip", hip);
        let material = pbr_material(&read_only_context, "test/sheared_rest_material");
        let root = glm::scaling(&vec3(2.0, 1.0, 1.0));
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let (_, _, mesh_object) =
            spawn_skinned_model(&mut builder, &mesh, &material, root, hip, Vec::new());
        let scene = builder.finish();

        let mut renderer = renderer(&read_only_context, 1, (16, 16));
        renderer.build_asset_data(&read_only_context.render_context, &scene);

        let slot = bone_slot(&renderer, &scene, mesh_object);
        let uploaded: Mat4 = mesh.read().bone_transforms[slot].transform.into();
        assert!(
            (uploaded - Mat4::identity()).abs().max() < 1e-5,
            "a bone at its bind pose should skin to the identity, uploaded {uploaded}"
        );
    }

    #[test]
    fn a_short_palette_keeps_the_next_instance_in_its_own_bone_slot() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let bind = glm::translation(&vec3(1.0, 0.0, 0.0));
        let mesh = skinned_cube(&read_only_context, "test/short_palette", "hip", bind);
        let material = pbr_material(&read_only_context, "test/short_palette_material");
        let animated: Mat4 = glm::translation(&vec3(0.0, 3.0, 0.0));
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let (_, _, rest) = spawn_skinned_model(
            &mut builder,
            &mesh,
            &material,
            Mat4::identity(),
            bind,
            Vec::new(),
        );
        let (_, _, posed) = spawn_skinned_model(
            &mut builder,
            &mesh,
            &material,
            Mat4::identity(),
            bind,
            vec![crate::assets::mesh::BoneTransform {
                transform: animated.into(),
            }],
        );
        let scene = builder.finish();

        let mut renderer = renderer(&read_only_context, 1, (16, 16));
        renderer.build_asset_data(&read_only_context.render_context, &scene);

        let mesh = mesh.read();
        assert_eq!(mesh.bone_transforms.len(), 2, "one bone set per instance");
        let rest_slot = bone_slot(&renderer, &scene, rest);
        let posed_slot = bone_slot(&renderer, &scene, posed);
        let mut slots = [rest_slot, posed_slot];
        slots.sort();
        assert_eq!(slots, [0, 1]);
        let rest_transform: Mat4 = mesh.bone_transforms[rest_slot].transform.into();
        let posed_transform: Mat4 = mesh.bone_transforms[posed_slot].transform.into();
        assert!((rest_transform - Mat4::identity()).abs().max() < 1e-5);
        assert_eq!(posed_transform, animated, "a full palette is drawn as is");
    }

    #[test]
    fn resize_renders_and_picks_at_the_new_size() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/resize");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let uuid = spawn_mesh(&mut builder, "cube", &cube, &material, Vec3::zeros());
        let scene = builder.finish();

        let mut renderer = renderer(&read_only_context, 1, (32, 32));
        let (camera, transform) = front_camera(1.0, 5.0);
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();
        assert_eq!(renderer.pick_game_object(16, 16), Some(uuid));
        // A readback still in flight when the targets change is dropped.
        renderer.request_pick_game_object(16, 16);

        renderer.resize_textures(96, 48);
        assert_eq!(renderer.scene_texture_size(), (96, 48));
        let (camera, transform) = front_camera(2.0, 5.0);
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();

        assert_eq!(renderer.pick_game_object(48, 24), Some(uuid));
        assert_eq!(renderer.pick_game_object(95, 0), None);
        assert_eq!(renderer.pick_game_object(96, 24), None, "out of bounds");
        let render_context = &read_only_context.render_context;
        let mut readback = renderer
            .submit_scene_texture_readback()
            .expect("scene texture has a size");
        let image = loop {
            let _ = render_context.device().poll(wgpu::Maintain::Wait);
            match readback.try_finish(render_context) {
                ReadbackPoll::Pending => continue,
                ReadbackPoll::Ready(image) => break image,
                ReadbackPoll::Failed(error) => panic!("scene readback failed: {error}"),
            }
        };
        assert_eq!(image.dimensions(), (96, 48));
    }

    #[test]
    fn assets_not_drawn_in_a_frame_are_dropped() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let (sphere, cube) = {
            let registry = read_only_context.registries.assets.read();
            (
                registry.sphere().expect("missing sphere mesh"),
                registry.cube().expect("missing cube mesh"),
            )
        };
        let first = pbr_material(&read_only_context, "test/drop_first");
        let second = pbr_material(&read_only_context, "test/drop_second");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_mesh(&mut builder, "sphere", &sphere, &first, Vec3::zeros());
        let sphere_scene = builder.finish();
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_mesh(&mut builder, "cube", &cube, &second, Vec3::zeros());
        let cube_scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);

        let mut renderer = renderer(&read_only_context, 1, (32, 32));
        renderer.render_scene_base(&camera, &transform, &sphere_scene, None, None);
        assert!(renderer.assets.contains(sphere.id()));
        assert!(renderer.assets.contains(first.id()));

        renderer.render_scene_base(&camera, &transform, &cube_scene, None, None);
        assert!(!renderer.assets.contains(sphere.id()));
        assert!(!renderer.assets.contains(first.id()));
        assert!(renderer.assets.contains(cube.id()));
        assert!(renderer.assets.contains(second.id()));
    }

    #[test]
    fn the_outline_keeps_its_color_through_post_processing_and_picking_is_unchanged() {
        const SIZE: (u32, u32) = (64, 64);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/outline_color");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let uuid = spawn_mesh(&mut builder, "cube", &cube, &material, Vec3::zeros());
        // +3 EV through ACES would change any color the chain touched.
        let volume = builder.spawn("volume");
        builder.add_component(volume, ComponentGlobalPostProcessVolume::default());
        builder.add_component(
            volume,
            ComponentTonemap {
                exposure_ev: 3.0,
                ..Default::default()
            },
        );
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);
        let (center_x, center_y) = project(&camera, &transform, Vec3::zeros(), SIZE);

        for samples in [1, 4] {
            let flags = test_adapter()
                .get_texture_format_features(wgpu::TextureFormat::Rgba16Float)
                .flags;
            if !flags.sample_count_supported(samples) {
                eprintln!("skipping {samples} samples: unsupported by the test adapter");
                continue;
            }
            let mut renderer = SceneRenderer::new(
                &read_only_context,
                SceneRendererOptions {
                    samples,
                    clear_color: Color32::BLACK,
                    ..Default::default()
                },
                SIZE,
            )
            .expect("scene renderer should load its assets");
            renderer.set_selected_game_object(Some(uuid));
            renderer.render_scene_base(&camera, &transform, &scene, None, None);
            renderer.finalize_scene();
            assert_eq!(
                renderer.post_process_settings().tonemapping.exposure_ev,
                3.0
            );
            let image = read_scene_texture(&renderer, &render_context);

            // Black tone maps to black, so scanning in from the right edge,
            // the first other pixel is the outline right of the cube.
            let edge = (0..SIZE.0)
                .rev()
                .find(|&x| image.get_pixel(x, center_y).0 != [0, 0, 0, 255])
                .expect("the cube's row has an outline");
            assert!(edge > center_x, "the outline lies right of the cube");
            assert_eq!(
                image.get_pixel(edge, center_y).0,
                [255, 149, 0, 255],
                "the selection outline keeps its authored color at {samples} samples"
            );

            assert_eq!(
                renderer.pick_game_object(center_x, center_y),
                Some(uuid),
                "the cube is picked under its center at {samples} samples"
            );
            assert_eq!(
                renderer.pick_game_object(edge, center_y),
                None,
                "the outline is not pickable at {samples} samples"
            );
        }
    }

    /// A PBR material registered as `name`, emitting white at strength one through `emissive`
    /// in its emissive slot.
    fn emissive_material(
        context: &ReadOnlyAssetContext,
        name: &str,
        emissive: MaterialTexture,
    ) -> Ref<Material> {
        let registry = context.registries.assets.read();
        let shader = registry
            .load::<Shader>("shaders/mesh/pbr")
            .expect("missing pbr shader");
        let mut material = Material::from_shader(context, shader);
        for variable in material.variables.iter_mut() {
            match variable.name.as_str() {
                "emission_color" => variable.value = ShaderVariableValue::Color([1.0; 4]),
                "emission_strength" => variable.value = ShaderVariableValue::Float(1.0),
                "emissive_texture" => {
                    variable.value = ShaderVariableValue::Texture2D(emissive.clone())
                }
                _ => {}
            }
        }
        registry
            .create_or_update(name.into(), material)
            .expect("material should be created")
    }

    #[test]
    fn an_image_in_the_emissive_slot_decodes_as_srgb_and_a_solid_color_stays_linear() {
        const SIZE: (u32, u32) = (96, 64);
        // sRGB 188 decodes to linear 0.503.
        const ENCODED: u8 = 188;
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let registry = read_only_context.registries.assets.read();
        let cube = registry.cube().expect("missing cube mesh");
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([ENCODED, ENCODED, ENCODED, 255]),
        ));
        let texture = Texture::from_image(&read_only_context, "test/srgb_gray", image)
            .expect("the image uploads");
        let texture = registry
            .create_or_update("test/srgb_gray".into(), texture)
            .expect("texture should be created");
        drop(registry);
        let decoded = ((f32::from(ENCODED) / 255.0 + 0.055) / 1.055).powf(2.4);
        let image_material = emissive_material(
            &read_only_context,
            "test/emissive_image",
            MaterialTexture::Asset(AssetRef::from_id(texture.id())),
        );
        let decoded_material = emissive_material(
            &read_only_context,
            "test/emissive_decoded",
            MaterialTexture::Color([decoded, decoded, decoded, 1.0]),
        );
        let raw = f32::from(ENCODED) / 255.0;
        let raw_material = emissive_material(
            &read_only_context,
            "test/emissive_raw",
            MaterialTexture::Color([raw, raw, raw, 1.0]),
        );
        let left = vec3(-1.6, 0.0, 0.0);
        let middle = vec3(0.0, 0.0, 0.0);
        let right = vec3(1.6, 0.0, 0.0);
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_mesh(&mut builder, "image", &cube, &image_material, left);
        spawn_mesh(&mut builder, "decoded", &cube, &decoded_material, middle);
        spawn_mesh(&mut builder, "raw", &cube, &raw_material, right);
        let scene = builder.finish();
        let (camera, transform) = front_camera(SIZE.0 as f32 / SIZE.1 as f32, 5.0);

        let mut renderer = SceneRenderer::new(
            &read_only_context,
            SceneRendererOptions {
                clear_color: Color32::BLACK,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();
        let image = read_scene_texture(&renderer, &render_context);
        let pixel = |position| {
            let (x, y) = project(&camera, &transform, position, SIZE);
            image.get_pixel(x, y).0[0]
        };

        let (image, decoded, raw) = (pixel(left), pixel(middle), pixel(right));
        assert!(
            image.abs_diff(decoded) <= 2,
            "the image emits its decoded value: image {image}, decoded {decoded}"
        );
        assert!(
            raw > decoded + 10,
            "the raw value is brighter than the decoded one: raw {raw}, decoded {decoded}"
        );
    }

    #[test]
    fn an_emissive_material_glows_without_light_and_a_default_one_stays_black() {
        const SIZE: (u32, u32) = (64, 64);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        // A material made from the shader reflects the emission fields with
        // zero strength, so it does not emit.
        let plain = pbr_material(&read_only_context, "test/emission_off");
        let emissive = {
            let registry = read_only_context.registries.assets.read();
            let shader = registry
                .load::<Shader>("shaders/mesh/pbr")
                .expect("missing pbr shader");
            let mut material = Material::from_shader(&read_only_context, shader);
            for variable in material.variables.iter_mut() {
                match variable.name.as_str() {
                    "emission_color" => {
                        variable.value = ShaderVariableValue::Color([0.25, 0.5, 1.0, 1.0])
                    }
                    "emission_strength" => variable.value = ShaderVariableValue::Float(2.0),
                    _ => {}
                }
            }
            registry
                .create_or_update("test/emission_on".into(), material)
                .expect("material should be created")
        };
        let left = vec3(-1.0, 0.0, 0.0);
        let right = vec3(1.0, 0.0, 0.0);
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_mesh(&mut builder, "plain", &cube, &plain, left);
        spawn_mesh(&mut builder, "emissive", &cube, &emissive, right);
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);

        let mut renderer = SceneRenderer::new(
            &read_only_context,
            SceneRendererOptions {
                clear_color: Color32::BLACK,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();
        let image = read_scene_texture(&renderer, &render_context);

        let (x, y) = project(&camera, &transform, left, SIZE);
        assert_eq!(
            image.get_pixel(x, y).0,
            [0, 0, 0, 255],
            "an unlit cube without emission is black"
        );
        let (x, y) = project(&camera, &transform, right, SIZE);
        let [r, g, b, _] = image.get_pixel(x, y).0;
        assert!(
            b > g && g > r && r > 0,
            "the emissive cube shows its emission color, got ({r}, {g}, {b})"
        );
    }

    /// Renders a wide flat box facing the camera, lit only by a spot light 2 units in front of
    /// it pointing straight at it, and returns the red channel at each of `points` on its face.
    /// With `other_lights`, the scene also holds a black directional light shining at the face
    /// and a point light out of range of it, which light nothing but come before the spot light
    /// in the light storage buffer.
    fn spot_lit_plane<const N: usize>(
        inner_angle: f32,
        outer_angle: f32,
        other_lights: bool,
        points: [Vec3; N],
    ) -> [u8; N] {
        const SIZE: (u32, u32) = (96, 96);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/spot_lit_plane");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let plane = builder.spawn("plane");
        builder.add_component(
            plane,
            ComponentMesh {
                mesh: AssetRef::from_id(cube.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        // The cube spans -1..1, so the face towards the camera sits at z = -0.05.
        builder.set_transform(plane, glm::scaling(&vec3(3.0, 3.0, 0.05)));
        let light = builder.spawn("spot");
        builder.add_component(
            light,
            crate::component::ComponentSpotLight {
                intensity: 20.0,
                range: 10.0,
                inner_angle,
                outer_angle,
                ..Default::default()
            },
        );
        // An identity rotation points the light along +Z, at the plane.
        builder.set_transform(light, glm::translation(&vec3(0.0, 0.0, -2.05)));
        if other_lights {
            let sun = builder.spawn("sun");
            builder.add_component(
                sun,
                crate::component::ComponentDirectionalLight {
                    color: Color32::BLACK,
                    intensity: 20.0,
                    ..Default::default()
                },
            );
            let lamp = builder.spawn("lamp");
            builder.add_component(
                lamp,
                crate::component::ComponentPointLight {
                    intensity: 20.0,
                    radius: 1.0,
                    ..Default::default()
                },
            );
            builder.set_transform(lamp, glm::translation(&vec3(0.0, 0.0, -20.0)));
        }
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);
        let mut renderer = SceneRenderer::new(
            &read_only_context,
            SceneRendererOptions {
                clear_color: Color32::BLACK,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();
        let image = read_scene_texture(&renderer, &render_context);
        points.map(|point| {
            let (x, y) = project(&camera, &transform, point, SIZE);
            image.get_pixel(x, y).0[0]
        })
    }

    /// The point on the plane's face `degrees` off the spot light's axis.
    fn off_axis(degrees: f32) -> Vec3 {
        vec3(2.0 * degrees.to_radians().tan(), 0.0, -0.05)
    }

    #[test]
    fn a_spot_light_lights_its_inner_cone_fully_fades_across_the_band_and_stops_outside() {
        let [axis, inner, band, outside] = spot_lit_plane(
            10.0,
            30.0,
            false,
            [off_axis(0.0), off_axis(6.0), off_axis(20.0), off_axis(40.0)],
        );
        assert!(axis > 40, "the plane is lit on the cone axis, got {axis}");
        assert!(
            inner.abs_diff(axis) <= 12,
            "inside the inner cone the light is full: axis {axis}, inner {inner}"
        );
        assert!(
            band > 0 && band + 10 < inner,
            "the falloff band is dimmer than the inner cone but lit: inner {inner}, band {band}"
        );
        assert_eq!(outside, 0, "outside the outer cone the plane is dark");
    }

    #[test]
    fn a_spot_light_with_equal_cones_has_a_hard_edge() {
        let [axis, edge_inside, edge_outside] = spot_lit_plane(
            25.0,
            25.0,
            false,
            [off_axis(0.0), off_axis(22.0), off_axis(28.0)],
        );
        assert!(axis > 40, "the plane is lit on the cone axis, got {axis}");
        assert!(
            edge_inside > 20,
            "just inside the cone is lit, not NaN black: {edge_inside}"
        );
        assert_eq!(edge_outside, 0, "just outside the cone is dark");
    }

    #[test]
    fn a_spot_light_after_directional_and_point_lights_shades_from_its_own_range() {
        let [alone_axis, alone_outside] =
            spot_lit_plane(10.0, 30.0, false, [off_axis(0.0), off_axis(40.0)]);
        let [axis, outside] = spot_lit_plane(10.0, 30.0, true, [off_axis(0.0), off_axis(40.0)]);
        assert!(
            axis > 40,
            "the spot light still lights its axis, got {axis}"
        );
        assert!(
            axis.abs_diff(alone_axis) <= 2,
            "the other lights add nothing on the axis: {alone_axis} alone, {axis} with them"
        );
        assert_eq!(
            (alone_outside, outside),
            (0, 0),
            "outside the cone stays dark, so no loop read the spot light as another kind"
        );
    }

    #[test]
    fn only_views_that_show_an_overlay_allocate_the_overlay_target() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let scene = SceneBuilder::new(read_only_context.registries.clone()).finish();
        let (camera, transform) = front_camera(1.0, 5.0);
        let frame = |options: SceneRendererOptions| {
            let mut renderer = SceneRenderer::new(&read_only_context, options, (16, 16))
                .expect("scene renderer should load its assets");
            renderer.render_scene_base(&camera, &transform, &scene, None, None);
            renderer.finalize_scene();
            renderer
        };

        // The Game tab, the player and thumbnails show no overlay.
        let game = frame(SceneRendererOptions::default());
        assert!(game.overlay().target().is_none());

        let editor = frame(SceneRendererOptions {
            grid: true,
            gizmos: true,
            ..Default::default()
        });
        let target = editor
            .overlay()
            .target()
            .expect("the editor shows overlays");
        assert_eq!(target.descriptor.format, OverlayCompositor::FORMAT);
    }

    #[test]
    fn an_inactive_outline_pass_releases_its_bind_group() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/outline");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let uuid = spawn_mesh(&mut builder, "cube", &cube, &material, Vec3::zeros());
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);
        let mut renderer = renderer(&read_only_context, 1, (32, 32));
        let frame = |renderer: &mut SceneRenderer| {
            renderer.render_scene_base(&camera, &transform, &scene, None, None);
            renderer.finalize_scene();
        };

        renderer.set_hovered_game_object(Some(uuid));
        renderer.set_selected_game_object(Some(uuid));
        frame(&mut renderer);
        assert!(renderer.outline_renderer.hovered_pass.is_cached());
        assert!(renderer.outline_renderer.selected_pass.is_cached());

        renderer.set_hovered_game_object(None);
        frame(&mut renderer);
        assert!(!renderer.outline_renderer.hovered_pass.is_cached());
        assert!(renderer.outline_renderer.selected_pass.is_cached());

        renderer.set_selected_game_object(None);
        frame(&mut renderer);
        assert!(!renderer.outline_renderer.selected_pass.is_cached());
    }

    #[test]
    fn deleting_a_drawn_mesh_releases_it_from_the_renderer() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let sphere = read_only_context
            .registries
            .assets
            .read()
            .sphere()
            .expect("missing sphere mesh");
        let sphere_id = sphere.id();
        let material = pbr_material(&read_only_context, "test/deleted_mesh");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let uuid = spawn_mesh(&mut builder, "sphere", &sphere, &material, Vec3::zeros());
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);

        let mut renderer = renderer(&read_only_context, 1, (32, 32));
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();
        assert_eq!(renderer.pick_game_object(16, 16), Some(uuid));
        assert!(renderer.assets.contains(sphere_id));
        let sphere_weak = sphere.downgrade();
        drop(sphere);

        let path = assets.engine_root().join("meshes/sphere.obj");
        std::fs::remove_file(&path).expect("failed to delete the sphere mesh");
        read_only_context
            .registries
            .assets
            .read()
            .recv_notify_event(
                notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::File))
                    .add_path(path),
            );
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();

        assert!(!renderer.assets.contains(sphere_id));
        assert!(
            sphere_weak.upgrade().is_none(),
            "nothing keeps the deleted mesh alive"
        );
        assert_eq!(renderer.pick_game_object(16, 16), None);
    }

    #[test]
    fn renders_more_instances_of_one_mesh_than_the_instance_cap() {
        const COLUMNS: u32 = 9;
        const ROWS: u32 = 5;
        const SPACING: f32 = 3.0;
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/instance_cap_material");

        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let mut expected = Vec::new();
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let position = vec3(
                    (column as f32 - (COLUMNS - 1) as f32 / 2.0) * SPACING,
                    (row as f32 - (ROWS - 1) as f32 / 2.0) * SPACING,
                    0.0,
                );
                let uuid = spawn_mesh(
                    &mut builder,
                    &format!("cube_{row}_{column}"),
                    &cube,
                    &material,
                    position,
                );
                expected.push((position, uuid));
            }
        }
        // More than the 30 instances one uniform buffer held before instances
        // moved to a storage buffer.
        assert!(expected.len() > 30);
        let scene = builder.finish();

        let (camera, camera_transform) = front_camera(1.0, 30.0);
        let mut renderer = renderer(&read_only_context, 1, (VIEWPORT, VIEWPORT));
        renderer.render_scene_base(&camera, &camera_transform, &scene, None, None);

        for (index, (position, uuid)) in expected.iter().enumerate() {
            let (x, y) = project(&camera, &camera_transform, *position, (VIEWPORT, VIEWPORT));
            assert_eq!(
                renderer.pick_game_object(x, y),
                Some(*uuid),
                "instance {index} was not drawn at its transform"
            );
        }
    }

    #[test]
    fn renders_a_thousand_instances_of_one_mesh_in_one_batch() {
        const COLUMNS: u32 = 40;
        const ROWS: u32 = 25;
        const SPACING: f32 = 3.0;
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/thousand_instances_material");

        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let mut expected = Vec::new();
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let position = vec3(
                    (column as f32 - (COLUMNS - 1) as f32 / 2.0) * SPACING,
                    (row as f32 - (ROWS - 1) as f32 / 2.0) * SPACING,
                    0.0,
                );
                let uuid = spawn_mesh(
                    &mut builder,
                    &format!("cube_{row}_{column}"),
                    &cube,
                    &material,
                    position,
                );
                expected.push((position, uuid));
            }
        }
        assert_eq!(expected.len(), 1000);
        let scene = builder.finish();

        let render_context = read_only_context.render_context.clone();
        let mut batching = renderer(&read_only_context, 1, (16, 16));
        batching.build_asset_data(&render_context, &scene);
        batching.finish_object_ids();
        let draw_list = batching.build_draw_list();
        let batches: Vec<_> = draw_list
            .iter()
            .map(|batch| (batch.mat_id, batch.mesh_id, batch.instances.clone()))
            .collect();
        assert_eq!(
            batches,
            vec![(material.id(), cube.id(), 0..1000)],
            "every instance of the mesh is drawn by one batch"
        );

        let (camera, camera_transform) = front_camera(1.0, 110.0);
        let mut renderer = renderer(&read_only_context, 1, (VIEWPORT, VIEWPORT));
        renderer.render_scene_base(&camera, &camera_transform, &scene, None, None);
        for (index, (position, uuid)) in expected.iter().enumerate() {
            let (x, y) = project(&camera, &camera_transform, *position, (VIEWPORT, VIEWPORT));
            assert_eq!(
                renderer.pick_game_object(x, y),
                Some(*uuid),
                "instance {index} was not drawn at its transform"
            );
        }
    }

    /// Whether the test device can render to and resolve `format` at `samples`.
    fn supports_samples(
        render_context: &RenderContext,
        format: wgpu::TextureFormat,
        samples: u32,
        resolve: bool,
    ) -> bool {
        let features = render_context.device().features();
        let format_features =
            if features.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
                test_adapter().get_texture_format_features(format)
            } else {
                format.guaranteed_format_features(features)
            };
        format_features.flags.sample_count_supported(samples)
            && (!resolve
                || format_features
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
    }

    #[test]
    fn renders_and_resolves_a_frame_at_four_samples() {
        const SAMPLES: u32 = 4;
        const SIZE: u32 = 64;
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        if !supports_samples(
            &render_context,
            wgpu::TextureFormat::Rgba16Float,
            SAMPLES,
            true,
        ) || !supports_samples(
            &render_context,
            wgpu::TextureFormat::Depth32Float,
            SAMPLES,
            false,
        ) {
            eprintln!(
                "skipping: test adapter {:?} cannot render Rgba16Float and Depth32Float at \
                 {SAMPLES}x MSAA",
                test_adapter().get_info().name
            );
            return;
        }

        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/msaa_material");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let game_object = builder.spawn("cube");
        builder.add_component(
            game_object,
            ComponentMesh {
                mesh: AssetRef::from_id(cube.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        // Rotated so its silhouette has diagonal edges that only partially
        // cover the pixels they cross.
        builder.set_transform(
            game_object,
            glm::rotation(30f32.to_radians(), &vec3(0.0, 0.0, 1.0)),
        );
        let uuid = builder.scene().uuid(game_object);
        let scene = builder.finish();

        let (camera, camera_transform) = front_camera(1.0, 5.0);
        let mut renderer = SceneRenderer::new(
            &read_only_context,
            SceneRendererOptions {
                samples: SAMPLES,
                clear_color: Color32::WHITE,
                ..Default::default()
            },
            (SIZE, SIZE),
        )
        .expect("scene renderer should load its assets");
        renderer.render_scene_base(&camera, &camera_transform, &scene, None, None);
        renderer.finalize_scene();

        let mut readback = renderer
            .submit_scene_texture_readback()
            .expect("scene texture has a size");
        let image = loop {
            let _ = render_context.device().poll(wgpu::Maintain::Wait);
            match readback.try_finish(&render_context) {
                ReadbackPoll::Pending => continue,
                ReadbackPoll::Ready(image) => break image,
                ReadbackPoll::Failed(error) => panic!("scene readback failed: {error}"),
            }
        };
        // The unlit cube is black on the white clear color, so only a resolve,
        // which averages an edge pixel's covered and uncovered samples, gives
        // a grey; a copy keeps a single sample's black or white.
        let background = image.get_pixel(0, 0)[0];
        let cube = image.get_pixel(SIZE / 2, SIZE / 2)[0];
        // White through the default ACES curve, which maps 1.0 to about 0.619.
        let white = linear_to_srgb(0.619);
        assert!(
            background.abs_diff(white) <= 1,
            "the corner should keep the tone-mapped clear color {white}, got {background}"
        );
        assert!(cube < 128, "the cube should cover the center, got {cube}");
        let edge_pixels = image
            .pixels()
            .filter(|pixel| pixel[0] > cube + 8 && pixel[0] < background - 8)
            .count();
        assert!(
            edge_pixels > 0,
            "no pixel along the cube's edges blends it with the background, so the \
             {SAMPLES} samples were not resolved"
        );
        assert_eq!(renderer.pick_game_object(SIZE / 2, SIZE / 2), Some(uuid));
    }

    /// Writes a `size` by `size` HDR image of one color to `path`.
    fn write_test_hdr(path: &Path, size: usize) {
        std::fs::create_dir_all(path.parent().expect("hdr path has a parent"))
            .expect("failed to create skybox directory");
        let file = std::fs::File::create(path).expect("failed to create hdr file");
        let pixels = vec![image::Rgb([0.2f32, 0.4, 0.8]); size * size];
        image::codecs::hdr::HdrEncoder::new(std::io::BufWriter::new(file))
            .encode(&pixels, size, size)
            .expect("failed to encode hdr file");
    }

    #[test]
    fn sky_light_skybox_loads_in_the_background_instead_of_stalling_a_frame() {
        // HDR skyboxes upload as Rgba32Float, which the editor's device filters
        // through FLOAT32_FILTERABLE.
        if !crate::test_support::test_render_context()
            .device()
            .features()
            .contains(wgpu::Features::FLOAT32_FILTERABLE)
        {
            eprintln!("skipping: the test adapter cannot filter Rgba32Float skyboxes");
            return;
        }
        let assets = TestAssets::engine();
        write_test_hdr(&assets.engine_root().join("skyboxes/test_sky.hdr"), 64);
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let skybox_id = read_only_context
            .registries
            .assets
            .read()
            .asset_id("skyboxes/test_sky")
            .expect("test skybox should have metadata");

        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let sky = builder.spawn("sky");
        builder.add_component(
            sky,
            ComponentSkyLight {
                skybox: AssetRef::from_id(skybox_id),
                ..Default::default()
            },
        );
        let scene = builder.finish();
        let (camera, camera_transform) = front_camera(1.0, 5.0);
        let mut renderer = renderer(&read_only_context, 1, (64, 64));

        renderer.render_scene_base(&camera, &camera_transform, &scene, None, None);
        assert_eq!(
            renderer.skybox_renderer.skybox_id(),
            None,
            "the first frame must render without waiting for the skybox to load"
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while renderer.skybox_renderer.skybox_id().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the skybox never finished loading"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
            renderer.render_scene_base(&camera, &camera_transform, &scene, None, None);
        }
        assert_eq!(renderer.skybox_renderer.skybox_id(), Some(skybox_id));
        let skybox = read_only_context
            .registries
            .assets
            .read()
            .loaded_by_id::<Skybox>(skybox_id)
            .expect("the loaded skybox should be cached");
        assert!(
            !skybox.read().dirty,
            "the frame that selected the skybox should have prepared its maps"
        );
    }

    /// What one frame of [`shadow_frames`] saw: the red channel at each
    /// sampled point, what the shadow pass drew, and which maps exist.
    struct ShadowFrame<const N: usize> {
        pixels: [u8; N],
        stats: ShadowFrameStats,
        allocated: (bool, bool),
    }

    /// An edit to a [`shadow_frames`] scene between two frames, given the
    /// objects its build returned.
    type ShadowEdit = Box<dyn FnOnce(&mut Scene, &[GameObject])>;

    /// The camera of the shadow tests: 8 units straight above the origin,
    /// looking down.
    fn overhead_camera() -> (Camera, Transform) {
        let camera = Camera::new(1.0, 60f32.to_radians(), 0.1, 200.0);
        let transform = Transform::from_components(
            vec3(0.0, 8.0, 0.0),
            UnitQuaternion::face_towards(&vec3(0.0, -1.0, 0.0), &vec3(0.0, 0.0, 1.0)),
            vec3(1.0, 1.0, 1.0),
        );
        (camera, transform)
    }

    /// The rotation that points a light's +Z along `direction`.
    fn pointing(direction: Vec3) -> Mat4 {
        glm::quat_to_mat4(
            &UnitQuaternion::rotation_between(&Vec3::z(), &direction.normalize())
                .expect("a direction off -Z"),
        )
    }

    /// Spawns a cube of `scale` at `position` that casts shadows.
    fn spawn_box(
        builder: &mut SceneBuilder,
        name: &str,
        cube: &Ref<Mesh>,
        material: &Ref<Material>,
        position: Vec3,
        scale: Vec3,
    ) -> GameObject {
        let object = builder.spawn(name);
        builder.add_component(
            object,
            ComponentMesh {
                mesh: AssetRef::from_id(cube.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        builder.set_transform(object, glm::translation(&position) * glm::scaling(&scale));
        object
    }

    /// Spawns a shadow-casting sun travelling along `direction`.
    fn spawn_sun(builder: &mut SceneBuilder, direction: Vec3) -> GameObject {
        let sun = builder.spawn("sun");
        builder.add_component(
            sun,
            crate::component::ComponentDirectionalLight {
                intensity: 3.0,
                cast_shadows: true,
                ..Default::default()
            },
        );
        builder.set_transform(sun, pointing(direction));
        sun
    }

    /// Renders, from [`overhead_camera`], a ground plane whose top face is at
    /// y = 0.05 spanning -4..4 in X and Z, plus whatever `build` adds, then
    /// renders again after each of `edits`. `build` returns the objects the
    /// edits change.
    fn shadow_frames<const N: usize>(
        shadows: bool,
        build: impl FnOnce(
            &mut SceneBuilder,
            &ReadOnlyAssetContext,
            &Ref<Mesh>,
            &Ref<Material>,
        ) -> Vec<GameObject>,
        points: [Vec3; N],
        edits: Vec<ShadowEdit>,
    ) -> Vec<ShadowFrame<N>> {
        const SIZE: (u32, u32) = (128, 128);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/shadows");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_box(
            &mut builder,
            "ground",
            &cube,
            &material,
            Vec3::zeros(),
            // The cube mesh spans -0.5..0.5.
            vec3(8.0, 0.1, 8.0),
        );
        let objects = build(&mut builder, &read_only_context, &cube, &material);
        let mut scene = builder.finish();
        let (camera, transform) = overhead_camera();
        let mut renderer = SceneRenderer::new(
            &read_only_context,
            SceneRendererOptions {
                clear_color: Color32::BLACK,
                shadows,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        let frame = |renderer: &mut SceneRenderer, scene: &Scene| {
            renderer.render_scene_base(&camera, &transform, scene, None, None);
            renderer.finalize_scene();
            let image = read_scene_texture(renderer, &render_context);
            ShadowFrame {
                pixels: points.map(|point| {
                    let (x, y) = project(&camera, &transform, point, SIZE);
                    image.get_pixel(x, y).0[0]
                }),
                stats: renderer.shadow_stats(),
                allocated: renderer.shadow_maps_allocated(),
            }
        };
        let mut frames = vec![frame(&mut renderer, &scene)];
        for edit in edits {
            edit(&mut scene, &objects);
            frames.push(frame(&mut renderer, &scene));
        }
        frames
    }

    /// A sun at 45 degrees along +X over a box hovering at y = 1.5: the
    /// box's shadow centers on (1.45, 0.05, 0), clear of the box as the
    /// overhead camera sees it, and (-1.5, 0.05, 0) is in full light.
    fn sun_over_box(
        builder: &mut SceneBuilder,
        _: &ReadOnlyAssetContext,
        cube: &Ref<Mesh>,
        material: &Ref<Material>,
    ) -> Vec<GameObject> {
        let sun = spawn_sun(builder, vec3(1.0, -1.0, 0.0));
        let occluder = spawn_box(
            builder,
            "box",
            cube,
            material,
            vec3(0.0, 1.5, 0.0),
            Vec3::repeat(1.0),
        );
        vec![sun, occluder]
    }

    const BOX_SHADOW: Vec3 = Vec3::new(1.45, 0.05, 0.0);
    const BOX_LIT: Vec3 = Vec3::new(-1.5, 0.05, 0.0);

    #[test]
    fn a_box_over_a_plane_darkens_the_plane_where_it_occludes_the_sun() {
        let frames = shadow_frames(true, sun_over_box, [BOX_SHADOW, BOX_LIT], vec![]);
        let [shadowed, lit] = frames[0].pixels;
        assert!(lit > 40, "the plane is lit away from the box, got {lit}");
        assert!(
            shadowed <= 2,
            "the box's shadow keeps the sun off the plane: {shadowed}, lit {lit}"
        );
        assert_eq!(frames[0].stats.cascades, SHADOW_CASCADE_COUNT);
        assert_eq!(
            frames[0].allocated,
            (true, false),
            "no spot light, no atlas"
        );

        let [unshadowed, lit_without] =
            shadow_frames(false, sun_over_box, [BOX_SHADOW, BOX_LIT], vec![])[0].pixels;
        assert!(
            unshadowed.abs_diff(lit) <= 2 && lit_without.abs_diff(lit) <= 2,
            "with shadows off the plane is lit everywhere: {unshadowed} and {lit_without}, \
             against {lit}"
        );
    }

    #[test]
    fn a_plane_facing_the_light_is_lit_evenly_with_no_acne() {
        let points: [Vec3; 25] = std::array::from_fn(|i| {
            vec3(
                -3.0 + 1.5 * (i % 5) as f32,
                0.05,
                -3.0 + 1.5 * (i / 5) as f32,
            )
        });
        let tilted = vec3(30f32.to_radians().sin(), -30f32.to_radians().cos(), 0.0);
        let build = |builder: &mut SceneBuilder,
                     _: &ReadOnlyAssetContext,
                     _: &Ref<Mesh>,
                     _: &Ref<Material>| { vec![spawn_sun(builder, tilted)] };
        let with = shadow_frames(true, build, points, vec![]);
        let without = shadow_frames(false, build, points, vec![]);
        let reference = without[0].pixels[12];
        assert!(reference > 40, "the plane is lit, got {reference}");
        for (i, (on, off)) in with[0].pixels.iter().zip(without[0].pixels).enumerate() {
            assert!(
                off > 40,
                "point {i} at {} is on the lit plane, got {off}",
                points[i]
            );
            assert!(
                on.abs_diff(off) <= 2,
                "point {i} at {} shadows itself: {on} with shadows, {off} without",
                points[i]
            );
        }
    }

    #[test]
    fn each_spot_light_shadow_lands_in_its_own_tile() {
        // Two spot lights 3 units up at x = -2 and x = 2, pointing down, each
        // with a box between it and the plane on the side towards the
        // center. A box's shadow falls 1 unit from its light towards the
        // center, and the point 1 unit away on the other side is lit.
        let build = |builder: &mut SceneBuilder,
                     _: &ReadOnlyAssetContext,
                     cube: &Ref<Mesh>,
                     material: &Ref<Material>| {
            for (i, x) in [-2.0f32, 2.0].into_iter().enumerate() {
                let spot = builder.spawn(format!("spot {i}"));
                builder.add_component(
                    spot,
                    crate::component::ComponentSpotLight {
                        intensity: 30.0,
                        range: 10.0,
                        inner_angle: 35.0,
                        outer_angle: 40.0,
                        cast_shadows: true,
                        ..Default::default()
                    },
                );
                builder.set_transform(
                    spot,
                    glm::translation(&vec3(x, 3.0, 0.0)) * pointing(-Vec3::y()),
                );
                let toward_center = -x.signum();
                spawn_box(
                    builder,
                    &format!("box {i}"),
                    cube,
                    material,
                    vec3(x + 0.4 * toward_center, 1.5, 0.0),
                    Vec3::repeat(0.5),
                );
            }
            vec![]
        };
        let points = [
            vec3(-1.0, 0.05, 0.0),
            vec3(-3.0, 0.05, 0.0),
            vec3(1.0, 0.05, 0.0),
            vec3(3.0, 0.05, 0.0),
        ];
        let frame = &shadow_frames(true, build, points, vec![])[0];
        let [left_shadow, left_lit, right_shadow, right_lit] = frame.pixels;
        assert_eq!(frame.stats.spot_tiles, 2);
        assert_eq!(frame.allocated, (false, true), "no sun, no cascades");
        for (side, shadowed, lit) in [
            ("left", left_shadow, left_lit),
            ("right", right_shadow, right_lit),
        ] {
            assert!(
                lit > 40,
                "the {side} spot light lights the plane, got {lit}"
            );
            assert!(
                shadowed <= 2,
                "the {side} box shadows its spot light: {shadowed}, lit {lit}"
            );
        }
        let unshadowed = shadow_frames(false, build, points, vec![])[0].pixels;
        assert!(
            unshadowed[0].abs_diff(left_lit) <= 2 && unshadowed[2].abs_diff(right_lit) <= 2,
            "without shadows both sides are lit: {unshadowed:?}, against {left_lit} and \
             {right_lit}"
        );
        assert!(
            unshadowed[1].abs_diff(left_lit) <= 2,
            "the lit side has no acne: {} without shadows, {left_lit} with",
            unshadowed[1]
        );
    }

    #[test]
    fn a_mesh_that_casts_no_shadow_or_is_disabled_drops_out_of_the_shadow_pass() {
        let set_cast = |cast_shadows: bool| -> ShadowEdit {
            Box::new(move |scene: &mut Scene, objects: &[GameObject]| {
                scene
                    .write_component::<ComponentMesh, _>(objects[1], |mesh| {
                        mesh.cast_shadows = cast_shadows
                    })
                    .expect("the box has a mesh");
            })
        };
        let set_enabled = |enabled: bool| -> ShadowEdit {
            Box::new(move |scene: &mut Scene, objects: &[GameObject]| {
                assert!(scene.set_component_enabled::<ComponentMesh>(objects[1], enabled));
            })
        };
        let frames = shadow_frames(
            true,
            sun_over_box,
            [BOX_SHADOW],
            vec![
                set_cast(false),
                set_cast(true),
                set_enabled(false),
                set_enabled(true),
            ],
        );
        let shadowed = |frame: &ShadowFrame<1>| frame.pixels[0] <= 2;
        // The ground and the box, in each cascade.
        let both = 2 * SHADOW_CASCADE_COUNT as u32;
        let ground_only = SHADOW_CASCADE_COUNT as u32;
        assert!(shadowed(&frames[0]));
        assert_eq!(frames[0].stats.caster_instances, both);
        assert!(
            !shadowed(&frames[1]),
            "a box that casts no shadow leaves the plane lit"
        );
        assert_eq!(frames[1].stats.caster_instances, ground_only);
        assert!(shadowed(&frames[2]), "casting again, it shadows again");
        assert_eq!(frames[2].stats.caster_instances, both);
        assert!(!shadowed(&frames[3]), "a disabled box casts no shadow");
        assert_eq!(frames[3].stats.caster_instances, ground_only);
        assert!(shadowed(&frames[4]), "re-enabled, it shadows again");
        assert_eq!(frames[4].stats.caster_instances, both);
    }

    #[test]
    fn a_disabled_or_unshadowed_sun_frees_its_cascades_until_it_casts_again() {
        let set_enabled = |enabled: bool| -> ShadowEdit {
            Box::new(move |scene: &mut Scene, objects: &[GameObject]| {
                assert!(scene
                    .set_component_enabled::<crate::component::ComponentDirectionalLight>(
                        objects[0], enabled
                    ));
            })
        };
        let set_cast = |cast_shadows: bool| -> ShadowEdit {
            Box::new(move |scene: &mut Scene, objects: &[GameObject]| {
                scene
                    .write_component::<crate::component::ComponentDirectionalLight, _>(
                        objects[0],
                        |sun| sun.cast_shadows = cast_shadows,
                    )
                    .expect("the sun has a light");
            })
        };
        let frames = shadow_frames(
            true,
            sun_over_box,
            [BOX_SHADOW],
            vec![
                set_enabled(false),
                set_enabled(true),
                set_cast(false),
                set_cast(true),
            ],
        );
        let allocated: Vec<_> = frames.iter().map(|frame| frame.allocated.0).collect();
        assert_eq!(allocated, [true, false, true, false, true]);
        let cascades: Vec<_> = frames.iter().map(|frame| frame.stats.cascades).collect();
        let all = SHADOW_CASCADE_COUNT;
        assert_eq!(cascades, [all, 0, all, 0, all]);
        assert!(
            frames[2].pixels[0] <= 2,
            "re-enabled, the sun is shadowed again"
        );

        // A renderer with shadows off gathers no caster and allocates nothing.
        let off = &shadow_frames(false, sun_over_box, [BOX_SHADOW], vec![])[0];
        assert_eq!(off.allocated, (false, false));
        assert_eq!(off.stats, ShadowFrameStats::default());
    }

    /// The box of `sun_over_box` in a PBR material that discards fragments
    /// whose alpha is below 0.5, with base color alpha `alpha`.
    fn masked_box_shadow(alpha: f32) -> u8 {
        let frames = shadow_frames(
            true,
            move |builder, context, cube, _| {
                let sun = spawn_sun(builder, vec3(1.0, -1.0, 0.0));
                let material = pbr_material(context, &format!("test/masked_{alpha}"));
                {
                    let mut material = material.write();
                    assert!(material.set_variable(
                        "base_color_factor",
                        ShaderVariableValue::Color([1.0, 1.0, 1.0, alpha])
                    ));
                    assert!(material.set_variable("alpha_cutoff", ShaderVariableValue::Float(0.5)));
                }
                spawn_box(
                    builder,
                    "box",
                    cube,
                    &material,
                    vec3(0.0, 1.5, 0.0),
                    Vec3::repeat(1.0),
                );
                vec![sun]
            },
            [BOX_SHADOW],
            vec![],
        );
        frames[0].pixels[0]
    }

    #[test]
    fn an_alpha_masked_material_casts_no_shadow_where_it_is_discarded() {
        let opaque = masked_box_shadow(1.0);
        let discarded = masked_box_shadow(0.25);
        assert!(
            opaque <= 2,
            "a kept masked surface casts its shadow, got {opaque}"
        );
        assert!(
            discarded > 40,
            "a fully discarded surface casts no shadow, got {discarded}"
        );
    }

    #[test]
    fn a_skinned_mesh_casts_its_posed_shadow() {
        // The mesh models a unit cube at the origin; its bone poses it at
        // y = 1.5, where the box of `sun_over_box` hovers. Only the
        // posed cube stands between the sun and `BOX_SHADOW`.
        let frames = shadow_frames(
            true,
            |builder, context, _, material| {
                let sun = spawn_sun(builder, vec3(1.0, -1.0, 0.0));
                let mesh = skinned_cube(context, "test/shadow_skinned", "hip", Mat4::identity());
                let hip = glm::translation(&vec3(0.0, 1.5, 0.0));
                spawn_skinned_model(builder, &mesh, material, Mat4::identity(), hip, Vec::new());
                vec![sun]
            },
            [BOX_SHADOW, BOX_LIT],
            vec![],
        );
        let [shadowed, lit] = frames[0].pixels;
        assert!(lit > 40, "the plane is lit away from the cube, got {lit}");
        assert!(
            shadowed <= 2,
            "the posed cube shadows the plane: {shadowed}, lit {lit}"
        );
    }
    /// The texel at `(x, y)` of `texture`, as its bytes.
    fn read_texel(
        render_context: &RenderContext,
        texture: &Texture,
        (x, y): (u32, u32),
    ) -> Vec<u8> {
        let device = render_context.device();
        let size = texture
            .descriptor
            .format
            .block_copy_size(None)
            .expect("a color format has a texel size");
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("texel_readback"),
            size: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        render_context.queue().submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
        let _ = device.poll(wgpu::Maintain::Wait);
        let bytes = slice.get_mapped_range()[..size as usize].to_vec();
        buffer.unmap();
        bytes
    }

    /// The four half floats of an `Rgba16Float` texel.
    fn halves(bytes: &[u8]) -> [f32; 4] {
        std::array::from_fn(|i| half_to_f32(u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]])))
    }

    /// The unit vector whose octahedral encoding is `(x, y)`, as
    /// `docs/design/rendering/gbuffer.md` defines it.
    fn octahedral_decode(x: f32, y: f32) -> Vec3 {
        let mut n = vec3(x, y, 1.0 - x.abs() - y.abs());
        let fold = (-n.z).clamp(0.0, 1.0);
        n.x += if n.x >= 0.0 { -fold } else { fold };
        n.y += if n.y >= 0.0 { -fold } else { fold };
        n.normalize()
    }

    fn assert_near(actual: Vec3, expected: Vec3, what: &str) {
        assert!(
            (actual - expected).norm() < 0.01,
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }

    fn assert_bytes_near(actual: &[u8], expected: &[u8], what: &str) {
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(*b) <= 1),
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }

    /// The material of the G-buffer tests: every input the geometry pass
    /// writes set to a distinct value.
    fn gbuffer_material(context: &ReadOnlyAssetContext, name: &str) -> Ref<Material> {
        let material = pbr_material(context, name);
        {
            let mut material = material.write();
            for (variable, value) in [
                (
                    "base_color_factor",
                    ShaderVariableValue::Color([0.8, 0.4, 0.2, 1.0]),
                ),
                ("metallic", ShaderVariableValue::Float(0.25)),
                ("roughness", ShaderVariableValue::Float(0.5)),
                ("ambient_occlusion", ShaderVariableValue::Float(0.75)),
                (
                    "emission_color",
                    ShaderVariableValue::Color([1.0, 0.5, 0.0, 1.0]),
                ),
                ("emission_strength", ShaderVariableValue::Float(2.0)),
            ] {
                assert!(material.set_variable(variable, value), "{variable}");
            }
        }
        material
    }

    /// The sRGB bytes of [`gbuffer_material`]'s base color.
    fn gbuffer_albedo_bytes() -> [u8; 3] {
        [0.8, 0.4, 0.2].map(linear_to_srgb)
    }

    /// The plane of the G-buffer tests: a thin box in front of the camera
    /// of `front_camera`, turned 30 degrees about Y, so its face towards the
    /// camera has the normal `(-sin 30, 0, -cos 30)`.
    fn spawn_turned_plane(builder: &mut SceneBuilder, mesh: &Ref<Mesh>, material: &Ref<Material>) {
        let plane = builder.spawn("plane");
        builder.add_component(
            plane,
            ComponentMesh {
                mesh: AssetRef::from_id(mesh.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        builder.set_transform(
            plane,
            glm::rotation(30f32.to_radians(), &Vec3::y()) * glm::scaling(&vec3(2.0, 2.0, 0.05)),
        );
    }

    /// The normal of [`spawn_turned_plane`]'s face towards the camera, in
    /// the view space of `front_camera`, which looks down +Z unrotated.
    fn turned_plane_normal() -> Vec3 {
        vec3(-30f32.to_radians().sin(), 0.0, -30f32.to_radians().cos())
    }

    /// Renders `scene` from `front_camera` with `view` selected and returns
    /// the renderer.
    fn render_gbuffer(
        context: &ReadOnlyAssetContext,
        scene: &Scene,
        samples: u32,
        size: (u32, u32),
        view: Option<GBufferView>,
    ) -> SceneRenderer {
        let mut renderer = SceneRenderer::new(
            context,
            SceneRendererOptions {
                samples,
                clear_color: Color32::BLACK,
                gbuffer_view: view,
                ..Default::default()
            },
            size,
        )
        .expect("scene renderer should load its assets");
        let (camera, transform) = front_camera(1.0, 5.0);
        renderer.render_scene_base(&camera, &transform, scene, None, None);
        renderer.finalize_scene();
        renderer
    }

    #[test]
    fn the_gbuffer_holds_a_planes_albedo_normals_material_and_emission() {
        const SIZE: (u32, u32) = (32, 32);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = gbuffer_material(&read_only_context, "test/gbuffer_plane");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_turned_plane(&mut builder, &cube, &material);
        let scene = builder.finish();
        let center = (SIZE.0 / 2, SIZE.1 / 2);

        for samples in [1, 4] {
            if !supports_samples(
                &render_context,
                wgpu::TextureFormat::Rgba16Float,
                samples,
                true,
            ) {
                continue;
            }
            let renderer = render_gbuffer(
                &read_only_context,
                &scene,
                samples,
                SIZE,
                Some(GBufferView::Albedo),
            );
            let gbuffer = renderer
                .gbuffer()
                .expect("a G-buffer view draws the G-buffer");
            assert_eq!(gbuffer.size(), SIZE);

            let albedo = read_texel(&render_context, &gbuffer.albedo, center);
            assert_bytes_near(&albedo[..3], &gbuffer_albedo_bytes(), "albedo");
            assert_eq!(albedo[3], 255, "a drawn surface marks its albedo alpha");
            let corner = read_texel(&render_context, &gbuffer.albedo, (0, 0));
            assert_eq!(corner, [0, 0, 0, 0], "nothing is drawn at the corner");

            let [sx, sy, gx, gy] = halves(&read_texel(&render_context, &gbuffer.normals, center));
            assert_near(
                octahedral_decode(sx, sy),
                turned_plane_normal(),
                &format!("shading normal at {samples} samples"),
            );
            assert_near(
                octahedral_decode(gx, gy),
                turned_plane_normal(),
                &format!("geometric normal at {samples} samples"),
            );

            let surface = read_texel(&render_context, &gbuffer.material, center);
            assert_bytes_near(
                &surface[..3],
                &[64, 128, 191],
                "metallic, roughness, occlusion",
            );

            let [r, g, b, _] = halves(&read_texel(&render_context, &gbuffer.emissive, center));
            assert_near(vec3(r, g, b), vec3(2.0, 1.0, 0.0), "emission");
        }
    }

    #[test]
    fn a_normal_map_bends_the_gbuffer_shading_normal_but_not_the_geometric_normal() {
        const SIZE: (u32, u32) = (32, 32);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        // A tangent-space normal along the tangent, at half scale.
        let material = pbr_material(&read_only_context, "test/gbuffer_normal_map");
        {
            let mut material = material.write();
            assert!(material.set_variable(
                "normal_texture",
                ShaderVariableValue::Texture2D(MaterialTexture::Color([1.0, 0.5, 1.0, 1.0]))
            ));
            assert!(material.set_variable("normal_scale", ShaderVariableValue::Float(0.5)));
        }
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_mesh(&mut builder, "plane", &cube, &material, Vec3::zeros());
        let scene = builder.finish();
        let renderer = render_gbuffer(
            &read_only_context,
            &scene,
            1,
            SIZE,
            Some(GBufferView::Normals),
        );
        let gbuffer = renderer.gbuffer().expect("the G-buffer is drawn");
        let [sx, sy, gx, gy] = halves(&read_texel(
            &render_context,
            &gbuffer.normals,
            (SIZE.0 / 2, SIZE.1 / 2),
        ));
        let geometric = octahedral_decode(gx, gy);
        let shading = octahedral_decode(sx, sy);
        assert_near(geometric, vec3(0.0, 0.0, -1.0), "geometric normal");
        // The texel (1, 0.5, 1) is the tangent plus the normal; at half
        // scale the normal tilts atan(0.5) towards the tangent, which lies
        // in the face's plane.
        let tilt = shading.xy().norm().atan2(-shading.z).to_degrees();
        assert!(
            (tilt - 0.5f32.atan().to_degrees()).abs() < 1.0,
            "the shading normal tilts atan(0.5) along the tangent, got {shading:?}"
        );
    }

    #[test]
    fn the_gbuffer_discards_the_fragments_an_alpha_cutoff_discards() {
        const SIZE: (u32, u32) = (32, 32);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let behind = gbuffer_material(&read_only_context, "test/gbuffer_behind");
        let masked = pbr_material(&read_only_context, "test/gbuffer_masked");
        {
            let mut masked = masked.write();
            assert!(masked.set_variable(
                "base_color_factor",
                ShaderVariableValue::Color([0.0, 1.0, 0.0, 0.25])
            ));
            assert!(masked.set_variable("alpha_cutoff", ShaderVariableValue::Float(0.5)));
        }
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_mesh(&mut builder, "behind", &cube, &behind, vec3(0.0, 0.0, 3.0));
        spawn_mesh(&mut builder, "masked", &cube, &masked, Vec3::zeros());
        let scene = builder.finish();
        let renderer = render_gbuffer(
            &read_only_context,
            &scene,
            1,
            SIZE,
            Some(GBufferView::Albedo),
        );
        let gbuffer = renderer.gbuffer().expect("the G-buffer is drawn");
        let albedo = read_texel(&render_context, &gbuffer.albedo, (SIZE.0 / 2, SIZE.1 / 2));
        assert_bytes_near(
            &albedo[..3],
            &gbuffer_albedo_bytes(),
            "the discarded box shows the box behind it",
        );
    }

    #[test]
    fn a_skinned_mesh_writes_its_posed_normal_into_the_gbuffer() {
        const SIZE: (u32, u32) = (32, 32);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let mesh = skinned_cube(
            &read_only_context,
            "test/gbuffer_skinned",
            "hip",
            Mat4::identity(),
        );
        let material = gbuffer_material(&read_only_context, "test/gbuffer_skinned_material");
        // No animator and no palette: the rest pose, in which the bone's
        // turn poses the cube.
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_skinned_model(
            &mut builder,
            &mesh,
            &material,
            Mat4::identity(),
            glm::rotation(30f32.to_radians(), &Vec3::y()),
            Vec::new(),
        );
        let scene = builder.finish();
        let renderer = render_gbuffer(
            &read_only_context,
            &scene,
            1,
            SIZE,
            Some(GBufferView::Normals),
        );
        let gbuffer = renderer.gbuffer().expect("the G-buffer is drawn");
        let [sx, sy, gx, gy] = halves(&read_texel(
            &render_context,
            &gbuffer.normals,
            (SIZE.0 / 2, SIZE.1 / 2),
        ));
        assert_near(
            octahedral_decode(sx, sy),
            turned_plane_normal(),
            "shading normal",
        );
        assert_near(
            octahedral_decode(gx, gy),
            turned_plane_normal(),
            "geometric normal",
        );
    }

    #[test]
    fn the_geometry_pass_runs_only_while_a_gbuffer_view_is_selected() {
        const SIZE: (u32, u32) = (16, 16);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = pbr_material(&read_only_context, "test/gbuffer_toggle");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_mesh(&mut builder, "box", &cube, &material, Vec3::zeros());
        let scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);
        let mut renderer = renderer(&read_only_context, 1, SIZE);
        let frame = |renderer: &mut SceneRenderer| {
            renderer.render_scene_base(&camera, &transform, &scene, None, None);
            renderer.finalize_scene();
            (
                renderer.gbuffer().is_some(),
                renderer.gbuffer_drawn_instances(),
            )
        };

        assert_eq!(
            frame(&mut renderer),
            (false, 0),
            "the forward frame draws no G-buffer"
        );
        assert!(
            !renderer.last_post_process_plan().passes.is_empty(),
            "the lit image is post-processed"
        );
        renderer.options_mut().gbuffer_view = Some(GBufferView::Normals);
        assert_eq!(
            frame(&mut renderer),
            (true, 1),
            "a G-buffer view draws the box"
        );
        renderer.resize_textures(24, 20);
        assert_eq!(
            renderer.gbuffer().map(GBufferTargets::size),
            Some((24, 20)),
            "the G-buffer resizes with the scene textures"
        );
        renderer.options_mut().gbuffer_view = None;
        assert_eq!(frame(&mut renderer), (false, 0), "the G-buffer is released");
    }

    #[test]
    fn the_scene_view_shows_each_gbuffer_target_in_place_of_the_lit_image() {
        const SIZE: (u32, u32) = (32, 32);
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let render_context = read_only_context.render_context.clone();
        let cube = read_only_context
            .registries
            .assets
            .read()
            .cube()
            .expect("missing cube mesh");
        let material = gbuffer_material(&read_only_context, "test/gbuffer_view");
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        spawn_turned_plane(&mut builder, &cube, &material);
        let scene = builder.finish();
        let normal = turned_plane_normal().map(|axis| (axis * 0.5 + 0.5) * 255.0);
        // Reinhard of the emission (2, 1, 0).
        let emission = [2.0 / 3.0, 0.5, 0.0].map(linear_to_srgb);
        for (view, expected) in [
            (GBufferView::Albedo, gbuffer_albedo_bytes()),
            (
                GBufferView::Normals,
                [normal.x, normal.y, normal.z].map(|value| value.round() as u8),
            ),
            (GBufferView::Material, [64, 128, 191]),
            (GBufferView::Emissive, emission),
        ] {
            let renderer = render_gbuffer(&read_only_context, &scene, 1, SIZE, Some(view));
            assert!(
                renderer.last_post_process_plan().passes.is_empty(),
                "{view:?}: the view replaces the post-processing chain"
            );
            let image = read_scene_texture(&renderer, &render_context);
            assert_bytes_near(
                &image.get_pixel(SIZE.0 / 2, SIZE.1 / 2).0[..3],
                &expected,
                view.label(),
            );
            assert_eq!(
                image.get_pixel(0, 0).0,
                [0, 0, 0, 255],
                "{view:?}: where nothing is drawn the view is black"
            );
        }
    }
}

#[cfg(test)]
mod skinning_tests {
    use super::{skinned_mesh_world_transform, skinning_instance_index};
    use crate::component::ComponentSkinnedMesh;
    use crate::scene::GameObjectRef;
    use crate::test_support::test_scene;
    use nalgebra_glm as glm;

    #[test]
    fn skinned_mesh_is_placed_by_its_root_bone_not_its_own_node() {
        // A model root, a node converting axes and units, and the skinned mesh below it, as
        // imported FBX and glTF models are laid out.
        let mut scene = test_scene();
        let root = scene.create(None, None);
        scene.set_transform(root, &glm::translation(&glm::vec3(5.0, 0.0, 0.0)));
        let converter = scene.create(None, Some(root));
        scene.set_transform(
            converter,
            &(glm::rotation(-90f32.to_radians(), &glm::Vec3::x())
                * glm::scaling(&glm::Vec3::repeat(100.0))),
        );
        let mesh_object = scene.create(None, Some(converter));
        let skinned_mesh = ComponentSkinnedMesh {
            root_bone: GameObjectRef::new(scene.uuid(root)),
            ..Default::default()
        };

        let placed = skinned_mesh_world_transform(&scene, mesh_object, &skinned_mesh).matrix();

        assert_eq!(placed, scene.world_transform(root).matrix());
        assert_ne!(placed, scene.world_transform(mesh_object).matrix());
    }

    #[test]
    fn skinned_mesh_without_a_root_bone_keeps_its_own_transform() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        scene.set_transform(parent, &glm::scaling(&glm::Vec3::repeat(2.0)));
        let mesh_object = scene.create(None, Some(parent));

        let placed =
            skinned_mesh_world_transform(&scene, mesh_object, &ComponentSkinnedMesh::default());

        assert_eq!(placed.matrix(), scene.world_transform(mesh_object).matrix());
    }

    #[test]
    fn mesh_without_bones_has_no_instance_index() {
        assert_eq!(skinning_instance_index(0, 0), None);
        assert_eq!(skinning_instance_index(8, 0), None);
    }

    #[test]
    fn instance_index_counts_uploaded_bone_sets() {
        assert_eq!(skinning_instance_index(0, 4), Some(0));
        assert_eq!(skinning_instance_index(8, 4), Some(2));
    }
}

#[cfg(test)]
mod readback_tests {
    use super::{half_to_f32, linear_to_srgb};

    #[test]
    fn half_floats_decode() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0xbc00), -1.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert_eq!(half_to_f32(0x4248), 3.140625);
        // Smallest positive subnormal half.
        assert_eq!(half_to_f32(0x0001), 5.960_464_5e-8);
        assert!(half_to_f32(0x7c00).is_infinite());
        assert!(half_to_f32(0x7e00).is_nan());
    }

    #[test]
    fn linear_values_encode_to_srgb_bytes() {
        assert_eq!(linear_to_srgb(0.0), 0);
        assert_eq!(linear_to_srgb(1.0), 255);
        assert_eq!(linear_to_srgb(2.5), 255);
        assert_eq!(linear_to_srgb(-1.0), 0);
        // Mid grey: linear 0.2158 is approximately sRGB 128.
        assert_eq!(linear_to_srgb(0.2158), 128);
    }
}
