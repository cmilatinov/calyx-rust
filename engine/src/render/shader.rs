use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use eframe::wgpu::ShaderSource;
use egui_wgpu::wgpu;

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::{mesh, Asset, LoadedAsset};
use crate::render::buffer::BufferLayout;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, RenderContext};
use crate::utils::TypeUuid;

pub type BindGroupEntries = BTreeMap<u32, Vec<wgpu::BindGroupLayoutEntry>>;
pub type BindGroupLayouts = Vec<wgpu::BindGroupLayout>;

#[derive(TypeUuid)]
#[uuid = "00415831-a64c-4dc2-b573-5e112f99b674"]
pub struct Shader {
    pub shader: wgpu::ShaderModule,
    pub bind_group_layouts: BindGroupLayouts,
    pub bind_group_entries: BindGroupEntries,
    pub pipeline_layout: wgpu::PipelineLayout,
    pub pipelines: HashMap<PipelineOptions, wgpu::RenderPipeline>,
    pub module: naga::Module,
}

impl Asset for Shader {
    fn get_file_extensions() -> &'static [&'static str] {
        &["wgsl"]
    }
    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError> {
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

        let module =
            naga::front::wgsl::parse_str(shader_src.as_str()).map_err(|_| AssetError::LoadError)?;

        let bind_group_entries = Self::bind_group_entries(&module);
        let bind_group_layouts = Self::bind_group_layouts(&device, &bind_group_entries);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: bind_group_layouts.iter().collect::<Vec<_>>().as_slice(),
            push_constant_ranges: &[],
        });

        Ok(LoadedAsset::new(Self {
            shader,
            bind_group_layouts,
            bind_group_entries,
            pipeline_layout,
            pipelines: HashMap::new(),
            module,
        }))
    }
}

impl Shader {
    fn binding_type(variable: &naga::GlobalVariable, ty: &naga::Type) -> wgpu::BindingType {
        match &ty.inner {
            naga::TypeInner::Image { dim, class, .. } => wgpu::BindingType::Texture {
                sample_type: match class {
                    naga::ImageClass::Sampled { kind, .. } => match kind {
                        naga::ScalarKind::Uint => wgpu::TextureSampleType::Uint,
                        naga::ScalarKind::Sint => wgpu::TextureSampleType::Sint,
                        naga::ScalarKind::Float => {
                            wgpu::TextureSampleType::Float { filterable: true }
                        }
                        _ => wgpu::TextureSampleType::Uint,
                    },
                    naga::ImageClass::Depth { .. } => wgpu::TextureSampleType::Depth,
                    _ => wgpu::TextureSampleType::Uint,
                },
                view_dimension: match dim {
                    naga::ImageDimension::D1 => wgpu::TextureViewDimension::D1,
                    naga::ImageDimension::D2 => wgpu::TextureViewDimension::D2,
                    naga::ImageDimension::D3 => wgpu::TextureViewDimension::D3,
                    naga::ImageDimension::Cube => wgpu::TextureViewDimension::Cube,
                },
                multisampled: match class {
                    naga::ImageClass::Sampled { multi, .. } => *multi,
                    naga::ImageClass::Depth { multi, .. } => *multi,
                    _ => false,
                },
            },
            naga::TypeInner::Sampler { comparison } => wgpu::BindingType::Sampler(if *comparison {
                wgpu::SamplerBindingType::Comparison
            } else {
                wgpu::SamplerBindingType::Filtering
            }),
            _ => wgpu::BindingType::Buffer {
                ty: match variable.space {
                    naga::AddressSpace::Uniform => wgpu::BufferBindingType::Uniform,
                    naga::AddressSpace::Storage { access } => wgpu::BufferBindingType::Storage {
                        read_only: access == naga::StorageAccess::LOAD,
                    },
                    _ => wgpu::BufferBindingType::Uniform,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        }
    }
    fn bind_group_layout_entry(
        variable: &naga::GlobalVariable,
        ty: &naga::Type,
        binding: u32,
    ) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: Self::binding_type(variable, ty),
            count: None,
        }
    }

    fn bind_group_layout(
        module: &naga::Module,
        variable: &naga::GlobalVariable,
        groups: &mut BindGroupEntries,
    ) {
        let ty = &module.types[variable.ty];
        if let Some(binding) = &variable.binding {
            let entries = groups.entry(binding.group).or_default();
            entries.push(Self::bind_group_layout_entry(variable, ty, binding.binding));
        }
    }

    fn bind_group_entries(module: &naga::Module) -> BindGroupEntries {
        let mut groups = Default::default();
        for (_, variable) in module.global_variables.iter() {
            Self::bind_group_layout(module, variable, &mut groups);
        }
        groups
    }

    fn bind_group_layouts(device: &wgpu::Device, groups: &BindGroupEntries) -> BindGroupLayouts {
        groups
            .iter()
            .map(|(_, entries)| {
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: entries.as_slice(),
                })
            })
            .collect()
    }

    pub fn rebuild_pipeline_layout(&mut self) {
        let device = RenderContext::device().unwrap();
        self.pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: self
                .bind_group_layouts
                .iter()
                .collect::<Vec<_>>()
                .as_slice(),
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
