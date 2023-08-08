use std::num::NonZeroU64;
use eframe::wgpu::{CommandBuffer, Device, RenderPass};
use egui_wgpu::{RenderState, wgpu};
use egui_wgpu::wgpu::Queue;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::wgpu::include_wgsl;
use glm::Mat4;

#[repr(C)]
#[derive(Default, Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub projection: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4]
}

pub struct SceneRenderer {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub uniform_buffer: wgpu::Buffer,
    pub camera: CameraUniform
}

impl<'rs> SceneRenderer {
    pub fn new(render_state: &RenderState) -> Self {
        let device = &render_state.device;

        let shader = device.create_shader_module(include_wgsl!("../../../assets/shaders/basic.wgsl"));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("custom3d"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(64 * 3),
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

        let uniform: CameraUniform = CameraUniform {
            projection: Mat4::identity().data.0,
            view: Mat4::identity().data.0,
            model: Mat4::identity().data.0
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("custom3d"),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("custom3d"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        Self {
            pipeline,
            bind_group,
            uniform_buffer,
            camera: CameraUniform::default()
        }
    }

    pub fn prepare(&self, _device: &Device, queue: &Queue) -> Vec<CommandBuffer> {
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.camera]),
        );
        Vec::new()
    }

    pub fn paint<'rp>(&'rp self, render_pass: &mut RenderPass<'rp>) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}