use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::default::Default;
use std::ops::Deref;
use std::sync::{RwLock, RwLockWriteGuard};

use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::{wgpu, RenderState};
use glm::{vec4, Mat4, Vec4};
use legion::{Entity, IntoQuery};

use crate::assets::mesh::Mesh;
use crate::assets::{AssetRegistry, Assets};
use crate::component::ComponentMesh;
use crate::component::ComponentTransform;
use crate::core::Ref;
use crate::math::Transform;
use crate::render::render_utils::RenderUtils;
use crate::render::{Camera, GizmoRenderer, Shader};
use crate::scene::Scene;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
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

#[allow(dead_code)]
pub struct SceneRenderer {
    scene_samples: u32,
    scene_texture: wgpu::Texture,
    scene_texture_handle: egui::TextureHandle,
    scene_texture_msaa: wgpu::Texture,
    scene_depth_texture: wgpu::Texture,
    scene_texture_view_msaa: wgpu::TextureView,
    scene_depth_texture_view: wgpu::TextureView,
    scene_bind_group: wgpu::BindGroup,
    scene_shader: Ref<Shader>,
    grid_shader: Ref<Shader>,
    camera_uniform_buffer: wgpu::Buffer,
    gizmo_renderer: GizmoRenderer,
    clear_color: Vec4,
}

impl SceneRenderer {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let render_state = cc.wgpu_render_state.as_ref().unwrap();
        let device = &render_state.device;
        let width = 1280;
        let height = 720;
        let samples = 1; // TODO: figure out why GTX 970 isn't supporting anti-aliasing

        // Textures
        let (scene_texture, scene_texture_msaa, scene_depth_texture) =
            Self::create_textures(device, width, height, samples);
        let scene_texture_view = scene_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let scene_texture_view_msaa =
            scene_texture_msaa.create_view(&wgpu::TextureViewDescriptor::default());
        let scene_depth_texture_view =
            scene_depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let scene_texture_id = render_state.renderer.write().register_native_texture(
            device,
            &scene_texture_view,
            wgpu::FilterMode::Linear,
        );
        let scene_texture_handle =
            egui::TextureHandle::new(cc.egui_ctx.tex_manager(), scene_texture_id);

        // Shaders
        let scene_shader;
        let grid_shader;
        {
            let mut registry = AssetRegistry::get_mut();
            scene_shader = registry.load::<Shader>("shaders/basic").unwrap();
            grid_shader = registry.load::<Shader>("shaders/grid").unwrap();
        }

        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_uniform_buffer"),
            contents: bytemuck::cast_slice(&[CameraUniform::default()]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bind_group"),
            layout: &scene_shader.read().unwrap().bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buffer.as_entire_binding(),
            }],
        });

        grid_shader.write().unwrap().set_fragment_targets(&[Some(
            RenderUtils::color_alpha_blending(render_state.target_format),
        )]);

        let gizmo_renderer = GizmoRenderer::new(cc, &camera_uniform_buffer, samples);

        Self {
            scene_samples: samples,
            scene_texture_msaa,
            scene_texture,
            scene_texture_view_msaa,
            scene_texture_handle,
            scene_depth_texture,
            scene_depth_texture_view,
            scene_bind_group,
            scene_shader,
            grid_shader,
            camera_uniform_buffer,
            gizmo_renderer,
            clear_color: vec4(0.03, 0.03, 0.03, 1.0),
        }
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
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Scene Encoder"),
        });
        let world = scene.world();
        {
            let mut mesh_map: HashMap<*const RwLock<Mesh>, &RwLock<Mesh>> = HashMap::new();
            let mut mesh_list: Vec<RwLockWriteGuard<Mesh>>;
            let scene_shader = self.scene_shader.read().unwrap();
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Viewport Scene"),
                    color_attachments: &[Some(RenderUtils::color_attachment(
                        &self.scene_texture_view_msaa,
                        &self.clear_color,
                    ))],
                    depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
                        &self.scene_depth_texture_view,
                        1.0,
                    )),
                });

                self.load_camera_uniforms(queue, camera, camera_transform);
                self.gizmo_renderer
                    .draw_gizmos(device, queue, camera_transform, scene);

                let mut query = <(Entity, &ComponentTransform, &ComponentMesh)>::query();
                for (entity, _, m_comp) in query.iter(world.deref()) {
                    if let Some(mesh_ref) = m_comp.mesh.as_ref() {
                        {
                            let mut mesh = mesh_ref.write().unwrap();
                            let ptr: *const RwLock<Mesh> = mesh_ref.deref().deref();
                            if !mesh_map.contains_key(&ptr) {
                                mesh.instances.clear();
                            }
                            let node = scene.get_node(*entity);
                            mesh.instances
                                .push(scene.get_world_transform(node).matrix.into());
                        }
                        mesh_map.insert(&***mesh_ref as *const _, &***mesh_ref);
                    }
                }

                // Lock all meshes for writing at once
                mesh_list = mesh_map
                    .values()
                    .map(|mesh| mesh.write().unwrap())
                    .collect();

                // Render scene meshes
                render_pass.set_pipeline(&scene_shader.pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);
                for mesh in mesh_list.iter_mut() {
                    RenderUtils::render_mesh(device, queue, &mut render_pass, mesh.borrow_mut());
                }

                // Render gizmos
                self.gizmo_renderer.render_gizmos(&mut render_pass);
            }
        }

        {
            let quad_binding = Assets::screen_space_quad().unwrap();
            let mut quad_mesh = quad_binding.write().unwrap();
            let grid_shader = self.grid_shader.read().unwrap();
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Scene Grid"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.scene_texture_view_msaa,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: true,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.scene_depth_texture_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: true,
                        }),
                        stencil_ops: None,
                    }),
                });

                // Render grid
                render_pass.set_pipeline(&grid_shader.pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);
                quad_mesh.instances.resize(1, *Mat4::identity().as_ref());
                RenderUtils::render_mesh(device, queue, &mut render_pass, quad_mesh.borrow_mut());
            }
        }

        // Resolve MSAA texture
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: &self.scene_texture_msaa,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            wgpu::ImageCopyTexture {
                texture: &self.scene_texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            wgpu::Extent3d {
                width: self.scene_texture.width(),
                height: self.scene_texture.height(),
                depth_or_array_layers: 1,
            },
        );

        queue.submit(Some(encoder.finish()));
    }

    pub fn scene_texture_handle(&self) -> &egui::TextureHandle {
        &self.scene_texture_handle
    }

    fn create_textures(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        samples: u32,
    ) -> (wgpu::Texture, wgpu::Texture, wgpu::Texture) {
        let scene_texture = device.create_texture(&wgpu::TextureDescriptor {
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
        });
        let scene_texture_msaa = device.create_texture(&wgpu::TextureDescriptor {
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
        });
        let scene_depth_texture = device.create_texture(&wgpu::TextureDescriptor {
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
        });
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

    pub fn resize_textures(
        &mut self,
        ctx: &egui::Context,
        render_state: &RenderState,
        width: u32,
        height: u32,
    ) {
        if self.scene_texture.width() == width && self.scene_texture.height() == height {
            return;
        }
        let device = &render_state.device;
        let (scene_texture, scene_texture_msaa, scene_depth_texture) =
            Self::create_textures(device, width, height, self.scene_samples);
        self.scene_texture = scene_texture;
        self.scene_texture_msaa = scene_texture_msaa;
        self.scene_depth_texture = scene_depth_texture;
        self.scene_texture_view_msaa = self.scene_texture_msaa.create_view(&Default::default());
        self.scene_depth_texture_view = self.scene_depth_texture.create_view(&Default::default());
        let scene_texture_view = self.scene_texture.create_view(&Default::default());
        let mut renderer = render_state.renderer.write();
        renderer.free_texture(&self.scene_texture_handle.id());
        let scene_texture_id =
            renderer.register_native_texture(device, &scene_texture_view, wgpu::FilterMode::Linear);
        self.scene_texture_handle = egui::TextureHandle::new(ctx.tex_manager(), scene_texture_id);
    }
}
