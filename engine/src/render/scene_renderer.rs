use super::RenderContext;
use crate::assets::material::Material;
use crate::assets::mesh::{Instance, Mesh};
use crate::assets::texture::Texture;
use crate::assets::AssetId;
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
use crate::scene::Scene;
use egui::Color32;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::{wgpu, RenderState};
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
#[derive(Default)]
pub struct SceneRendererOptions {
    /// Whether to draw the editor grid.
    pub grid: bool,
    /// Whether to draw debug gizmos.
    pub gizmos: bool,
    /// Clear color used for the scene color target.
    pub clear_color: Color32,
    // TODO: figure out why GTX 970 isn't supporting MSAA
    /// MSAA sample count used for offscreen scene rendering.
    pub samples: u32,
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

impl SceneRenderer {
    /// Creates a scene renderer for `context` with `options`.
    pub fn new(
        context: &ReadOnlyAssetContext,
        mut options: SceneRendererOptions,
        initial_size: (u32, u32),
    ) -> Self {
        let render_state = context.render_context.render_state();
        let asset_registry = context.registries.assets.read();
        let device = &render_state.device;
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

        let mesh_renderer = MeshRenderer::new(context);
        let skybox_renderer = SkyboxRenderer::new(context);

        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_uniform_buffer"),
            contents: bytemuck::cast_slice(&[CameraUniform::default()]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let grid_renderer = GridRenderer::new(context, device, &camera_uniform_buffer);

        let gizmo_renderer = GizmoRenderer::new(context, &camera_uniform_buffer, options.samples);
        let outline_renderer = OutlineRenderer::new(context, device);
        let particle_renderer = ParticleRenderer::new(context);

        // Default assets
        let cube = asset_registry.cube().unwrap();
        let screen_space_quad = asset_registry.screen_space_quad().unwrap();
        let black_texture_2d = asset_registry.black_texture_2d().unwrap();
        let black_texture_cube = asset_registry.black_texture_cube().unwrap();
        let white_texture = asset_registry.white_texture().unwrap();

        Self {
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
        }
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

    /// Renders the main scene content into the internal MSAA scene textures.
    pub fn render_scene_base(
        &mut self,
        render_state: &RenderState,
        camera: &Camera,
        camera_transform: &Transform,
        scene: &Scene,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        let queue = &render_state.queue;
        let device = &render_state.device;

        self.load_camera_uniforms(queue, camera, camera_transform);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("encoder"),
        });
        self.poll_object_id_readback();
        let options = PipelineOptions::builder()
            .samples(self.options.samples)
            .fragment_targets(vec![Some(wgpu::ColorTargetState {
                format: self.scene_texture_msaa.descriptor.format,
                blend: None,
                write_mask: Default::default(),
            })])
            .build();
        self.build_asset_data(render_state, scene, &options);
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
        self.build_mesh_data(render_state);
        self.light_manager.build_data(render_state, scene);
        let assets = self.assets.lock(device);
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
            render_state,
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
                render_state,
                &mut encoder,
                &self.default_assets.screen_space_quad,
                &self.scene_texture_msaa,
                &self.scene_depth_texture,
                self.options.samples,
            );
        }
        self.particle_renderer.render(
            render_state,
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
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Scene Gizmos"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scene_texture_msaa.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.scene_depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.gizmo_renderer
                .render_gizmos(self.scene_texture_msaa.descriptor.format, &mut render_pass);
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Applies the current outline state and resolves the scene texture for presentation.
    pub fn finalize_scene(&mut self, render_state: &RenderState) {
        let device = &render_state.device;
        let queue = &render_state.queue;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene_outline_encoder"),
        });
        self.render_outline_to_scene(render_state, &mut encoder);
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
            if last != Default::default() {
                let entry = mesh_instances.entry(last.2).or_default();
                let start = *entry;
                draw_list.push((last.0, last.1, last.2, start..(start + instance_count)));
                *entry += instance_count;
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
        render_state: &RenderState,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        self.outline_renderer
            .set_selected_object_id(self.object_id_for_game_object(self.selected_game_object));
        self.outline_renderer
            .set_hovered_object_id(self.object_id_for_game_object(self.hovered_game_object));
        self.outline_renderer.render(
            render_state,
            encoder,
            &self.default_assets.screen_space_quad,
            &self.scene_texture_msaa,
            &self.scene_object_id_texture,
            self.options.samples,
        );
    }

    fn build_asset_data(
        &mut self,
        render_state: &RenderState,
        scene: &Scene,
        render_options: &PipelineOptions,
    ) {
        let world = &scene.world;
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
            let transform = scene.world_transform(game_object);
            let bone_transform_index;
            {
                let mut mesh = mesh_ref.write();
                if !skinned_meshes.contains(&mesh_id) {
                    mesh.bone_transforms.clear();
                }
                skinned_meshes.insert(mesh_id);
                bone_transform_index = mesh.bone_transforms.len() / mesh.bones.len();
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
        for (_, mut mesh) in self.assets.meshes.lock_write() {
            mesh.instances.clear();
        }
        for (_, mut shader) in self.assets.shaders.lock_write() {
            shader.build_pipeline(render_options);
        }
        for (_, mut material) in self.assets.materials.lock_write() {
            material.load_buffers(render_state);
            let Self {
                asset_context,
                assets: AssetRenderState { textures, .. },
                ..
            } = self;
            material.collect_textures(
                asset_context,
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

    fn build_mesh_data(&mut self, render_state: &RenderState) {
        for (_, mut mesh) in self.assets.meshes.lock_write() {
            RenderUtils::rebuild_mesh_data(&render_state.device, &render_state.queue, &mut mesh);
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

    fn resolve_scene_texture(
        scene_texture_msaa: &Texture,
        scene_texture: &Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) {
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
