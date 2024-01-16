use std::collections::HashMap;
use std::default::Default;
use std::ops::{Deref, Range};

use egui::Color32;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::{wgpu, RenderState};
use glm::{Mat4, Vec3};
use legion::{Entity, IntoQuery};

use crate::assets::texture::Texture2D;
use crate::assets::{AssetRegistry, Assets};
use crate::component::{ComponentMesh, ComponentPointLight};
use crate::core::Ref;
use crate::math::Transform;
use crate::render::asset_render_state::AssetRenderState;
use crate::render::buffer::ResizableBuffer;
use crate::render::render_utils::RenderUtils;
use crate::render::{Camera, GizmoRenderer, PipelineOptions, PipelineOptionsBuilder, Shader};
use crate::scene::Scene;

use super::{LockedAssetRenderState, RenderContext};

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

#[repr(C)]
#[derive(Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PointLight {
    position: [f32; 3],
    radius: f32,
    color: [f32; 3],
    _padding2: f32,
}

#[derive(Default)]
pub struct SceneRendererOptions {
    pub grid: bool,
    pub gizmos: bool,
    pub clear_color: Color32,
    // TODO: figure out why GTX 970 isn't supporting MSAA
    pub samples: u32,
}

#[allow(dead_code)]
pub struct SceneRenderer {
    options: SceneRendererOptions,
    scene_texture: Texture2D,
    scene_depth_texture: Texture2D,
    scene_texture_msaa: Texture2D,
    scene_bind_group: wgpu::BindGroup,
    scene_shader: Ref<Shader>,
    grid_shader: Ref<Shader>,
    camera_uniform_buffer: wgpu::Buffer,
    light_storage_buffer: ResizableBuffer,
    gizmo_renderer: GizmoRenderer,
    assets: AssetRenderState,
    draw_list: Vec<(usize, usize, usize, [[f32; 4]; 4])>,
}

impl SceneRenderer {
    pub fn new(mut options: SceneRendererOptions) -> Self {
        let render_state = RenderContext::render_state().unwrap();
        let device = &render_state.device;
        let width = 1280;
        let height = 720;
        options.samples = options.samples.max(1);

        // Textures
        let (scene_texture, scene_texture_msaa, scene_depth_texture) =
            Self::create_textures(width, height, options.samples);

        // Shaders
        let scene_shader;
        let grid_shader;
        {
            let registry = AssetRegistry::get();
            scene_shader = registry.load::<Shader>("shaders/basic").unwrap();
            grid_shader = registry.load::<Shader>("shaders/grid").unwrap();
        }

        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_uniform_buffer"),
            contents: bytemuck::cast_slice(&[CameraUniform::default()]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let light_storage_buffer =
            ResizableBuffer::new(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bind_group"),
            layout: &scene_shader.read().bind_group_layouts[0],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buffer.as_entire_binding(),
            }],
        });

        let gizmo_renderer = GizmoRenderer::new(&camera_uniform_buffer, options.samples);

        Self {
            options,
            scene_texture_msaa,
            scene_texture,
            scene_depth_texture,
            scene_bind_group,
            scene_shader,
            grid_shader,
            camera_uniform_buffer,
            light_storage_buffer,
            gizmo_renderer,
            assets: Default::default(),
            draw_list: Default::default(),
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
    ) {
        let queue = &render_state.queue;
        let device = &render_state.device;

        self.load_camera_uniforms(queue, camera, camera_transform);
        if self.options.gizmos {
            self.gizmo_renderer
                .draw_gizmos(device, queue, camera_transform, scene);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("encoder"),
        });
        self.render_meshes(render_state, scene, &mut encoder);
        if self.options.grid {
            self.render_grid(render_state, &mut encoder);
        }

        // Resolve MSAA texture
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: &self.scene_texture_msaa.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            wgpu::ImageCopyTexture {
                texture: &self.scene_texture.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            self.scene_texture.size,
        );

        queue.submit(Some(encoder.finish()));
    }

    fn render_meshes(
        &mut self,
        render_state: &RenderState,
        scene: &Scene,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let device = &render_state.device;
        let options = PipelineOptionsBuilder::default()
            .samples(self.options.samples)
            .build();
        self.build_asset_data(render_state, scene, &options);
        let draw_list = self.build_draw_list();
        self.build_mesh_data(render_state);
        self.build_light_data(render_state, scene);
        let assets = self.assets.lock();
        let material_bind_groups = self.build_material_bind_groups(device, &assets);
        let light_storage_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light_storage_bind_group"),
            layout: &self.scene_shader.read().bind_group_layouts[1],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self
                    .light_storage_buffer
                    .get_wgpu_buffer()
                    .as_entire_binding(),
            }],
        });
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
                )),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut last: (usize, usize, usize) = Default::default();
            for (shader_id, mat_id, mesh_id, instances) in draw_list {
                let shader = assets.shader(shader_id);
                let mesh = assets.mesh(mesh_id);
                if shader_id != last.0 {
                    if let Some(pipeline) = shader.get_pipeline(&options) {
                        render_pass.set_pipeline(pipeline);
                        render_pass.set_bind_group(0, &self.scene_bind_group, &[]);
                        render_pass.set_bind_group(1, &light_storage_bind_group, &[]);
                    }
                }
                if mat_id != last.1 {
                    if let Some(groups) = material_bind_groups.get(&mat_id) {
                        for (index, group) in groups {
                            render_pass.set_bind_group(*index, group, &[]);
                        }
                    }
                }
                RenderUtils::bind_mesh_buffers(&mut render_pass, &mesh);
                RenderUtils::draw_mesh_instanced(&mut render_pass, mesh, instances);
                last = (shader_id, mat_id, mesh_id);
            }

            // Render gizmos
            if self.options.gizmos {
                self.gizmo_renderer.render_gizmos(&mut render_pass);
            }
        }
    }

    fn render_grid(&mut self, render_state: &RenderState, encoder: &mut wgpu::CommandEncoder) {
        let device = &render_state.device;
        let queue = &render_state.queue;
        let quad_binding = Assets::screen_space_quad().unwrap();
        let mut quad_mesh = quad_binding.write();
        let mut grid_shader = self.grid_shader.write();
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Scene Grid"),
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

            // Render grid
            let options = PipelineOptionsBuilder::default()
                .samples(self.options.samples)
                .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(
                    render_state.target_format,
                ))])
                .build();
            grid_shader.build_pipeline(&options);
            if let Some(pipeline) = grid_shader.get_pipeline(&options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);
                quad_mesh.instances.resize(1, *Mat4::identity().as_ref());
                RenderUtils::render_mesh(device, queue, &mut render_pass, &mut quad_mesh);
            }
        }
    }

    fn build_draw_list(&mut self) -> Vec<(usize, usize, usize, Range<u32>)> {
        let mut last: (usize, usize, usize) = Default::default();
        let mut mesh_instances: HashMap<usize, u32> = Default::default();
        let mut instance_count: u32 = 0;
        let mut mesh = None;
        let mut draw_list: Vec<(usize, usize, usize, Range<u32>)> = Default::default();
        let mut insert_instance_list = |last: (usize, usize, usize), instance_count: u32| {
            if last != Default::default() {
                let entry = mesh_instances.entry(last.2).or_default();
                let start = *entry;
                draw_list.push((last.0, last.1, last.2, start..(start + instance_count)));
                *entry += instance_count;
            }
        };
        for (shader_id, mat_id, mesh_id, matrix) in self.draw_list.drain(0..) {
            if (shader_id, mat_id, mesh_id) != last {
                mesh = Some(self.assets.mesh(mesh_id).write());
                insert_instance_list(last, instance_count);
                instance_count = 0;
            }
            if let Some(ref mut mesh) = &mut mesh {
                mesh.instances.push(matrix);
                instance_count += 1;
            }
            last = (shader_id, mat_id, mesh_id);
        }
        insert_instance_list(last, instance_count);
        draw_list
    }

    fn build_asset_data(
        &mut self,
        render_state: &RenderState,
        scene: &Scene,
        render_options: &PipelineOptions,
    ) {
        let world = scene.world();
        self.draw_list.clear();
        let mut query = <(Entity, &ComponentMesh)>::query();
        for (entity, c_mesh) in query.iter(world.deref()) {
            if let Some(mesh_ref) = c_mesh.mesh.as_ref() {
                if let Some(mat_ref) = c_mesh.material.as_ref() {
                    let shader_ref = mat_ref.read().shader.clone();
                    let node = scene.get_node(*entity);
                    self.draw_list.push((
                        shader_ref.id(),
                        mat_ref.id(),
                        mesh_ref.id(),
                        scene.get_world_transform(node).matrix.into(),
                    ));
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
            }
        }
        for (_, mut mesh) in self.assets.meshes.lock_write() {
            mesh.instances.clear();
        }
        for (_, mut shader) in self.assets.shaders.lock_write() {
            shader.build_pipeline(render_options);
        }
        for (_, mut material) in self.assets.materials.lock_write() {
            material.load_buffers(render_state);
            material.collect_textures(&mut self.assets.textures);
        }
        self.draw_list.sort_by_key(|(s, mat, m, _)| (*s, *mat, *m));
    }

    fn build_material_bind_groups(
        &self,
        device: &wgpu::Device,
        assets: &LockedAssetRenderState,
    ) -> HashMap<usize, HashMap<u32, wgpu::BindGroup>> {
        let mut bind_groups: HashMap<usize, HashMap<u32, wgpu::BindGroup>> = Default::default();
        for (mat_id, mat) in assets.materials.iter() {
            bind_groups.insert(*mat_id, mat.bind_groups(device, assets));
        }
        bind_groups
    }

    fn build_mesh_data(&mut self, render_state: &RenderState) {
        for (_, mut mesh) in self.assets.meshes.lock_write() {
            RenderUtils::rebuild_mesh_data(&render_state.device, &render_state.queue, &mut mesh);
        }
    }

    fn build_light_data(&mut self, render_state: &RenderState, scene: &Scene) {
        let device = &render_state.device;
        let queue = &render_state.queue;
        let world = scene.world();
        let mut lights = Vec::new();
        let mut query = <(Entity, &ComponentPointLight)>::query();
        for (entity, light) in query.iter(world.deref()) {
            if !light.active {
                continue;
            }
            let mut color = Vec3::default();
            color.copy_from_slice(&light.color.to_normalized_gamma_f32()[0..3]);
            lights.push(PointLight {
                color: color.into(),
                radius: light.radius,
                position: scene
                    .get_world_transform(scene.get_node(*entity))
                    .position
                    .into(),
                ..Default::default()
            });
        }
        let size = (16 + std::cmp::max(lights.len(), 1) * std::mem::size_of::<PointLight>()) as u64;
        self.light_storage_buffer.resize(device, size);
        self.light_storage_buffer
            .write_buffer(device, queue, &[lights.len() as u32], None);
        if !lights.is_empty() {
            self.light_storage_buffer
                .write_buffer(device, queue, lights.as_slice(), Some(16));
        }
    }

    pub fn scene_texture(&self) -> &Texture2D {
        &self.scene_texture
    }

    pub fn scene_texture_handle(&self) -> &egui::TextureHandle {
        self.scene_texture.handle.as_ref().unwrap()
    }

    fn create_textures(width: u32, height: u32, samples: u32) -> (Texture2D, Texture2D, Texture2D) {
        let scene_texture = Texture2D::new(
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
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            true,
        );
        let scene_texture_msaa = Texture2D::new(
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
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            false,
        );
        let scene_depth_texture = Texture2D::new(
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
        let projection = camera.projection;
        let view = camera_transform.get_inverse_matrix();
        camera_uniform
            .projection
            .clone_from_slice(projection.as_ref());
        camera_uniform.view.clone_from_slice(view.as_ref());
        camera_uniform
            .inverse_projection
            .clone_from_slice(glm::inverse(&projection).as_ref());
        camera_uniform
            .inverse_view
            .clone_from_slice(glm::inverse(&view).as_ref());
        camera_uniform.near_plane = camera.near_plane;
        camera_uniform.far_plane = camera.far_plane;
        queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::cast_slice(&[camera_uniform]),
        );
    }

    pub fn resize_textures(&mut self, width: u32, height: u32) {
        if self.scene_texture.size.width == width && self.scene_texture.size.height == height {
            return;
        }
        (
            self.scene_texture,
            self.scene_texture_msaa,
            self.scene_depth_texture,
        ) = Self::create_textures(width, height, self.options.samples);
    }
}
