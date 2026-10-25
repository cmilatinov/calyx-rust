use super::shader_preprocessor::ShaderPreprocessor;
use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, LoadedAsset};
use crate::context::ReadOnlyAssetContext;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, RenderContext};
use crate::utils::TypeUuid;
use eframe::wgpu::ShaderSource;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::PipelineCompilationOptions;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// Bind-group layout entries grouped by bind-group index.
pub type BindGroupEntries = BTreeMap<u32, Vec<wgpu::BindGroupLayoutEntry>>;
/// Ordered bind-group layouts created from shader reflection.
pub type BindGroupLayouts = Vec<wgpu::BindGroupLayout>;

/// High-level shader pipeline kind.
#[derive(PartialEq, Eq)]
pub enum ShaderType {
    /// Shader contains vertex and fragment entry points.
    VertexFragment,
    /// Shader contains a compute entry point.
    Compute,
}

/// Reflected WGSL shader asset with cached pipelines.
#[derive(TypeUuid)]
#[uuid = "00415831-a64c-4dc2-b573-5e112f99b674"]
pub struct Shader {
    pub(crate) render_context: Arc<RenderContext>,
    /// Shader stage category.
    pub ty: ShaderType,
    /// Human-readable shader name.
    pub name: String,
    /// Fully preprocessed WGSL source.
    pub source: String,
    /// Backing wgpu shader module.
    pub shader: wgpu::ShaderModule,
    /// Bind-group layouts derived from reflection.
    pub bind_group_layouts: BindGroupLayouts,
    /// Raw bind-group entries derived from reflection.
    pub bind_group_entries: BindGroupEntries,
    /// Pipeline layout built from `bind_group_layouts`.
    pub pipeline_layout: wgpu::PipelineLayout,
    /// Bind-group entries each compute entry point uses, by entry point
    /// name.
    compute_entry_points: HashMap<String, BindGroupEntries>,
    /// Compute pipelines built so far, by entry point name.
    compute_pipelines: Mutex<HashMap<String, ComputeKernel>>,
    /// Render pipelines built so far, keyed by pipeline options. Behind a
    /// mutex so [`Shader::pipeline`] can build through a read lock.
    pipelines: Mutex<HashMap<PipelineOptions, wgpu::RenderPipeline>>,
    /// Parsed Naga module used for reflection.
    pub module: naga::Module,
}

impl Asset for Shader {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Shader"
    }

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
        let source = ShaderPreprocessor::load_shader_source(&game.registries.assets.read(), path)
            .map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(err)
        })?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(
                path.file_stem().and_then(|f| f.to_str()).ok_or(
                    AssetError::LoadError
                        .with_path(path)
                        .with_type(Self::asset_name()),
                )?,
            ),
            source: ShaderSource::Wgsl(Cow::Borrowed(source.as_str())),
        });

        let module = naga::front::wgsl::parse_str(source.as_str()).map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(err.emit_to_string(source.as_str()))
        })?;

        let ty = Self::shader_type(&module);
        let bind_group_entries = Self::bind_group_entries(&module);
        let bind_group_layouts = Self::bind_group_layouts(device, &bind_group_entries);
        log::info!(
            "Loaded shader {} from {} with {} bind groups",
            name,
            path.display(),
            bind_group_entries.len()
        );

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: bind_group_layouts.iter().collect::<Vec<_>>().as_slice(),
            push_constant_ranges: &[],
        });

        let compute_entry_points = Self::compute_entry_points(&module, &bind_group_entries);

        Ok(LoadedAsset::new(Self {
            render_context,
            ty,
            name,
            source,
            shader,
            bind_group_layouts,
            bind_group_entries,
            pipeline_layout,
            compute_entry_points,
            compute_pipelines: Mutex::new(HashMap::new()),
            pipelines: Mutex::new(HashMap::new()),
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
            .values()
            .map(|entries| {
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: entries.as_slice(),
                })
            })
            .collect()
    }

    /// Rebuilds the shader pipeline layout from the current bind-group
    /// layouts.
    pub fn rebuild_pipeline_layout(&mut self) {
        let device = self.render_context.device();
        log::trace!("Rebuilding pipeline layout for shader {}", self.name);
        self.pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: self
                .bind_group_layouts
                .iter()
                .collect::<Vec<_>>()
                .as_slice(),
            push_constant_ranges: &[],
        });
        self.pipelines
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.compute_pipelines
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Returns the render pipeline for `options`, building and caching it on
    /// first use. The cache keys on every option, so a change of sample
    /// count, target format, depth state or vertex layout builds a new
    /// pipeline. A hot reload replaces the shader, which starts with an empty
    /// cache, so callers that ask every frame pick up the reloaded pipeline.
    pub fn pipeline(&self, options: &PipelineOptions) -> wgpu::RenderPipeline {
        let mut pipelines = self
            .pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(pipeline) = pipelines.get(options) {
            return pipeline.clone();
        }
        let pipeline = self.build_pipeline(options);
        pipelines.insert(options.clone(), pipeline.clone());
        pipeline
    }

    fn build_pipeline(&self, options: &PipelineOptions) -> wgpu::RenderPipeline {
        log::trace!(
            "Building render pipeline for shader {}: samples={}, targets={}",
            self.name,
            options.samples,
            options.fragment_targets.len()
        );
        let vertex_buffers = options
            .vertex_layouts
            .iter()
            .map(|layout| layout.as_wgpu())
            .collect::<Vec<_>>();
        let device = self.render_context.device();
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(self.name.as_str()),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: vertex_buffers.as_slice(),
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
        })
    }

    /// Returns how many render pipelines this shader has built.
    pub fn pipeline_count(&self) -> usize {
        self.pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Returns the compute pipeline of `entry_point` with the bind-group
    /// layouts it binds, building and caching it on first use, or `None`
    /// when the shader has no such compute entry point.
    ///
    /// In a shader with several compute entry points, the pipeline's layout
    /// holds only the bindings the entry point uses, so each kernel binds
    /// just its own resources. A shader with one compute entry point keeps
    /// its own layouts from [`Self::bind_group_layouts`]. A hot reload replaces the shader, which
    /// starts with an empty cache.
    pub fn compute_pipeline(&self, entry_point: &str) -> Option<ComputeKernel> {
        let mut pipelines = self
            .compute_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(kernel) = pipelines.get(entry_point) {
            return Some(kernel.clone());
        }
        let used = self.compute_entry_points.get(entry_point)?;
        let kernel = self.build_compute_pipeline(entry_point, used);
        pipelines.insert(entry_point.to_string(), kernel.clone());
        Some(kernel)
    }

    /// Returns how many compute pipelines this shader has built.
    pub fn compute_pipeline_count(&self) -> usize {
        self.compute_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn build_compute_pipeline(&self, entry_point: &str, used: &BindGroupEntries) -> ComputeKernel {
        log::trace!(
            "Building compute pipeline {entry_point} for shader {}",
            self.name
        );
        let device = self.render_context.device();
        let group_count = used.keys().next_back().map_or(0, |group| group + 1);
        let bind_group_layouts: Vec<wgpu::BindGroupLayout> = (0..group_count)
            .map(|group| {
                let entries = used.get(&group).map(Vec::as_slice).unwrap_or_default();
                let shader_group = self
                    .bind_group_entries
                    .keys()
                    .position(|shader_group| *shader_group == group);
                match shader_group {
                    Some(index) if self.bind_group_entries[&group].as_slice() == entries => {
                        self.bind_group_layouts[index].clone()
                    }
                    _ => device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some(entry_point),
                        entries,
                    }),
                }
            })
            .collect();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(entry_point),
            bind_group_layouts: bind_group_layouts.iter().collect::<Vec<_>>().as_slice(),
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry_point),
            layout: Some(&pipeline_layout),
            module: &self.shader,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            cache: None,
        });
        ComputeKernel {
            pipeline,
            bind_group_layouts,
        }
    }

    /// The bind-group entries each compute entry point of `module` binds.
    ///
    /// A shader with one compute entry point binds every resource it
    /// declares, through the shader's own layouts. With several, each binds
    /// only the resources it uses, as Naga's validation reports them; when
    /// the module does not validate, each binds every resource.
    fn compute_entry_points(
        module: &naga::Module,
        bind_group_entries: &BindGroupEntries,
    ) -> HashMap<String, BindGroupEntries> {
        let compute_entry_points = module
            .entry_points
            .iter()
            .filter(|entry_point| entry_point.stage == naga::ShaderStage::Compute)
            .count();
        let info = (compute_entry_points > 1)
            .then(|| {
                naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                )
                .validate(module)
                .map_err(|error| {
                    log::warn!("Shader validation failed, binding every resource: {error}")
                })
                .ok()
            })
            .flatten();
        module
            .entry_points
            .iter()
            .enumerate()
            .filter(|(_, entry_point)| entry_point.stage == naga::ShaderStage::Compute)
            .map(|(index, entry_point)| {
                let Some(info) = &info else {
                    return (entry_point.name.clone(), bind_group_entries.clone());
                };
                let function = info.get_entry_point(index);
                let mut used = BindGroupEntries::new();
                for (handle, variable) in module.global_variables.iter() {
                    let Some(binding) = &variable.binding else {
                        continue;
                    };
                    if function[handle].is_empty() {
                        continue;
                    }
                    let entry = bind_group_entries
                        .get(&binding.group)
                        .and_then(|entries| {
                            entries
                                .iter()
                                .find(|entry| entry.binding == binding.binding)
                        })
                        .cloned();
                    if let Some(entry) = entry {
                        used.entry(binding.group).or_default().push(entry);
                    }
                }
                (entry_point.name.clone(), used)
            })
            .collect()
    }
}

/// A compute pipeline with the layouts of the bind groups it binds, by group
/// index.
#[derive(Clone)]
pub struct ComputeKernel {
    /// The pipeline.
    pub pipeline: wgpu::ComputePipeline,
    /// Layout of each bind group the pipeline binds.
    pub bind_group_layouts: Vec<wgpu::BindGroupLayout>,
}

#[cfg(test)]
mod tests {
    use super::Shader;
    use crate::render::PARTICLE_KERNELS;
    use crate::test_support::TestAssets;

    fn bindings(shader: &Shader, entry_point: &str) -> Vec<(u32, u32)> {
        shader.compute_entry_points[entry_point]
            .iter()
            .flat_map(|(group, entries)| entries.iter().map(|entry| (*group, entry.binding)))
            .collect()
    }

    #[test]
    fn each_compute_entry_point_binds_only_what_it_uses_and_is_cached() {
        let assets = TestAssets::engine();
        let context = assets.asset_context().lock_read();
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>(PARTICLE_KERNELS)
            .expect("the particle kernels should load");
        let shader = shader.read();

        assert_eq!(bindings(&shader, "update_main"), [(0, 0), (0, 1), (0, 3)]);
        assert_eq!(
            bindings(&shader, "scatter_main"),
            [(0, 0), (0, 1), (0, 2), (0, 3)]
        );
        assert_eq!(bindings(&shader, "sort_main"), [(0, 4), (0, 6), (0, 9)]);

        let update = shader.compute_pipeline("update_main").unwrap();
        assert_eq!(update.bind_group_layouts.len(), 1);
        assert_ne!(update.bind_group_layouts[0], shader.bind_group_layouts[0]);
        assert_eq!(
            shader.compute_pipeline("update_main").unwrap().pipeline,
            update.pipeline
        );
        let sort = shader.compute_pipeline("sort_main").unwrap();
        assert_ne!(sort.pipeline, update.pipeline);
        assert!(shader.compute_pipeline("vs_main").is_none());
        assert_eq!(shader.compute_pipeline_count(), 2);
    }

    #[test]
    fn an_entry_point_using_every_binding_keeps_the_shader_layout() {
        let assets = TestAssets::engine();
        let context = assets.asset_context().lock_read();
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/mip_generator_cube")
            .expect("the mip generator should load");
        let shader = shader.read();
        let kernel = shader.compute_pipeline("compute_main").unwrap();
        assert_eq!(kernel.bind_group_layouts, shader.bind_group_layouts);
    }
}
