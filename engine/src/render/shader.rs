use super::shader_preprocessor::ShaderPreprocessor;
use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::{mesh, Asset, LoadedAsset};
use crate::context::ReadOnlyAssetContext;
use crate::render::buffer::BufferLayout;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, RenderContext};
use crate::utils::TypeUuid;
use eframe::wgpu::ShaderSource;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::PipelineCompilationOptions;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

pub type BindGroupEntries = BTreeMap<u32, Vec<wgpu::BindGroupLayoutEntry>>;
pub type BindGroupLayouts = Vec<wgpu::BindGroupLayout>;

#[derive(PartialEq, Eq)]
pub enum ShaderType {
    VertexFragment,
    Compute,
}

#[derive(TypeUuid)]
#[uuid = "00415831-a64c-4dc2-b573-5e112f99b674"]
pub struct Shader {
    pub(crate) render_context: Arc<RenderContext>,
    pub ty: ShaderType,
    pub name: String,
    pub source: String,
    pub shader: wgpu::ShaderModule,
    pub bind_group_layouts: BindGroupLayouts,
    pub bind_group_entries: BindGroupEntries,
    pub pipeline_layout: wgpu::PipelineLayout,
    pub compute_pipeline: Option<wgpu::ComputePipeline>,
    pub pipelines: HashMap<PipelineOptions, wgpu::RenderPipeline>,
    pub module: naga::Module,
}

impl Asset for Shader {
    fn file_extensions() -> &'static [&'static str] {
        &["wgsl"]
    }

    fn from_file(
        game: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        let render_context = game.render_context.clone();
        let device = render_context.device();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("shader")
            .to_string();
        let source = ShaderPreprocessor::load_shader_source(&game.asset_registry.read(), path)
            .map_err(|_| AssetError::LoadError)?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(
                path.file_stem()
                    .and_then(|f| f.to_str())
                    .ok_or(AssetError::LoadError)?,
            ),
            source: ShaderSource::Wgsl(Cow::Borrowed(source.as_str())),
        });

        let module =
            naga::front::wgsl::parse_str(source.as_str()).map_err(|_| AssetError::LoadError)?;

        let ty = Self::shader_type(&module);
        let bind_group_entries = Self::bind_group_entries(&module);
        let bind_group_layouts = Self::bind_group_layouts(device, &bind_group_entries);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: bind_group_layouts.iter().collect::<Vec<_>>().as_slice(),
            push_constant_ranges: &[],
        });

        let compute_pipeline = if let ShaderType::Compute = ty {
            Some(
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name.as_str()),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some("compute_main"),
                    compilation_options: Default::default(),
                    cache: None,
                }),
            )
        } else {
            None
        };

        Ok(LoadedAsset::new(Self {
            render_context,
            ty,
            name,
            source,
            shader,
            bind_group_layouts,
            bind_group_entries,
            pipeline_layout,
            compute_pipeline,
            pipelines: HashMap::new(),
            module,
        }))
    }
}

impl Shader {
    fn shader_type(module: &naga::Module) -> ShaderType {
        if module
            .entry_points
            .iter()
            .any(|ep| ep.stage == naga::ShaderStage::Compute)
        {
            ShaderType::Compute
        } else {
            ShaderType::VertexFragment
        }
    }

    fn texture_format(format: naga::StorageFormat) -> wgpu::TextureFormat {
        match format {
            // 8-bit formats
            naga::StorageFormat::R8Unorm => wgpu::TextureFormat::R8Unorm,
            naga::StorageFormat::R8Snorm => wgpu::TextureFormat::R8Snorm,
            naga::StorageFormat::R8Uint => wgpu::TextureFormat::R8Uint,
            naga::StorageFormat::R8Sint => wgpu::TextureFormat::R8Sint,

            // 16-bit formats
            naga::StorageFormat::R16Uint => wgpu::TextureFormat::R16Uint,
            naga::StorageFormat::R16Sint => wgpu::TextureFormat::R16Sint,
            naga::StorageFormat::R16Float => wgpu::TextureFormat::R16Float,
            naga::StorageFormat::Rg8Unorm => wgpu::TextureFormat::Rg8Unorm,
            naga::StorageFormat::Rg8Snorm => wgpu::TextureFormat::Rg8Snorm,
            naga::StorageFormat::Rg8Uint => wgpu::TextureFormat::Rg8Uint,
            naga::StorageFormat::Rg8Sint => wgpu::TextureFormat::Rg8Sint,

            // 32-bit formats
            naga::StorageFormat::R32Uint => wgpu::TextureFormat::R32Uint,
            naga::StorageFormat::R32Sint => wgpu::TextureFormat::R32Sint,
            naga::StorageFormat::R32Float => wgpu::TextureFormat::R32Float,
            naga::StorageFormat::Rg16Uint => wgpu::TextureFormat::Rg16Uint,
            naga::StorageFormat::Rg16Sint => wgpu::TextureFormat::Rg16Sint,
            naga::StorageFormat::Rg16Float => wgpu::TextureFormat::Rg16Float,
            naga::StorageFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
            naga::StorageFormat::Rgba8Snorm => wgpu::TextureFormat::Rgba8Snorm,
            naga::StorageFormat::Rgba8Uint => wgpu::TextureFormat::Rgba8Uint,
            naga::StorageFormat::Rgba8Sint => wgpu::TextureFormat::Rgba8Sint,
            naga::StorageFormat::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,

            // Packed 32-bit formats
            naga::StorageFormat::Rgb10a2Uint => wgpu::TextureFormat::Rgb10a2Uint,
            naga::StorageFormat::Rgb10a2Unorm => wgpu::TextureFormat::Rgb10a2Unorm,
            naga::StorageFormat::Rg11b10Ufloat => wgpu::TextureFormat::Rg11b10Ufloat,

            // 64-bit formats
            naga::StorageFormat::R64Uint => wgpu::TextureFormat::R64Uint,
            naga::StorageFormat::Rg32Uint => wgpu::TextureFormat::Rg32Uint,
            naga::StorageFormat::Rg32Sint => wgpu::TextureFormat::Rg32Sint,
            naga::StorageFormat::Rg32Float => wgpu::TextureFormat::Rg32Float,
            naga::StorageFormat::Rgba16Uint => wgpu::TextureFormat::Rgba16Uint,
            naga::StorageFormat::Rgba16Sint => wgpu::TextureFormat::Rgba16Sint,
            naga::StorageFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,

            // 128-bit formats
            naga::StorageFormat::Rgba32Uint => wgpu::TextureFormat::Rgba32Uint,
            naga::StorageFormat::Rgba32Sint => wgpu::TextureFormat::Rgba32Sint,
            naga::StorageFormat::Rgba32Float => wgpu::TextureFormat::Rgba32Float,

            // Normalized 16-bit per channel formats
            naga::StorageFormat::R16Unorm => wgpu::TextureFormat::R16Unorm,
            naga::StorageFormat::R16Snorm => wgpu::TextureFormat::R16Snorm,
            naga::StorageFormat::Rg16Unorm => wgpu::TextureFormat::Rg16Unorm,
            naga::StorageFormat::Rg16Snorm => wgpu::TextureFormat::Rg16Snorm,
            naga::StorageFormat::Rgba16Unorm => wgpu::TextureFormat::Rgba16Unorm,
            naga::StorageFormat::Rgba16Snorm => wgpu::TextureFormat::Rgba16Snorm,
        }
    }

    fn texture_view_dimension(
        dimension: naga::ImageDimension,
        arrayed: bool,
    ) -> wgpu::TextureViewDimension {
        match dimension {
            naga::ImageDimension::D1 => wgpu::TextureViewDimension::D1,
            naga::ImageDimension::D2 => {
                if arrayed {
                    wgpu::TextureViewDimension::D2Array
                } else {
                    wgpu::TextureViewDimension::D2
                }
            }
            naga::ImageDimension::D3 => wgpu::TextureViewDimension::D3,
            naga::ImageDimension::Cube => {
                if arrayed {
                    wgpu::TextureViewDimension::CubeArray
                } else {
                    wgpu::TextureViewDimension::Cube
                }
            }
        }
    }

    fn binding_type(
        module: &naga::Module,
        variable: &naga::GlobalVariable,
        ty: &naga::Type,
    ) -> wgpu::BindingType {
        match &ty.inner {
            naga::TypeInner::Image {
                dim,
                class,
                arrayed,
            } => match class {
                naga::ImageClass::Storage { access, format } => wgpu::BindingType::StorageTexture {
                    access: if *access == naga::StorageAccess::all() {
                        wgpu::StorageTextureAccess::ReadWrite
                    } else if *access == naga::StorageAccess::STORE {
                        wgpu::StorageTextureAccess::WriteOnly
                    } else {
                        wgpu::StorageTextureAccess::ReadOnly
                    },
                    format: Self::texture_format(*format),
                    view_dimension: Self::texture_view_dimension(*dim, *arrayed),
                },
                _ => wgpu::BindingType::Texture {
                    sample_type: match class {
                        naga::ImageClass::Sampled { kind, .. } => match kind {
                            naga::ScalarKind::Uint => wgpu::TextureSampleType::Uint,
                            naga::ScalarKind::Sint => wgpu::TextureSampleType::Sint,
                            naga::ScalarKind::Float | naga::ScalarKind::AbstractFloat => {
                                wgpu::TextureSampleType::Float { filterable: true }
                            }
                            naga::ScalarKind::Bool | naga::ScalarKind::AbstractInt => {
                                wgpu::TextureSampleType::Uint
                            }
                        },
                        naga::ImageClass::Depth { .. } => wgpu::TextureSampleType::Depth,
                        _ => wgpu::TextureSampleType::Uint,
                    },
                    view_dimension: Self::texture_view_dimension(*dim, *arrayed),
                    multisampled: match class {
                        naga::ImageClass::Sampled { multi, .. } => *multi,
                        naga::ImageClass::Depth { multi, .. } => *multi,
                        _ => false,
                    },
                },
            },
            naga::TypeInner::Sampler { comparison } => wgpu::BindingType::Sampler(if *comparison {
                wgpu::SamplerBindingType::Comparison
            } else {
                wgpu::SamplerBindingType::Filtering
            }),
            naga::TypeInner::BindingArray { base, .. } => {
                Self::binding_type(module, variable, &module.types[*base])
            }
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
        module: &naga::Module,
        variable: &naga::GlobalVariable,
        ty: &naga::Type,
        binding: u32,
    ) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: match Self::shader_type(module) {
                ShaderType::Compute => wgpu::ShaderStages::COMPUTE,
                ShaderType::VertexFragment => wgpu::ShaderStages::VERTEX_FRAGMENT,
            },
            ty: Self::binding_type(module, variable, ty),
            count: match &ty.inner {
                naga::TypeInner::Array {
                    size: naga::ArraySize::Constant(size),
                    ..
                } => Some(*size),
                naga::TypeInner::BindingArray {
                    size: naga::ArraySize::Constant(size),
                    ..
                } => Some(*size),
                _ => None,
            },
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
            entries.push(Self::bind_group_layout_entry(
                module,
                variable,
                ty,
                binding.binding,
            ));
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
        let device = self.render_context.device();
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
        let device = self.render_context.device();
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(self.name.as_str()),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[mesh::Vertex::layout(wgpu::VertexStepMode::Vertex)],
                compilation_options: PipelineCompilationOptions {
                    constants: &Default::default(),
                    zero_initialize_workgroup_memory: false,
                },
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                targets: options.fragment_targets.as_slice(),
                compilation_options: PipelineCompilationOptions {
                    constants: &Default::default(),
                    zero_initialize_workgroup_memory: false,
                },
            }),
            primitive: wgpu::PrimitiveState {
                topology: options.primitive_topology,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: options.cull_mode,
                polygon_mode: options.polygon_mode,
                ..Default::default()
            },
            depth_stencil: options.depth_stencil.clone(),
            multisample: RenderUtils::multisample_default(options.samples),
            multiview: None,
            cache: None,
        });
        self.pipelines.insert(options.clone(), pipeline);
    }

    pub fn get_pipeline(&self, options: &PipelineOptions) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(options)
    }

    pub fn get_compute_pipeline(&self) -> Option<&wgpu::ComputePipeline> {
        self.compute_pipeline.as_ref()
    }
}
