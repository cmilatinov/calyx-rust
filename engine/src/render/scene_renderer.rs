use super::{LockedAssetRenderState, RenderContext};
use crate::assets::material::{Material, MaterialBindGroupCacheKey};
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
    Camera, GizmoRenderer, GridRenderer, LightManager, PipelineOptions, Shader, SkyboxRenderer,
};
use crate::scene::Scene;
use egui::Color32;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::{wgpu, RenderState};
use legion::{Entity, IntoQuery};
use log::warn;
use nalgebra_glm as glm;
use nalgebra_glm::Mat4;
use rapier3d::pipeline::DebugRenderPipeline;
use std::collections::{HashMap, HashSet};
use std::default::Default;
use std::ops::Deref;
use std::ops::Range;
use std::sync::Arc;
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
    _padding: [f32; 2],
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
            _padding: [0.0; 2],
        }
    }
}

#[derive(Default)]
pub struct SceneRendererOptions {
    pub grid: bool,
    pub gizmos: bool,
    pub clear_color: Color32,
    // TODO: figure out why GTX 970 isn't supporting MSAA
    pub samples: u32,
}

pub struct DrawListElement {
    shader_id: AssetId,
    mat_id: AssetId,
    mesh_id: AssetId,
    bone_transform_index: i32,
    transform: [[f32; 4]; 4],
}

struct SceneRendererAssets {
    cube: Ref<Mesh>,
    screen_space_quad: Ref<Mesh>,
    black_texture_2d: Ref<Texture>,
    black_texture_cube: Ref<Texture>,
    missing_texture: Ref<Texture>,
}

pub struct SceneRenderer {
    asset_context: ReadOnlyAssetContext,
    default_assets: SceneRendererAssets,
    options: SceneRendererOptions,
    scene_texture: Texture,
    scene_depth_texture: Texture,
    scene_texture_msaa: Texture,
    scene_shader: Ref<Shader>,
    grid_renderer: GridRenderer,
    skybox_renderer: SkyboxRenderer,
    camera_uniform_buffer: wgpu::Buffer,
    light_manager: LightManager,
    gizmo_renderer: GizmoRenderer,
    assets: AssetRenderState,
    draw_list: Vec<DrawListElement>,
    material_bind_group_cache: HashMap<AssetId, CachedMaterialBindGroups>,
}

struct CachedMaterialBindGroups {
    key: MaterialBindGroupCacheKey,
    groups: HashMap<u32, wgpu::BindGroup>,
}

impl SceneRenderer {
    pub fn new(
        context: &ReadOnlyAssetContext,
        mut options: SceneRendererOptions,
        initial_size: (u32, u32),
    ) -> Self {
        let render_state = context.render_context.render_state();
        let asset_registry = context.registries.assets.read();
        let device = &render_state.device;
        let (width, height) = if initial_size.0 == 0 || initial_size.1 == 0 {
            warn!("SceneRenderer created before a valid render size was available; using 1x1 initial textures");
            (1, 1)
        } else {
            initial_size
        };
        options.samples = options.samples.max(1);

        // Textures
        let (scene_texture, scene_texture_msaa, scene_depth_texture) = Self::create_textures(
            context.render_context.clone(),
            width,
            height,
            options.samples,
        );

        // Shaders
        let scene_shader = asset_registry
            .load::<Shader>("shaders/pbr")
            .expect("missing scene_shader");
        let skybox_renderer = SkyboxRenderer::new(context);

        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_uniform_buffer"),
            contents: bytemuck::cast_slice(&[CameraUniform::default()]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let grid_renderer = GridRenderer::new(context, device, &camera_uniform_buffer);

        let gizmo_renderer = GizmoRenderer::new(context, &camera_uniform_buffer, options.samples);

        // Default assets
        let cube = asset_registry.cube().unwrap();
        let screen_space_quad = asset_registry.screen_space_quad().unwrap();
        let black_texture_2d = asset_registry.black_texture_2d().unwrap();
        let black_texture_cube = asset_registry.black_texture_cube().unwrap();
        let missing_texture = asset_registry.missing_texture().unwrap();

        Self {
            asset_context: context.clone(),
            default_assets: SceneRendererAssets {
                cube,
                screen_space_quad,
                black_texture_2d,
                black_texture_cube,
                missing_texture,
            },
            options,
            scene_texture_msaa,
            scene_texture,
            scene_depth_texture,
            scene_shader,
            grid_renderer,
            skybox_renderer,
            camera_uniform_buffer,
            light_manager: Default::default(),
            gizmo_renderer,
            assets: Default::default(),
            draw_list: Default::default(),
            material_bind_group_cache: Default::default(),
        }
    }

    pub fn options(&self) -> &SceneRendererOptions {
        &self.options
    }

    pub fn options_mut(&mut self) -> &mut SceneRendererOptions {
        &mut self.options
    }

    pub fn render_scene(
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
        if self.options.gizmos {
            self.gizmo_renderer.draw_gizmos(
                device,
                queue,
                camera_transform,
                scene,
                physics_debug_pipeline,
            );
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("encoder"),
        });
        self.render_meshes(render_state, scene, &mut encoder);
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

        // Resolve MSAA texture
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.scene_texture_msaa.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            wgpu::TexelCopyTextureInfo {
                texture: &self.scene_texture.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            self.scene_texture.descriptor.size,
        );

        queue.submit(Some(encoder.finish()));
    }

    fn scene_bind_group(
        &self,
        device: &wgpu::Device,
        irradiance_map: &Texture,
        prefilter_map: &Texture,
        brdf_map: &Texture,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bind_group"),
            layout: &self.scene_shader.read().bind_group_layouts[0],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&irradiance_map.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&irradiance_map.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&prefilter_map.view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&prefilter_map.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&brdf_map.view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&brdf_map.sampler),
                },
            ],
        })
    }

    fn render_meshes(
        &mut self,
        render_state: &RenderState,
        scene: &Scene,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let device = &render_state.device;
        let options = PipelineOptions::builder()
            .samples(self.options.samples)
            .fragment_targets(vec![Some(wgpu::ColorTargetState {
                format: self.scene_texture_msaa.descriptor.format,
                blend: None,
                write_mask: Default::default(),
            })])
            .build();
        self.build_asset_data(render_state, scene, &options);
        let draw_list = self.build_draw_list();
        self.build_mesh_data(render_state);
        self.light_manager.build_data(render_state, scene);
        let assets = self.assets.lock(device);
        let material_bind_groups = Self::build_material_bind_groups(
            device,
            &self.asset_context,
            self.default_assets.missing_texture.clone(),
            &mut self.material_bind_group_cache,
            &assets,
        );
        let black_texture_cube = self.default_assets.black_texture_cube.read();
        let black_texture_2d = self.default_assets.black_texture_2d.read();
        let (irradiance_map, prefilter_map, brdf_map) = self
            .skybox_renderer
            .skybox_id()
            .and_then(|id| {
                let skybox = assets.skybox(id)?;
                Some((
                    &skybox.irradiance_cubemap,
                    &skybox.prefilter_cubemap,
                    &skybox.brdf_map,
                ))
            })
            .unwrap_or((
                black_texture_cube.deref(),
                black_texture_cube.deref(),
                black_texture_2d.deref(),
            ));
        let scene_bind_group =
            self.scene_bind_group(device, irradiance_map, prefilter_map, brdf_map);
        let light_storage_bind_group = {
            let scene_shader = self.scene_shader.read();
            self.light_manager.storage_bind_group(device, &scene_shader)
        };
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Viewport Scene"),
                color_attachments: &[Some(RenderUtils::color_attachment(
                    &self.scene_texture_msaa.view,
                    self.options.clear_color,
                ))],
                depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
                    &self.scene_depth_texture.view,
                    1.0,
                    Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                )),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut last: (AssetId, AssetId, AssetId) = Default::default();
            for (shader_id, mat_id, mesh_id, instances) in draw_list {
                let Some(shader) = assets.shader(shader_id) else {
                    continue;
                };
                let Some(mesh) = assets.mesh(mesh_id) else {
                    continue;
                };
                if shader_id != last.0 {
                    if let Some(pipeline) = shader.get_pipeline(&options) {
                        render_pass.set_pipeline(pipeline);
                        render_pass.set_bind_group(0, &scene_bind_group, &[]);
                        render_pass.set_bind_group(2, &light_storage_bind_group, &[]);
                    }
                }
                if mat_id != last.1 {
                    if let Some(groups) = material_bind_groups.get(&mat_id) {
                        for (index, group) in groups {
                            render_pass.set_bind_group(*index, group, &[]);
                        }
                    }
                }
                if mesh_id != last.2 {
                    let Some(mesh_instance_group) = assets.mesh_instance_group(mesh_id) else {
                        continue;
                    };
                    render_pass.set_bind_group(1, mesh_instance_group, &[]);
                }
                RenderUtils::bind_mesh_buffers(&mut render_pass, mesh);
                RenderUtils::draw_mesh_instanced(&mut render_pass, mesh, instances);
                last = (shader_id, mat_id, mesh_id);
            }

            // Render gizmos
            if self.options.gizmos {
                self.gizmo_renderer
                    .render_gizmos(self.scene_texture_msaa.descriptor.format, &mut render_pass);
            }
        }
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
        self.draw_list.push(DrawListElement {
            shader_id: shader_ref.id(),
            mat_id: mat_ref.id(),
            mesh_id: mesh_ref.id(),
            bone_transform_index: bone_transform_index.unwrap_or(-1),
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

    fn build_asset_data(
        &mut self,
        render_state: &RenderState,
        scene: &Scene,
        render_options: &PipelineOptions,
    ) {
        let world = &scene.world;
        self.draw_list.clear();
        let mut query = <(Entity, &ComponentMesh)>::query();
        for (entity, c_mesh) in query.iter(world) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            let Some(mesh_ref) = c_mesh.mesh.get_ref(&self.asset_context.registries) else {
                continue;
            };
            let Some(mat_ref) = c_mesh.material.get_ref(&self.asset_context.registries) else {
                continue;
            };
            let transform = scene.world_transform(game_object);
            self.insert_draw_list_entry(&mesh_ref, &mat_ref, None, transform.matrix().into());
        }
        let mut skinned_meshes: HashSet<Uuid> = Default::default();
        let mut query = <(Entity, &ComponentSkinnedMesh)>::query();
        for (entity, c_skinned_mesh) in query.iter(world) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
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
                Some(bone_transform_index as i32),
                transform.matrix().into(),
            );
        }
        let mut query = <&ComponentSkyLight>::query();
        let mut skybox = None;
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
        }
        self.skybox_renderer.set_skybox(skybox);
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
                self.default_assets.missing_texture.clone(),
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

    fn build_material_bind_groups(
        device: &wgpu::Device,
        asset_context: &ReadOnlyAssetContext,
        default_texture: Ref<Texture>,
        cache: &mut HashMap<AssetId, CachedMaterialBindGroups>,
        assets: &LockedAssetRenderState,
    ) -> HashMap<AssetId, HashMap<u32, wgpu::BindGroup>> {
        let live_materials: HashSet<AssetId> = assets.materials.keys().copied().collect();
        cache.retain(|mat_id, _| live_materials.contains(mat_id));

        let mut bind_groups: HashMap<AssetId, HashMap<u32, wgpu::BindGroup>> = Default::default();
        for (mat_id, mat) in assets.materials.iter() {
            let key = mat.bind_group_cache_key(asset_context, default_texture.clone());

            let groups = match cache.get(mat_id) {
                Some(cached) if cached.key == key => cached.groups.clone(),
                _ => {
                    let groups =
                        mat.bind_groups(device, asset_context, assets, default_texture.clone());
                    cache.insert(
                        *mat_id,
                        CachedMaterialBindGroups {
                            key,
                            groups: groups.clone(),
                        },
                    );
                    groups
                }
            };

            bind_groups.insert(*mat_id, groups);
        }
        bind_groups
    }

    fn build_mesh_data(&mut self, render_state: &RenderState) {
        for (_, mut mesh) in self.assets.meshes.lock_write() {
            RenderUtils::rebuild_mesh_data(&render_state.device, &render_state.queue, &mut mesh);
        }
    }

    pub fn scene_texture(&self) -> &Texture {
        &self.scene_texture
    }

    pub fn scene_texture_handle(&self) -> Option<&egui::TextureHandle> {
        self.scene_texture.handle.as_ref()
    }

    fn create_textures(
        render_context: Arc<RenderContext>,
        width: u32,
        height: u32,
        samples: u32,
    ) -> (Texture, Texture, Texture) {
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
                format: wgpu::TextureFormat::Rg11b10Ufloat,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
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
                format: wgpu::TextureFormat::Rg11b10Ufloat,
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
        (scene_texture, scene_texture_msaa, scene_depth_texture)
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
        queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::cast_slice(&[camera_uniform]),
        );
    }

    pub fn resize_textures(&mut self, width: u32, height: u32) {
        if self.scene_texture.descriptor.size.width == width
            && self.scene_texture.descriptor.size.height == height
        {
            return;
        }
        (
            self.scene_texture,
            self.scene_texture_msaa,
            self.scene_depth_texture,
        ) = Self::create_textures(
            self.asset_context.render_context.clone(),
            width,
            height,
            self.options.samples,
        );
    }
}
