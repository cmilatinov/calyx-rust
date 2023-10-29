use std::fs;
use std::path::Path;

use eframe::wgpu::ShaderSource;
use egui_wgpu::wgpu;

use crate::assets::error::AssetError;
use crate::assets::{mesh, Asset};
use crate::render::buffer::BufferLayout;
use crate::render::render_utils::RenderUtils;
use crate::render::RenderContext;

pub struct Shader {
    pub shader: wgpu::ShaderModule,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline_layout: wgpu::PipelineLayout,
    pub pipeline: wgpu::RenderPipeline,
}

impl Asset for Shader {
    fn get_file_extensions() -> &'static [&'static str] {
        &["wgsl"]
    }
    fn from_file(path: &Path) -> Result<Self, AssetError> {
        let device = RenderContext::device().unwrap();
        let shader_src = fs::read_to_string(path).map_err(|_| AssetError::LoadError)?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(
                path.file_stem()
                    .ok_or(AssetError::LoadError)?
                    .to_str()
                    .ok_or(AssetError::LoadError)?,
            ),
            source: ShaderSource::Wgsl(shader_src.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[
                    mesh::Vertex::layout(wgpu::VertexStepMode::Vertex),
                    mesh::Instance::layout(wgpu::VertexStepMode::Instance),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[RenderContext::texture_format().map(RenderUtils::color_default)],
            }),
            primitive: RenderUtils::primitive_default(wgpu::PrimitiveTopology::TriangleList),
            depth_stencil: Some(RenderUtils::depth_default()),
            multisample: RenderUtils::multisample_default(1),
            multiview: None,
        });

        Ok(Self {
            shader,
            bind_group_layout,
            pipeline_layout,
            pipeline,
        })
    }
}

impl Shader {
    pub fn set_fragment_targets(&mut self, targets: &[Option<wgpu::ColorTargetState>]) {
        let device = RenderContext::device().unwrap();
        self.pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: "vs_main",
                buffers: &[
                    mesh::Vertex::layout(wgpu::VertexStepMode::Vertex),
                    mesh::Instance::layout(wgpu::VertexStepMode::Instance),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: "fs_main",
                targets,
            }),
            primitive: RenderUtils::primitive_default(wgpu::PrimitiveTopology::TriangleList),
            depth_stencil: Some(RenderUtils::depth_default()),
            multisample: RenderUtils::multisample_default(1),
            multiview: None,
        });
    }
}
