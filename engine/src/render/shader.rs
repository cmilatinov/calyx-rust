use std::collections::HashMap;
use std::fs;
use std::path::Path;

use eframe::wgpu::ShaderSource;
use egui_wgpu::wgpu;

use crate::assets::error::AssetError;
use crate::assets::{mesh, Asset};
use crate::render::buffer::BufferLayout;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, RenderContext};

pub struct Shader {
    pub shader: wgpu::ShaderModule,
    pub bind_group_layouts: Vec<wgpu::BindGroupLayout>,
    pub pipeline_layout: wgpu::PipelineLayout,
    pub pipelines: HashMap<PipelineOptions, wgpu::RenderPipeline>,
    pub module: naga::Module
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
                    .and_then(|f| f.to_str())
                    .ok_or(AssetError::LoadError)?,
            ),
            source: ShaderSource::Wgsl(shader_src.clone().into()),
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

        let lighting_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("lighting_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let bind_group_layouts = vec![
            bind_group_layout,
            lighting_bind_group_layout,
            texture_bind_group_layout,
        ];
        let layouts = bind_group_layouts.iter().collect::<Vec<_>>();

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: layouts.as_slice(),
            push_constant_ranges: &[],
        });

        let module = naga::front::wgsl::parse_str(shader_src.as_str())
            .map_err(|_| AssetError::LoadError)?;

        Ok(Self {
            shader,
            bind_group_layouts,
            pipeline_layout,
            pipelines: HashMap::new(),
            module
        })
    }
}

impl Shader {
    pub fn rebuild_pipeline_layout(&mut self) {
        let device = RenderContext::device().unwrap();
        let layouts: Vec<&wgpu::BindGroupLayout> = self.bind_group_layouts.iter().collect();
        self.pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: layouts.as_slice(),
            push_constant_ranges: &[],
        });
        self.pipelines.clear();
    }

    pub fn build_pipeline(&mut self, options: &PipelineOptions) {
        if self.pipelines.contains_key(options) {
            return;
        }
        let device = RenderContext::device().unwrap();
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                targets: options.fragment_targets.as_slice(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: options.primitive_topology,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: options.cull_mode,
                polygon_mode: options.polygon_mode,
                ..Default::default()
            },
            depth_stencil: Some(RenderUtils::depth_default()),
            multisample: RenderUtils::multisample_default(options.samples),
            multiview: None,
        });
        self.pipelines.insert(options.clone(), pipeline);
    }

    pub fn get_pipeline(&self, options: &PipelineOptions) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(options)
    }
}
