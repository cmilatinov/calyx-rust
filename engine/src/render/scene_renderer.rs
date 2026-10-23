use super::RenderContext;
use crate::assets::error::AssetError;
use crate::assets::material::Material;
use crate::assets::mesh::{Instance, Mesh};
use crate::assets::texture::Texture;
use crate::assets::{Asset, AssetId, AssetRegistry};
use crate::component::{ComponentMesh, ComponentSkinnedMesh, ComponentSkyLight};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::Transform;
use crate::render::asset_render_state::AssetRenderState;
use crate::render::render_utils::RenderUtils;
use crate::render::{
    Camera, GizmoRenderer, GridRenderer, LightManager, MeshRenderDefaults, MeshRenderTargets,
    MeshRenderer, OutlineRenderer, ParticleRenderer, PipelineOptions, SkyboxRenderer,
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
struct CameraUniform {
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
}

impl Default for SceneRendererOptions {
    fn default() -> Self {
        Self {
            grid: false,
            gizmos: false,
            clear_color: Color32::TRANSPARENT,
            mesh_cull_mode: Some(wgpu::Face::Back),
            samples: 0,
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
    screen_space_quad: Ref<Mesh>,
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

        // Default assets
        let cube = SceneRendererError::load::<Mesh>(context, "meshes/cube")?;
        let white_texture = SceneRendererError::load::<Texture>(context, "textures/white")?;
        let asset_registry = context.registries.assets.read();
        let screen_space_quad = SceneRendererError::require(
            asset_registry.screen_space_quad(),
            AssetRegistry::SCREEN_SPACE_QUAD,
        )?;
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
                screen_space_quad,
                black_texture_2d,
                black_texture_cube,
                white_texture,
            },
            options,
            scene_texture_msaa,
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
        })
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
    pub fn render_scene_base(
        &mut self,
        camera: &Camera,
        camera_transform: &Transform,
        scene: &Scene,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        let render_context = self.asset_context.render_context.clone();
        let queue = render_context.queue();
        let device = render_context.device();

        self.load_camera_uniforms(queue, camera, camera_transform);

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
                None,
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
        if self.options.grid {
            self.grid_renderer.render(
                &render_context,
                &mut encoder,
                &self.default_assets.screen_space_quad,
                &self.scene_texture_msaa,
                &self.scene_depth_texture,
                self.options.samples,
            );
        }
        self.particle_renderer.render(
            &render_context,
            &mut encoder,
            &self.asset_context,
            scene,
            &camera_transform.position,
            &self.camera_uniform_buffer,
            &self.scene_texture_msaa,
            &self.scene_depth_texture,
            self.options.samples,
        );
        if self.options.gizmos {
            let mut render_pass = RenderUtils::overlay_pass(
                &mut encoder,
                &self.scene_texture_msaa.view,
                Some(&self.scene_depth_texture.view),
                "Scene Gizmos",
            );
            self.gizmo_renderer
                .render_gizmos(self.scene_texture_msaa.descriptor.format, &mut render_pass);
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Applies the current outline state and resolves the scene texture for presentation.
    ///
    /// Call it once after each [`Self::render_scene_base`]: above one sample the
    /// resolve discards the multisampled surface.
    pub fn finalize_scene(&mut self) {
        let render_context = self.asset_context.render_context.clone();
        let device = render_context.device();
        let queue = render_context.queue();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene_outline_encoder"),
        });
        self.render_outline_to_scene(&render_context, &mut encoder);
        Self::resolve_scene_texture(&self.scene_texture_msaa, &self.scene_texture, &mut encoder);
        queue.submit(Some(encoder.finish()));
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

    fn render_outline_to_scene(
        &mut self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        self.outline_renderer
            .set_selected_object_id(self.object_id_for_game_object(self.selected_game_object));
        self.outline_renderer
            .set_hovered_object_id(self.object_id_for_game_object(self.hovered_game_object));
        self.outline_renderer.render(
            render_context,
            encoder,
            &self.default_assets.screen_space_quad,
            &self.scene_texture_msaa,
            &self.scene_object_id_texture,
            self.options.samples,
        );
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
                let Some(index) =
                    skinning_instance_index(mesh.bone_transforms.len(), mesh.bones.len())
                else {
                    crate::logging::warn_once(format!("boneless-skinned-mesh:{mesh_id}"), || {
                        format!("Skinned mesh {mesh_id} has no bones; skipping skinning upload")
                    });
                    continue;
                };
                skinned_meshes.insert(mesh_id);
                bone_transform_index = index;
                mesh.bone_transforms
                    .extend(c_skinned_mesh.bone_transforms.iter().copied());
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
        for c_sky_light in query.iter(world).filter(|s| s.active) {
            let Some(skybox_ref) = c_sky_light.skybox.get_ref(&self.asset_context.registries)
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
            self.assets
                .meshes
                .entry(self.default_assets.screen_space_quad.id())
                .or_insert(self.default_assets.screen_space_quad.clone());
            skybox = Some(skybox_id);
            sky_light_intensity = c_sky_light.intensity.max(0.0);
        }
        self.skybox_renderer.set_skybox(skybox);
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
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

    /// Resolves the scene color into the single-sample `scene_texture`.
    ///
    /// A multisampled texture cannot be the source of a texture copy, so above
    /// one sample the resolve runs as a render pass whose resolve target is the
    /// scene texture. At one sample the two textures match and a copy suffices.
    fn resolve_scene_texture(
        scene_texture_msaa: &Texture,
        scene_texture: &Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if scene_texture_msaa.descriptor.sample_count > 1 {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Scene Resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &scene_texture_msaa.view,
                    resolve_target: Some(&scene_texture.view),
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
            return;
        }
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &scene_texture_msaa.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            wgpu::TexelCopyTextureInfo {
                texture: &scene_texture.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            scene_texture.descriptor.size,
        );
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

fn half_to_f32(half: u16) -> f32 {
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

fn linear_to_srgb(value: f32) -> u8 {
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
    use crate::assets::AssetRef;
    use crate::component::{
        ComponentID, ComponentMesh, ComponentParticleSystem, ParticleScalarRange,
        ParticleSizeCurve, ParticleSpawnShape,
    };
    use crate::math::Transform;
    use crate::render::{Camera, Shader};
    use crate::test_support::{
        test_adapter, test_asset_context_with_assets, SceneBuilder, TestAssets,
    };
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec3, vec4, Vec3};

    const VIEWPORT: u32 = 512;

    /// Creates a PBR material registered as `name`.
    fn pbr_material(context: &ReadOnlyAssetContext, name: &str) -> Ref<Material> {
        let registry = context.registries.assets.read();
        let shader = registry
            .load::<Shader>("shaders/pbr")
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

    /// Spawns an emitter at `position` holding one particle of `color` and
    /// `size`, emitted on the spot.
    fn spawn_particle(
        builder: &mut SceneBuilder,
        name: &str,
        position: Vec3,
        color: Color32,
        size: f32,
    ) -> GameObject {
        let game_object = builder.spawn(name);
        let mut system = ComponentParticleSystem::default();
        system.spawn_rate = 0.0;
        system.burst_count = 1;
        system.max_particles = 1;
        system.lifetime = ParticleScalarRange {
            min: 10.0,
            max: 10.0,
        };
        system.spawn_shape = ParticleSpawnShape::Sphere { radius: 0.0 };
        system.initial_velocity = Vec3::zeros();
        system.velocity_randomness = Vec3::zeros();
        system.acceleration = Vec3::zeros();
        system.size = ParticleSizeCurve {
            start: size,
            end: size,
            randomness: 0.0,
        };
        system.start_color = color;
        system.end_color = color;
        builder.add_component(game_object, system);
        builder.set_transform(game_object, glm::translation(&position));
        let scene = builder.scene_mut();
        let emitter_transform = scene.world_transform(game_object);
        let seed = scene.uuid(game_object);
        scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
            system.reset_runtime(seed);
            system.step(0.0, &emitter_transform);
        });
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
        renderer.render_scene_base(&camera, &transform, &scene, None);
        renderer.finalize_scene();
        let image = read_scene_texture(&renderer, &render_context);

        let (x, y) = project(&camera, &transform, left, SIZE);
        let pixel = image.get_pixel(x, y);
        assert!(
            pixel[0] > 200 && pixel[1] < 50,
            "the red system should draw its particle on the left, got {pixel:?}"
        );
        let (x, y) = project(&camera, &transform, right, SIZE);
        let pixel = image.get_pixel(x, y);
        assert!(
            pixel[1] > 200 && pixel[0] < 50,
            "the green system should draw its particle on the right, got {pixel:?}"
        );
    }

    #[test]
    fn particle_systems_not_drawn_in_a_frame_are_dropped() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let mut builder = SceneBuilder::new(read_only_context.registries.clone());
        let kept = spawn_particle(&mut builder, "kept", Vec3::zeros(), Color32::RED, 1.0);
        let removed = spawn_particle(&mut builder, "removed", Vec3::zeros(), Color32::RED, 1.0);
        let mut scene = builder.finish();
        let (camera, transform) = front_camera(1.0, 5.0);

        let mut renderer = renderer(&read_only_context, 1, (32, 32));
        renderer.render_scene_base(&camera, &transform, &scene, None);
        assert_eq!(
            renderer.particle_renderer.tracked_systems(),
            HashSet::from([kept, removed])
        );

        scene.delete(removed);
        scene.flush_deletes();
        renderer.render_scene_base(&camera, &transform, &scene, None);
        assert_eq!(
            renderer.particle_renderer.tracked_systems(),
            HashSet::from([kept])
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
                assert_eq!(name, "shaders/pbr");
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

        let load_error = AssetError::NotFound.with_source("asset name `shaders/pbr`");
        let failed = SceneRendererError::AssetLoad {
            name: "shaders/pbr".into(),
            error: load_error.clone(),
        };
        assert_eq!(
            failed.to_string(),
            "scene renderer asset `shaders/pbr` failed to load"
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
        renderer.render_scene_base(&camera, &transform, &scene, None);
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
        renderer.render_scene_base(&camera, &transform, &scene, None);
        renderer.finalize_scene();
        assert_eq!(renderer.pick_game_object(16, 16), Some(uuid));
        // A readback still in flight when the targets change is dropped.
        renderer.request_pick_game_object(16, 16);

        renderer.resize_textures(96, 48);
        assert_eq!(renderer.scene_texture_size(), (96, 48));
        let (camera, transform) = front_camera(2.0, 5.0);
        renderer.render_scene_base(&camera, &transform, &scene, None);
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
        renderer.render_scene_base(&camera, &transform, &sphere_scene, None);
        assert!(renderer.assets.contains(sphere.id()));
        assert!(renderer.assets.contains(first.id()));

        renderer.render_scene_base(&camera, &transform, &cube_scene, None);
        assert!(!renderer.assets.contains(sphere.id()));
        assert!(!renderer.assets.contains(first.id()));
        assert!(renderer.assets.contains(cube.id()));
        assert!(renderer.assets.contains(second.id()));
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
            renderer.render_scene_base(&camera, &transform, &scene, None);
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
        renderer.render_scene_base(&camera, &transform, &scene, None);
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
        renderer.render_scene_base(&camera, &transform, &scene, None);
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
        renderer.render_scene_base(&camera, &camera_transform, &scene, None);

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
        renderer.render_scene_base(&camera, &camera_transform, &scene, None);
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
        renderer.render_scene_base(&camera, &camera_transform, &scene, None);
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
        assert_eq!(background, 255, "the corner should keep the clear color");
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
