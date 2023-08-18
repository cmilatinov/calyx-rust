use std::collections::HashMap;
use std::mem;
use std::num::NonZeroU64;
use std::ops::Deref;
use std::sync::{RwLock, RwLockWriteGuard};

use eframe::wgpu::FilterMode;
use egui_wgpu::{RenderState, wgpu};
use egui_wgpu::wgpu::include_wgsl;
use egui_wgpu::wgpu::util::DeviceExt;
use glm::Mat4;
use specs::{Join, WorldExt};
use crate::assets::mesh;
use crate::assets::mesh::Mesh;
use crate::component::ComponentMesh;
use crate::component::ComponentTransform;
use crate::math::Transform;
use crate::render::buffer::BufferLayout;

use crate::render::Camera;
use crate::scene::Scene;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub projection: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            projection: Mat4::identity().data.0,
            view: Mat4::identity().data.0,
        }
    }
}

pub struct SceneRenderer {
    pub scene_texture_handle: egui::TextureHandle,
    scene_texture: wgpu::Texture,
    scene_texture_view: wgpu::TextureView,
    scene_depth_texture: wgpu::Texture,
    scene_depth_texture_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    camera_uniform_buffer: wgpu::Buffer
}

impl<'rs> SceneRenderer {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let render_state = cc.wgpu_render_state.as_ref().unwrap();
        let device = &render_state.device;

        let scene_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Scene Texture"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC |
                wgpu::TextureUsages::RENDER_ATTACHMENT |
                wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let scene_texture_view = scene_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let scene_texture_id = render_state.renderer.write()
            .register_native_texture(
                device,
                &scene_texture_view,
                FilterMode::Linear,
            );
        let scene_texture_handle = egui::TextureHandle::new(
            cc.egui_ctx.tex_manager(),
            scene_texture_id,
        );

        let scene_depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Scene Depth Texture"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let scene_depth_texture_view = scene_depth_texture.create_view(
            &wgpu::TextureViewDescriptor::default()
        );

        let shader = device.create_shader_module(
            include_wgsl!("../../../assets/shaders/basic.wgsl")
        );

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("custom3d"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(mem::size_of::<CameraUniform>() as u64),
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("custom3d"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("custom3d"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[
                    mesh::Vertex::layout(wgpu::VertexStepMode::Vertex),
                    mesh::Instance::layout(wgpu::VertexStepMode::Instance)
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(render_state.target_format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default()
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("custom3d"),
            contents: bytemuck::cast_slice(&[CameraUniform::default()]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("custom3d"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buffer.as_entire_binding(),
            }],
        });

        Self {
            scene_texture,
            scene_texture_view,
            scene_texture_handle,
            scene_depth_texture,
            scene_depth_texture_view,
            pipeline,
            bind_group,
            camera_uniform_buffer
        }
    }

    pub fn render_scene(
        &self,
        render_state: &RenderState,
        camera_transform: &Transform,
        _camera: &Camera,
        scene: &Scene,
    ) {
        let queue = &render_state.queue;
        let device = &render_state.device;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Scene Encoder")
        });
        let e_s = scene.world.entities();
        let t_s = scene.world.read_component::<ComponentTransform>();
        let m_s = scene.world.read_component::<ComponentMesh>();
        let mut mesh_map: HashMap<*const RwLock<Mesh>, &RwLock<Mesh>> = HashMap::new();
        let mut mesh_list: Vec<RwLockWriteGuard<Mesh>> = Vec::new();
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Viewport Scene"),
                color_attachments: &[Some(self.color_attachment())],
                depth_stencil_attachment: Some(self.depth_stencil_attachment()),
            });

            let mut camera_uniform = CameraUniform::default();
            camera_uniform.projection = glm::perspective_lh::<f32>(
                16.0 / 9.0,
                45.0_f32.to_radians() as f32,
                0.1,
                100.0,
            ).data.0;
            camera_uniform.view.clone_from_slice(&camera_transform.get_inverse_matrix().data.0);
            queue.write_buffer(
                &self.camera_uniform_buffer,
                0,
                bytemuck::cast_slice(&[camera_uniform]),
            );

            let default = ComponentTransform::default();
            for (id, m_comp) in (&e_s, &m_s).join() {
                let t_comp = t_s.get(id).unwrap_or(&default);
                {
                    let mut mesh = m_comp.mesh.write().unwrap();
                    let ptr: *const RwLock<Mesh> = m_comp.mesh.deref();
                    if !mesh_map.contains_key(&ptr) {
                        mesh.instances.clear();
                    }
                    mesh.instances.push(
                        t_comp.transform.get_matrix().into()
                    );
                }
                mesh_map.insert(&*m_comp.mesh as *const _, &*m_comp.mesh);
            }

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);

            // Lock all meshes for writing at once
            mesh_list = mesh_map.iter()
                .map(|i| i.1.write().unwrap())
                .collect();

            for mesh in mesh_list.iter_mut() {
                if mesh.dirty {
                    mesh.rebuild_mesh_data(device);
                    mesh.dirty = false;
                }
                mesh.rebuild_instance_data(device);
                render_pass.set_index_buffer(
                    mesh.index_buffer.as_ref().unwrap().slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.set_vertex_buffer(0, mesh.vertex_buffer.as_ref().unwrap().slice(..));
                render_pass.set_vertex_buffer(1, mesh.instance_buffer.as_ref().unwrap().slice(..));
                render_pass.draw_indexed(
                    0..(mesh.indices.len() as u32), 0,
                    0..(mesh.instances.len() as u32)
                );
            }
        }

        queue.submit(Some(encoder.finish()));
    }

    fn color_attachment(&self) -> wgpu::RenderPassColorAttachment {
        wgpu::RenderPassColorAttachment {
            view: &self.scene_texture_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.03,
                    g: 0.03,
                    b: 0.03,
                    a: 1.0,
                }),
                store: true,
            },
        }
    }

    fn depth_stencil_attachment(&self) -> wgpu::RenderPassDepthStencilAttachment {
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.scene_depth_texture_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: true
            }),
            stencil_ops: None
        }
    }
}
