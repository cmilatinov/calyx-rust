use std::mem;
use std::num::NonZeroU64;
use eframe::wgpu::FilterMode;
use egui::TextureHandle;
use egui_wgpu::{RenderState, wgpu};
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::wgpu::include_wgsl;
use glm::Mat4;
use crate::render::Camera;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub projection: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            projection: Mat4::identity().data.0,
            view: Mat4::identity().data.0,
            model: Mat4::identity().data.0
        }
    }
}

pub struct SceneRenderer {
    pub scene_texture: wgpu::Texture,
    pub scene_texture_view: wgpu::TextureView,
    pub scene_texture_handle: egui::TextureHandle,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub camera_uniform_buffer: wgpu::Buffer,
}

impl<'rs> SceneRenderer {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let render_state = cc.wgpu_render_state.as_ref().unwrap();
        let device = &render_state.device;

        let scene_texture = device.create_texture(&wgpu::TextureDescriptor {
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
            label: Some("Scene Texture"),
            view_formats: &[],
        });
        let scene_texture_view = scene_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let scene_texture_id = render_state.renderer.write()
            .register_native_texture(
                device,
                &scene_texture_view,
                FilterMode::Linear
            );
        let scene_texture_handle = TextureHandle::new(cc.egui_ctx.tex_manager(), scene_texture_id);

        let shader = device.create_shader_module(include_wgsl!("../../../assets/shaders/basic.wgsl"));

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
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(render_state.target_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
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
            pipeline,
            bind_group,
            camera_uniform_buffer,
        }
    }

    pub fn update(&self, render_state: &RenderState, camera: &Camera) {
        let queue = &render_state.queue;
        let device = &render_state.device;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Scene Encoder")
        });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Viewport Scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
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
                })],
                depth_stencil_attachment: None,
            });
            self.render(queue, &mut render_pass, camera);
        }
        queue.submit(Some(encoder.finish()));
    }

    fn render<'rp>(
        &'rp self,
        queue: &wgpu::Queue,
        render_pass: &mut wgpu::RenderPass<'rp>,
        camera: &Camera
    ) {
        let mut camera_uniform = CameraUniform::default();
        camera_uniform.projection = glm::perspective_lh::<f32>(
            16.0 / 9.0,
            45.0_f32.to_radians() as f32,
            0.1,
            100.0
        ).data.0;
        camera_uniform.view.clone_from_slice(&camera.transform.get_inverse_matrix().data.0);
        queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::cast_slice(&[camera_uniform]),
        );
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}