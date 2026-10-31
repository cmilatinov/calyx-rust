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
    PostProcessChain, PostProcessSettings, SsaoPass, TonemapPass,
};
use crate::render::render_utils::RenderUtils;
use crate::render::{
    Camera, GizmoRenderer, GridRenderer, LightManager, MeshRenderDefaults, MeshRenderTargets,
    MeshRenderer, OutlineRenderer, ParticleRenderer, ParticleSimulation, PipelineOptions,
    SkyboxRenderer,
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
use std::ops::Range;
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

/// World transform that places a skinned mesh. The animator skins its vertices into the space of
/// its root bone, so the root bone's world transform places them; the mesh object's own world
/// transform would apply every node between the root bone and the mesh a second time. A mesh
/// whose root bone is not in the scene keeps its own transform.
fn skinned_mesh_world_transform(
    scene: &Scene,
    game_object: GameObject,
    skinned_mesh: &ComponentSkinnedMesh,
) -> Transform {
    let placement = skinned_mesh
        .root_bone
        .game_object(scene)
        .unwrap_or(game_object);
    scene.world_transform(placement)
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
        self.light_manager.build_data(&render_context, scene);
        // Compute the skybox's environment maps before the meshes that are lit
        // by them and the skybox pass that draws its cubemap.
        self.skybox_renderer
            .prepare(&render_context, &mut encoder, &self.assets);
        let assets = self.assets.lock();
        {
            let black_texture_cube = self.default_assets.black_texture_cube.read();
            let black_texture_2d = self.default_assets.black_texture_2d.read();
            self.mesh_renderer.render(
                device,
                &mut encoder,
                &self.asset_context,
                &assets,
                MeshRenderDefaults {
                    material_texture: self.default_assets.white_texture.clone(),
                    black_texture_2d: &black_texture_2d,
                    black_texture_cube: &black_texture_cube,
                },
                MeshRenderTargets {
                    color: &self.scene_texture_msaa,
                    depth: &self.scene_depth_texture,
                },
                queue,
                &self.light_manager,
                &self.camera_uniform_buffer,
                self.options.clear_color,
                &options,
                self.skybox_renderer.skybox_id(),
                self.sky_light_intensity,
                self.light_manager.ambient_light(),
                &draw_list,
            );
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
    pub fn finalize_scene(&mut self) {
        let render_context = self.asset_context.render_context.clone();
        let device = render_context.device();
        let queue = render_context.queue();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene_finalize_encoder"),
        });
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

    fn build_draw_list(&mut self) -> Vec<(AssetId, AssetId, AssetId, Range<u32>)> {
        let mut last: (AssetId, AssetId, AssetId) = Default::default();
        let mut mesh_instances: HashMap<AssetId, u32> = Default::default();
        let mut instance_count: u32 = 0;
        let mut mesh = None;
        let mut draw_list: Vec<(AssetId, AssetId, AssetId, Range<u32>)> = Default::default();
        let mut insert_instance_list = |last: (AssetId, AssetId, AssetId), instance_count: u32| {
            if last != Default::default() && instance_count > 0 {
                let (shader_id, mat_id, mesh_id) = last;
                let start = mesh_instances.entry(mesh_id).or_default();
                draw_list.push((
                    shader_id,
                    mat_id,
                    mesh_id,
                    *start..(*start + instance_count),
                ));
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
        } in self.draw_list.drain(0..)
        {
            if (shader_id, mat_id, mesh_id) != last {
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
            last = (shader_id, mat_id, mesh_id);
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
            if !scene.is_visible_in_hierarchy(game_object) {
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
            if !scene.is_visible_in_hierarchy(game_object) {
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
            );
        }
        let mut query = <&ComponentSkyLight>::query();
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
            .filter(|s| s.active && view_environment.is_none());
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
                 ..
             }| (*shader_id, *mat_id, *mesh_id),
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
                // Post-processing passes that read depth bind it.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
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
        let mut camera_uniform = CameraUniform::default();
        let mut projection = camera.projection;
        let mut view = camera_transform.inverse_matrix();
        camera_uniform
            .projection
            .clone_from_slice(projection.as_mut());
        camera_uniform.view.clone_from_slice(view.as_mut());
        camera_uniform
            .inverse_projection
            .clone_from_slice(glm::inverse(&projection).as_mut());
        camera_uniform
            .inverse_view
            .clone_from_slice(glm::inverse(&view).as_mut());
        camera_uniform.near_plane = camera.near_plane;
        camera_uniform.far_plane = camera.far_plane;
        camera_uniform.viewport_size = [
            self.scene_texture_msaa.descriptor.size.width as f32,
            self.scene_texture_msaa.descriptor.size.height as f32,
        ];
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
        ComponentGlobalPostProcessVolume, ComponentID, ComponentMesh, ComponentParticleSystem,
        ParticleColorOverLifetime, ParticleEmission, ParticleForces, ParticleScalarRange,
        ParticleSizeOverLifetime, ParticleSpawnShape, ParticleVelocity,
    };
    use crate::math::Transform;
    use crate::render::postprocess::TonemapSettings;
    use crate::render::{Camera, Shader};
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
            .map(|(_, material, mesh, range)| (*material, *mesh, range.len()))
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
                .filter(|(_, _, mesh_id, _)| *mesh_id == mesh.id())
                .map(|(_, _, _, range)| range.clone())
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
        builder.add_component(
            volume,
            ComponentGlobalPostProcessVolume {
                priority: 0,
                settings: PostProcessSettings {
                    tonemapping: TonemapSettings {
                        exposure_ev: 3.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
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
            .map(|(_, mat_id, mesh_id, range)| (*mat_id, *mesh_id, range.clone()))
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
