use super::shader_loader::AssetShaderLoader;
use crate as calyx_engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, LoadedAsset};
use crate::context::ReadOnlyAssetContext;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, RenderContext};
use crate::utils::TypeUuid;
use calyx_shader::Preprocessed;
use eframe::wgpu::ShaderSource;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::PipelineCompilationOptions;
use naga::front::wgsl::ParseError;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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
    /// Bind-group layouts derived from reflection. Empty for a shader with
    /// several compute entry points, whose kernels each use the layouts of
    /// [`Shader::compute_pipeline`].
    pub bind_group_layouts: BindGroupLayouts,
    /// Raw bind-group entries derived from reflection.
    pub bind_group_entries: BindGroupEntries,
    /// Pipeline layout built from `bind_group_layouts`.
    pub pipeline_layout: wgpu::PipelineLayout,
    /// Bind-group entries each compute entry point uses, by entry point
    /// name.
    compute_entry_points: HashMap<String, BindGroupEntries>,
    /// Compute pipelines built so far, by entry point name, or `None` for
    /// a kernel wgpu rejected.
    compute_pipelines: Mutex<HashMap<String, Option<ComputeKernel>>>,
    /// Render pipelines built so far, keyed by pipeline options, or `None`
    /// for options wgpu rejected. Behind a mutex so [`Shader::pipeline`] can
    /// build through a read lock.
    pipelines: Mutex<HashMap<PipelineOptions, Option<wgpu::RenderPipeline>>>,
    /// Render pipelines of the version this one hot-reloaded over, kept for
    /// options whose pipeline wgpu rejects in this version.
    previous_pipelines: HashMap<PipelineOptions, wgpu::RenderPipeline>,
    /// Compute pipelines of the version this one hot-reloaded over, kept for
    /// kernels wgpu rejects in this version.
    previous_compute_pipelines: HashMap<String, ComputeKernel>,
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
        // A writer that truncates the file before writing its new contents
        // leaves it empty for a moment. Empty WGSL parses, into a shader with
        // no entry points, so it is refused as a write in progress.
        let raw = std::fs::read(path).map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(err)
        })?;
        if raw.trim_ascii().is_empty() {
            return Err(AssetError::Incomplete
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source("the shader source is empty"));
        }
        let preprocessed = AssetShaderLoader::new(&game.registries.assets.read())
            .preprocess(path)
            .map_err(|err| {
                AssetError::LoadError
                    .with_path(path)
                    .with_type(Self::asset_name())
                    .with_source(err)
            })?;
        // Parsed first, so a shader naga rejects reports where in its files
        // and never reaches wgpu.
        let module = naga::front::wgsl::parse_str(&preprocessed.source).map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(Self::describe_parse_error(&err, &preprocessed))
        })?;
        let source = preprocessed.source;
        // wgpu validates the module and layouts further than naga's parser,
        // and its default handler panics on an error outside a scope.
        let scope = ValidationScope::push(device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name.as_str()),
            source: ShaderSource::Wgsl(Cow::Borrowed(source.as_str())),
        });

        let ty = Self::shader_type(&module);
        let bind_group_entries = Self::bind_group_entries(&module);
        // The kernels of a shader with several each bind only what they use,
        // through layouts of their own. All of the shader's bindings in one
        // layout could exceed a per-stage limit, such as eight storage
        // buffers, that no single kernel reaches, so such a shader keeps none.
        let kernels = module
            .entry_points
            .iter()
            .filter(|entry_point| entry_point.stage == naga::ShaderStage::Compute)
            .count();
        let bind_group_layouts = if kernels > 1 {
            BindGroupLayouts::new()
        } else {
            Self::bind_group_layouts(device, &bind_group_entries)
        };
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
        if let Some(error) = scope.pop() {
            return Err(AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(error));
        }

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
            previous_pipelines: HashMap::new(),
            previous_compute_pipelines: HashMap::new(),
            module,
        }))
    }

    /// Refuses a version that lacks an entry point the live shader has, or
    /// whose bind-group entries differ from the live shader's: renderers
    /// draw through the live entry points with bind groups built from the
    /// live layouts. An accepted version keeps the live pipelines, for
    /// options whose pipeline wgpu rejects in it.
    fn accept_reload(&mut self, previous: &Self) -> Result<(), AssetError> {
        if let Some(change) = Self::incompatible_change(previous, self) {
            return Err(AssetError::LoadError
                .with_type(Self::asset_name())
                .with_source(format!(
                    "{change}, so the reload is refused and shader {} keeps running; \
                     a change to its entry points or bind-group layout takes effect \
                     when the editor restarts",
                    previous.name
                )));
        }
        let pipelines = previous
            .pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.previous_pipelines = previous.previous_pipelines.clone();
        self.previous_pipelines.extend(
            pipelines
                .into_iter()
                .filter_map(|(options, pipeline)| Some((options, pipeline?))),
        );
        let kernels = previous
            .compute_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.previous_compute_pipelines = previous.previous_compute_pipelines.clone();
        self.previous_compute_pipelines.extend(
            kernels
                .into_iter()
                .filter_map(|(entry_point, kernel)| Some((entry_point, kernel?))),
        );
        Ok(())
    }
}

/// A wgpu validation error scope on a device, so an error is returned
/// rather than reaching the device's default handler, which panics. The
/// scope is popped when the guard is dropped, so an early return or a panic
/// between push and pop never leaves it on the device's stack.
///
/// wgpu keeps one error-scope stack per device, shared by every thread, so a
/// scope also holds [`SCOPE_LOCK`] from push to pop. Without it, scopes opened
/// on two threads at once could pop each other's errors. Scopes do not nest.
struct ValidationScope<'a> {
    device: Option<&'a wgpu::Device>,
    // Released after `drop` pops the scope.
    _lock: MutexGuard<'static, ()>,
}

static SCOPE_LOCK: Mutex<()> = Mutex::new(());

impl<'a> ValidationScope<'a> {
    fn push(device: &'a wgpu::Device) -> Self {
        let lock = SCOPE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        Self {
            device: Some(device),
            _lock: lock,
        }
    }

    /// Pops the scope, returning the first validation error raised in it.
    fn pop(mut self) -> Option<wgpu::Error> {
        Self::pop_from(self.device.take()?)
    }

    fn pop_from(device: &wgpu::Device) -> Option<wgpu::Error> {
        pollster::block_on(device.pop_error_scope())
    }
}

impl Drop for ValidationScope<'_> {
    /// Pops a scope that was not popped explicitly, logging its error.
    fn drop(&mut self) {
        if let Some(error) = self.device.take().and_then(Self::pop_from) {
            log::error!("wgpu validation error in an abandoned error scope: {error}");
        }
    }
}

impl Shader {
    /// The first difference between `live` and `reloaded` that a running
    /// renderer cannot follow: an entry point `reloaded` lacks, or a changed
    /// bind-group entry.
    fn incompatible_change(live: &Self, reloaded: &Self) -> Option<String> {
        for entry_point in &live.module.entry_points {
            let kept =
                reloaded.module.entry_points.iter().any(|other| {
                    other.name == entry_point.name && other.stage == entry_point.stage
                });
            if !kept {
                return Some(format!(
                    "the reloaded shader has no {:?} entry point `{}`",
                    entry_point.stage, entry_point.name
                ));
            }
        }
        let bindings = |groups: &BindGroupEntries| {
            groups
                .iter()
                .flat_map(|(group, entries)| {
                    entries
                        .iter()
                        .map(move |entry| ((*group, entry.binding), *entry))
                })
                .collect::<BTreeMap<_, _>>()
        };
        let live = bindings(&live.bind_group_entries);
        let reloaded = bindings(&reloaded.bind_group_entries);
        let keys = live.keys().chain(reloaded.keys()).collect::<BTreeSet<_>>();
        for key in keys {
            let change = match (live.get(key), reloaded.get(key)) {
                (Some(_), None) => "was removed",
                (None, Some(_)) => "was added",
                (Some(before), Some(after)) if before != after => "changed",
                _ => continue,
            };
            let (group, binding) = key;
            return Some(format!("group {group} binding {binding} {change}"));
        }
        None
    }

    /// Describes naga's `error` in `shader`'s source at the files and lines
    /// it came from, rather than the lines of the expanded source. A column
    /// is the expanded line's, which a parameter's replacement can shift.
    fn describe_parse_error(error: &ParseError, shader: &Preprocessed) -> String {
        let source = shader.source.as_str();
        let mut lines = Vec::new();
        for (span, label) in error.labels() {
            if !span.is_defined() {
                continue;
            }
            let location = span.location(source);
            let line = location.line_number as usize;
            let place = match shader.source_map.location(line) {
                Some(original) => format!("{}:{}", original.file.display(), original.line),
                None => format!("line {line} of the preprocessed source"),
            };
            let place = format!("{place}:{}", location.line_position);
            if lines.is_empty() {
                lines.push(format!("{place}: {}", error.message()));
            } else {
                lines.push(place);
            }
            let code = source.lines().nth(line - 1).unwrap_or_default();
            let column = location.line_position as usize - 1;
            let indent = code
                .get(..column)
                .map_or(column, |prefix| prefix.chars().count());
            let width = (location.length as usize).max(1);
            lines.push(format!("    {code}"));
            lines.push(format!(
                "    {}{} {label}",
                " ".repeat(indent),
                "^".repeat(width)
            ));
        }
        if lines.is_empty() {
            lines.push(error.message().to_owned());
        }
        // naga keeps an error's notes private; they end its own rendering.
        for note in error
            .emit_to_string(source)
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("= "))
        {
            lines.push(format!("  = {note}"));
        }
        lines.join("\n")
    }

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
        self.previous_pipelines.clear();
        self.previous_compute_pipelines.clear();
    }

    /// Returns the render pipeline for `options`, building and caching it on
    /// first use. The cache keys on every option, so a change of sample
    /// count, target format, depth state or vertex layout builds a new
    /// pipeline. A hot reload replaces the shader, which starts with an empty
    /// cache, so callers that ask every frame pick up the reloaded pipeline.
    ///
    /// When wgpu rejects the pipeline, the error is logged once and the
    /// shader keeps the pipeline the version it reloaded over built for
    /// `options`, or returns `None` when it has none, and the caller skips
    /// the draw.
    pub fn pipeline(&self, options: &PipelineOptions) -> Option<wgpu::RenderPipeline> {
        let mut pipelines = self
            .pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(pipeline) = pipelines.get(options) {
            return pipeline.clone();
        }
        let scope = ValidationScope::push(self.render_context.device());
        let pipeline = self.build_pipeline(options);
        let pipeline = match scope.pop() {
            None => Some(pipeline),
            Some(error) => {
                let previous = self.previous_pipelines.get(options).cloned();
                log::error!(
                    "wgpu rejected a render pipeline of shader {}, {}: {error}",
                    self.name,
                    if previous.is_some() {
                        "keeping the one it built before"
                    } else {
                        "skipping its draws"
                    }
                );
                previous
            }
        };
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
            .values()
            .flatten()
            .count()
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
    ///
    /// When wgpu rejects the pipeline, the error is logged once and the
    /// shader keeps the kernel the version it reloaded over built, or
    /// returns `None` when it has none.
    pub fn compute_pipeline(&self, entry_point: &str) -> Option<ComputeKernel> {
        let mut pipelines = self
            .compute_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(kernel) = pipelines.get(entry_point) {
            return kernel.clone();
        }
        let used = self.compute_entry_points.get(entry_point)?;
        let scope = ValidationScope::push(self.render_context.device());
        let kernel = self.build_compute_pipeline(entry_point, used);
        let kernel = match scope.pop() {
            None => Some(kernel),
            Some(error) => {
                let previous = self.previous_compute_pipelines.get(entry_point).cloned();
                log::error!(
                    "wgpu rejected compute pipeline {entry_point} of shader {}, {}: {error}",
                    self.name,
                    if previous.is_some() {
                        "keeping the one it built before"
                    } else {
                        "skipping its dispatches"
                    }
                );
                previous
            }
        };
        pipelines.insert(entry_point.to_string(), kernel.clone());
        kernel
    }

    /// Returns how many compute pipelines this shader has built.
    pub fn compute_pipeline_count(&self) -> usize {
        self.compute_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .flatten()
            .count()
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
                    Some(index)
                        if index < self.bind_group_layouts.len()
                            && self.bind_group_entries[&group].as_slice() == entries =>
                    {
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
    use crate::assets::Asset;
    use crate::render::AssetShaderLoader;
    use crate::render::PARTICLE_KERNELS;
    use crate::test_support::TestAssets;
    use calyx_shader::MemoryLoader;
    use std::collections::HashSet;
    use std::path::Path;

    /// naga reports a parse error in the expanded source; the shader's
    /// error names the included module's file and line instead.
    #[test]
    fn a_parse_error_in_an_included_module_names_its_file_and_line() {
        let loader = MemoryLoader::new()
            .with(
                "shaders/module.wgsl",
                "// A module.\n//#param keys: storage<array<f32>>\n\n\
                 fn key(index: u32) -> f32 {\n    return #keys[index] +;\n}\n",
            )
            .with(
                "main.wgsl",
                "// The shader.\n//#include \"shaders/module.wgsl\"\n\
                 //#binding keys\n\
                 @group(0) @binding(0) var<storage> particle_keys: array<f32>;\n",
            );
        let shader = calyx_shader::preprocess(&loader, Path::new("main.wgsl"))
            .expect("the shader preprocesses");
        let error = naga::front::wgsl::parse_str(&shader.source).expect_err("naga rejects it");
        let expanded = error
            .location(&shader.source)
            .expect("the error has a location")
            .line_number;
        assert_eq!(expanded, 6, "{}", shader.source);

        let description = Shader::describe_parse_error(&error, &shader);
        let first = description.lines().next().unwrap_or_default();
        assert!(first.starts_with("shaders/module.wgsl:5:"), "{description}");
        assert!(first.contains(error.message()), "{description}");
        assert!(
            description.contains("return particle_keys[index] +;"),
            "{description}"
        );
        assert!(!description.contains("main.wgsl"), "{description}");
    }

    fn bindings(shader: &Shader, entry_point: &str) -> Vec<(u32, u32)> {
        shader.compute_entry_points[entry_point]
            .iter()
            .flat_map(|(group, entries)| entries.iter().map(|entry| (*group, entry.binding)))
            .collect()
    }

    fn wgsl_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("the asset dir is readable") {
            let path = entry.expect("the asset dir lists").path();
            if path.is_dir() {
                wgsl_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "wgsl") {
                files.push(path);
            }
        }
    }

    /// Every shader of the engine's and the sandbox's assets loads. A file
    /// another file includes is a module, such as `curve.wgsl` with its
    /// parameters or `pbr_utils.wgsl` with the constants of the shader
    /// that includes it, and loads through the shaders that include it.
    #[test]
    fn every_shader_of_the_assets_loads() {
        let assets = TestAssets::game();
        let context = assets.asset_context().lock_read();
        let mut files = Vec::new();
        for root in assets.roots() {
            wgsl_files(&root, &mut files);
        }
        let files: Vec<_> = files
            .iter()
            .map(|path| std::fs::canonicalize(path).expect("the shader exists"))
            .collect();
        let mut modules = HashSet::new();
        {
            let registry = context.registries.assets.read();
            let loader = AssetShaderLoader::new(&registry);
            for path in &files {
                let dependencies = loader
                    .dependencies(path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                modules.extend(dependencies);
            }
        }
        let shaders: Vec<_> = files
            .iter()
            .filter(|path| !modules.contains(*path))
            .collect();
        for path in &shaders {
            if let Err(error) = Shader::from_file(&context, path) {
                panic!("{} does not load: {error}", path.display());
            }
        }
        assert!(
            modules.iter().any(|path| path.ends_with("curve.wgsl")),
            "{modules:?}"
        );
        assert!(shaders.len() > 10, "only {} shaders", shaders.len());
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
        assert!(shader.bind_group_layouts.is_empty());
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
            .load::<Shader>("shaders/utility/mip_generator_cube")
            .expect("the mip generator should load");
        let shader = shader.read();
        let kernel = shader.compute_pipeline("compute_main").unwrap();
        assert_eq!(kernel.bind_group_layouts, shader.bind_group_layouts);
    }
}

#[cfg(test)]
mod kernel_limit_tests {
    use super::Shader;
    use crate::assets::Asset;
    use crate::test_support::TestAssets;

    /// Two kernels that use five storage buffers each, nine between them:
    /// within the per-stage limit of eight for each kernel, over it for the
    /// shader as a whole.
    const KERNELS: &str = r#"
@group(0) @binding(0) var<storage, read_write> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read> c: array<f32>;
@group(0) @binding(3) var<storage, read> d: array<f32>;
@group(0) @binding(4) var<storage, read> e: array<f32>;
@group(0) @binding(5) var<storage, read> f: array<f32>;
@group(0) @binding(6) var<storage, read> g: array<f32>;
@group(0) @binding(7) var<storage, read> h: array<f32>;
@group(0) @binding(8) var<storage, read> i: array<f32>;

@compute @workgroup_size(1)
fn first(@builtin(global_invocation_id) gid: vec3u) {
    a[gid.x] = b[gid.x] + c[gid.x] + d[gid.x] + e[gid.x];
}

@compute @workgroup_size(1)
fn second(@builtin(global_invocation_id) gid: vec3u) {
    a[gid.x] = f[gid.x] + g[gid.x] + h[gid.x] + i[gid.x];
}
"#;

    #[test]
    fn kernels_within_the_storage_limit_load_whatever_the_shader_declares() {
        let assets = TestAssets::engine();
        let path = assets.engine_root().join("nine_buffers.wgsl");
        std::fs::write(&path, KERNELS).expect("the shader is writable");
        let context = assets.asset_context().lock_read();
        let shader = Shader::from_file(&context, &path)
            .expect("the shader loads")
            .asset;
        for kernel in ["first", "second"] {
            let kernel = shader.compute_pipeline(kernel).expect("a kernel");
            assert_eq!(kernel.bind_group_layouts.len(), 1);
        }
        assert!(
            shader.bind_group_layouts.is_empty(),
            "a shader of several kernels keeps no layout of its own"
        );
    }
}

#[cfg(test)]
mod hot_reload_tests {
    use super::Shader;
    use crate::render::{PipelineOptions, RenderUtils};
    use crate::test_support::TestAssets;
    use egui_wgpu::wgpu;
    use notify::{Event, EventKind};
    use std::path::Path;
    use std::time::Duration;

    const NAME: &str = "reload_probe";

    /// A full-screen shader with one uniform; `color` is its fragment color.
    fn shader_source(color: &str) -> String {
        format!(
            r#"
struct Tint {{ color: vec4f }}
@group(0) @binding(0) var<uniform> tint: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {{
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}}

@fragment
fn fs_main() -> @location(0) vec4f {{
    return tint.color * {color};
}}
"#
        )
    }

    const NO_FRAGMENT: &str = r#"
struct Tint { color: vec4f }
@group(0) @binding(0) var<uniform> tint: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    return tint.color;
}
"#;

    const MOVED_BINDING: &str = r#"
struct Tint { color: vec4f }
@group(0) @binding(1) var<uniform> tint: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    return vec4f(0.0, 0.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return tint.color;
}
"#;

    /// Same entry points and bindings, but the vertex stage reads a vertex
    /// attribute that a full-screen draw has no buffer for, so wgpu rejects
    /// its pipeline.
    const NEEDS_A_VERTEX_BUFFER: &str = r#"
struct Tint { color: vec4f }
@group(0) @binding(0) var<uniform> tint: Tint;

@vertex
fn vs_main(@location(0) position: vec3f) -> @builtin(position) vec4f {
    return vec4f(position, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return tint.color;
}
"#;

    fn options() -> PipelineOptions {
        PipelineOptions::fullscreen_builder()
            .target(RenderUtils::color_alpha_blending(
                wgpu::TextureFormat::Rgba8Unorm,
            ))
            .build()
    }

    fn write_and_reload(assets: &crate::context::ReadOnlyAssetContext, path: &Path, source: &str) {
        std::fs::write(path, source).expect("the shader is writable");
        let registry = assets.registries.assets.read();
        registry.recv_notify_event(
            Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                .add_path(path.to_path_buf()),
        );
        std::thread::sleep(Duration::from_millis(200));
        registry.reload_assets();
    }

    fn reload_error_messages(assets: &crate::context::ReadOnlyAssetContext) -> Vec<String> {
        assets
            .registries
            .assets
            .read()
            .reload_errors()
            .iter()
            .map(|error| error.error.to_string())
            .collect()
    }

    fn entry_points(shader: &Shader) -> Vec<String> {
        shader
            .module
            .entry_points
            .iter()
            .map(|entry_point| entry_point.name.clone())
            .collect()
    }

    /// An empty write, a version without `fs_main` and a version with a
    /// moved binding are each refused and keep the running shader; a valid
    /// edit that keeps the layout reloads.
    #[test]
    fn a_reload_that_would_break_the_running_shader_keeps_it() {
        let assets = TestAssets::engine();
        let path = assets.engine_root().join(format!("{NAME}.wgsl"));
        std::fs::write(&path, shader_source("1.0")).expect("the shader is writable");
        let context = assets.asset_context();
        let context = context.lock_read();
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>(NAME)
            .expect("the shader loads");
        assert!(shader.read().pipeline(&options()).is_some());
        let expected = vec!["vs_main".to_string(), "fs_main".to_string()];

        write_and_reload(&context, &path, "");
        assert_eq!(entry_points(&shader.read()), expected);
        assert!(shader.read().pipeline(&options()).is_some());
        assert!(
            reload_error_messages(&context).is_empty(),
            "an empty write is in progress"
        );

        write_and_reload(&context, &path, NO_FRAGMENT);
        assert_eq!(entry_points(&shader.read()), expected);
        assert!(shader.read().pipeline(&options()).is_some());
        let errors = reload_error_messages(&context);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("fs_main"), "{errors:?}");
        assert!(errors[0].contains("restart"), "{errors:?}");

        write_and_reload(&context, &path, MOVED_BINDING);
        assert_eq!(entry_points(&shader.read()), expected);
        assert!(shader.read().pipeline(&options()).is_some());
        let errors = reload_error_messages(&context);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("group 0 binding"), "{errors:?}");

        write_and_reload(&context, &path, &shader_source("0.5"));
        assert!(shader.read().source.contains("0.5"));
        assert!(shader.read().pipeline(&options()).is_some());
        assert!(reload_error_messages(&context).is_empty());
    }

    /// A reload wgpu rejects the pipeline of keeps drawing with the pipeline
    /// the shader built before, and a shader with none skips the draw.
    #[test]
    fn a_pipeline_wgpu_rejects_keeps_the_previous_one_or_none() {
        let assets = TestAssets::engine();
        let path = assets.engine_root().join(format!("{NAME}.wgsl"));
        std::fs::write(&path, shader_source("1.0")).expect("the shader is writable");
        let context = assets.asset_context();
        let context = context.lock_read();
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>(NAME)
            .expect("the shader loads");
        let before = shader.read().pipeline(&options()).expect("a pipeline");

        write_and_reload(&context, &path, NEEDS_A_VERTEX_BUFFER);
        assert!(reload_error_messages(&context).is_empty());
        assert_eq!(shader.read().pipeline(&options()), Some(before));

        let fresh = assets.engine_root().join("fresh_probe.wgsl");
        std::fs::write(&fresh, NEEDS_A_VERTEX_BUFFER).expect("the shader is writable");
        use crate::assets::Asset;
        let fresh = Shader::from_file(&context, &fresh)
            .expect("the shader loads")
            .asset;
        assert!(fresh.pipeline(&options()).is_none());
    }

    const INVALID: &str = "fn main() { let x: u32 = 1.0; }";
    const VALID: &str = "@compute @workgroup_size(1) fn main() {}";

    fn create_module(device: &wgpu::Device, source: &'static str) {
        let _module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scope_probe"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    }

    /// A scope dropped without `pop` is popped by its guard, taking its
    /// error with it, so the scope pushed before it is the one popped next.
    #[test]
    fn a_dropped_validation_scope_pops_itself() {
        let render_context = crate::test_support::test_render_context();
        let device = render_context.device();
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        {
            let _scope = super::ValidationScope::push(device);
            create_module(device, INVALID);
        }
        assert!(pollster::block_on(device.pop_error_scope()).is_none());
    }

    /// Scopes opened on two threads at once each see only their own errors.
    #[test]
    fn validation_scopes_on_two_threads_keep_their_own_errors() {
        let render_context = crate::test_support::test_render_context();
        let device = render_context.device();
        std::thread::scope(|threads| {
            let invalid = threads.spawn(|| {
                (0..200).all(|_| {
                    let scope = super::ValidationScope::push(device);
                    create_module(device, INVALID);
                    scope.pop().is_some()
                })
            });
            let valid = threads.spawn(|| {
                (0..200).all(|_| {
                    let scope = super::ValidationScope::push(device);
                    create_module(device, VALID);
                    scope.pop().is_none()
                })
            });
            assert!(
                invalid.join().unwrap(),
                "an invalid module's error was lost"
            );
            assert!(valid.join().unwrap(), "a valid module got another's error");
        });
    }
}
