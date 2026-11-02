use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::io::BufReader;
use std::path::Path;

use super::{AssetAccess, AssetRef, LoadedAsset};
use crate as calyx_engine;
use crate::assets::error::AssetError;
use crate::assets::import::ImportedMaterial;
use crate::assets::texture::{Texture, TextureColorSpace};
use crate::assets::{Asset, AssetId};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{LockedAssetRenderState, RenderContext, Shader, FIRST_MATERIAL_GROUP};
use crate::utils::TypeUuid;
use egui_wgpu::wgpu;
use naga::{ImageDimension, Scalar, ScalarKind, TypeInner, VectorSize};
use serde::{Deserialize, Serialize};

/// High-level categories of shader bindings exposed through materials.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindingType {
    /// Uniform or storage buffer binding.
    Buffer,
    /// Sampler binding.
    Sampler,
    /// Sampled texture binding.
    Texture,
}

/// One editable shader variable exposed by a material asset.
#[derive(Serialize, Deserialize)]
pub struct ShaderVariable {
    /// Bind group index.
    pub group: u32,
    /// Binding slot inside the bind group.
    pub binding: u32,
    /// Optional byte offset for struct members packed into a buffer binding.
    pub offset: Option<u32>,
    /// Display name for the variable.
    pub name: String,
    /// Optional byte size for buffer-backed variables.
    pub span: Option<u32>,
    /// Stored runtime/editor value.
    pub value: ShaderVariableValue,
}

impl PartialEq for ShaderVariable {
    fn eq(&self, other: &Self) -> bool {
        self.group == other.group && self.binding == other.binding && self.offset == other.offset
    }
}

impl Eq for ShaderVariable {}

impl PartialOrd for ShaderVariable {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ShaderVariable {
    fn cmp(&self, other: &Self) -> Ordering {
        self.group
            .cmp(&other.group)
            .then_with(|| self.binding.cmp(&other.binding))
            .then_with(|| self.offset.cmp(&other.offset))
    }
}

/// Texture source stored by material texture slots.
///
/// The slot decides how the texture's channels are read, not the source: a slot whose shader
/// variable names a color, base color or emissive, samples an image as sRGB, and every other
/// slot samples it as linear (see [`Material::texture_color_space`]). A [`Self::Color`] holds
/// linear values and samples as given in any slot.
#[derive(Clone, Serialize, Deserialize)]
pub enum MaterialTexture {
    /// Sample from a texture asset on disk.
    Asset(AssetRef<Texture>),
    /// Sample from a generated in-memory 1x1 color texture.
    Color([f32; 4]),
}

impl Default for MaterialTexture {
    fn default() -> Self {
        Self::Color([1.0, 1.0, 1.0, 1.0])
    }
}

impl PartialEq for MaterialTexture {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Asset(left), Self::Asset(right)) => left.id() == right.id(),
            (Self::Color(left), Self::Color(right)) => left == right,
            _ => false,
        }
    }
}

impl MaterialTexture {
    /// Quantizes linear RGBA floats into a stable 8-bit texture cache key.
    pub fn color_key(color: [f32; 4]) -> [u8; 4] {
        color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
    }

    fn as_texture(&self, context: &ReadOnlyAssetContext, default: Ref<Texture>) -> Ref<Texture> {
        match self {
            Self::Asset(texture) => texture.get_ref(&context.registries).unwrap_or(default),
            Self::Color(color) => context.registries.assets.read().color_texture_2d(*color),
        }
    }
}

/// Editable value payload for a [`ShaderVariable`].
#[derive(Serialize, Deserialize)]
pub enum ShaderVariableValue {
    /// Signed integer value.
    Int(i32),
    /// Unsigned integer value.
    Uint(u32),
    /// Floating-point value.
    Float(f32),
    /// Boolean value.
    Bool(bool),
    /// RGBA color written to shader buffers as four 32-bit floats.
    Color([f32; 4]),
    /// 2D vector.
    Vec2([f32; 2]),
    /// 3D vector.
    Vec3([f32; 3]),
    /// 4D vector.
    Vec4([f32; 4]),
    /// 4x4 matrix.
    Mat4([[f32; 4]; 4]),
    /// 2D texture source.
    Texture2D(MaterialTexture),
    /// Texture sampler slot.
    Sampler,
}

impl ShaderVariableValue {
    /// Returns the bindable resource category for this value.
    pub fn binding_type(&self) -> BindingType {
        match self {
            ShaderVariableValue::Sampler => BindingType::Sampler,
            ShaderVariableValue::Texture2D(_) => BindingType::Texture,
            _ => BindingType::Buffer,
        }
    }

    /// Returns the raw bytes written into a buffer binding for this value.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            ShaderVariableValue::Int(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Uint(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Float(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Bool(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Color(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Vec2(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Vec3(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Vec4(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Mat4(value) => bytemuck::cast_slice(std::slice::from_ref(value)),
            ShaderVariableValue::Texture2D(_) => &[],
            ShaderVariableValue::Sampler => &[],
        }
    }

    /// Resolves the texture referenced by this value or returns `default`.
    pub fn as_texture(
        &self,
        context: &ReadOnlyAssetContext,
        default: Ref<Texture>,
    ) -> Ref<Texture> {
        let ShaderVariableValue::Texture2D(texture) = self else {
            return default;
        };
        texture.as_texture(context, default)
    }
}

/// Material asset that binds a shader and its editable variables.
///
/// A material file stores the variables it had when it was saved. When it loads, every
/// variable its shader declares that the file lacks is added with its reflected default (see
/// [`Material::from_shader`]), so a material saved before its shader gained an input renders
/// as it did then.
#[derive(TypeUuid, Serialize)]
#[uuid = "f98a7f41-84d4-482d-b7af-a670b07035ae"]
pub struct Material {
    /// Shader asset used by this material.
    pub shader: AssetRef<Shader>,
    /// Editable shader variables.
    pub variables: Vec<ShaderVariable>,
    #[serde(skip)]
    variable_indices: HashMap<(u32, u32), usize>,
    /// Color space of each texture slot, keyed by group and binding, from the shader's
    /// variable names.
    #[serde(skip)]
    texture_color_spaces: HashMap<(u32, u32), TextureColorSpace>,
    #[serde(skip)]
    /// Cached bind-group entry metadata keyed by group and binding.
    pub bind_group_entries: BTreeMap<u32, BTreeMap<u32, BindGroupEntry>>,
    #[serde(skip)]
    /// GPU buffers created for buffer-backed shader variables.
    pub buffers: HashMap<(u32, u32), wgpu::Buffer>,
    /// The [`Shader::layout_generation`] the variables, bind-group entries and buffers were
    /// built for. [`Material::follow_shader`] rebuilds them when the shader's differs.
    #[serde(skip)]
    shader_layout_generation: Option<u64>,
}

/// Cached metadata for one bind-group entry.
pub struct BindGroupEntry {
    ty: BindingType,
    size: Option<u32>,
}

/// What a material's bind groups bind. While it compares equal across frames,
/// the renderer reuses the bind groups it built for the material.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct MaterialBindGroupCacheKey {
    shader_id: AssetId,
    /// The shader layouts the bind groups are built for.
    shader_layout_generation: Option<u64>,
    entries: Vec<MaterialBindGroupCacheEntry>,
}

#[derive(Clone, PartialEq, Eq)]
struct MaterialBindGroupCacheEntry {
    group: u32,
    binding: u32,
    value: MaterialBindGroupCacheValue,
}

#[derive(Clone, PartialEq, Eq)]
enum MaterialBindGroupCacheValue {
    /// The buffer bound, compared by identity: a hot reload replaces the
    /// material's buffers with new ones of the same size, and a bind group
    /// that kept the old buffer would never see the material's values again.
    Buffer {
        buffer: Option<wgpu::Buffer>,
    },
    Texture {
        texture_id: AssetId,
        color_space: TextureColorSpace,
    },
    Sampler {
        texture_id: AssetId,
    },
}

impl Asset for Material {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Material"
    }

    fn file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxmat"]
    }

    fn from_file(
        assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|err| {
                AssetError::LoadError
                    .with_path(path)
                    .with_type(Self::asset_name())
                    .with_source(err)
            })?;
        let reader = BufReader::new(file);
        let data: MaterialData = serde_json::from_reader(reader).map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(err)
        })?;
        let material: Material = (assets, data).into();
        log::info!(
            "Loaded material {} with {} shader variables",
            path.display(),
            material.variables.len()
        );
        Ok(LoadedAsset::new(material))
    }
}

impl Material {
    /// Builds a default material from a shader asset by reflecting its exposed
    /// variables. Each variable takes its reflected default: zero, except white for a color
    /// and for a texture, the flat normal for a normal texture, and one for the multipliers
    /// `normal_scale` and `occlusion_strength`.
    pub fn from_shader(assets: &ReadOnlyAssetContext, shader_ref: Ref<Shader>) -> Self {
        let mut material = Self {
            shader: Some(shader_ref).into(),
            variables: Default::default(),
            variable_indices: Default::default(),
            texture_color_spaces: Default::default(),
            bind_group_entries: Default::default(),
            buffers: Default::default(),
            shader_layout_generation: None,
        };
        material.init(assets);
        material
    }

    /// Builds a material on `shader`, the PBR scene shader, from an imported glTF-style
    /// material. `textures` holds the texture asset id of each imported texture, or `None` for
    /// one that did not upload; a slot naming none keeps the shader's default. The ambient
    /// occlusion multiplier is one, so only the occlusion texture darkens the ambient light.
    pub(crate) fn from_imported(
        assets: &ReadOnlyAssetContext,
        shader: Ref<Shader>,
        imported: &ImportedMaterial,
        textures: &[Option<AssetId>],
    ) -> Self {
        let mut material = Self::from_shader(assets, shader);
        let texture = |slot: Option<usize>| {
            slot.and_then(|index| textures.get(index).copied().flatten())
                .map(|id| {
                    ShaderVariableValue::Texture2D(MaterialTexture::Asset(AssetRef::from_id(id)))
                })
        };
        let [r, g, b] = imported.emissive_factor;
        let values = [
            (
                "base_color_factor",
                Some(ShaderVariableValue::Color(imported.base_color_factor)),
            ),
            ("base_color_texture", texture(imported.base_color_texture)),
            (
                "metallic",
                Some(ShaderVariableValue::Float(imported.metallic_factor)),
            ),
            (
                "roughness",
                Some(ShaderVariableValue::Float(imported.roughness_factor)),
            ),
            (
                "metallic_roughness_texture",
                texture(imported.metallic_roughness_texture),
            ),
            ("normal_texture", texture(imported.normal_texture)),
            (
                "normal_scale",
                Some(ShaderVariableValue::Float(imported.normal_scale)),
            ),
            (
                "emission_color",
                Some(ShaderVariableValue::Color([r, g, b, 1.0])),
            ),
            ("emission_strength", Some(ShaderVariableValue::Float(1.0))),
            ("emissive_texture", texture(imported.emissive_texture)),
            ("occlusion_texture", texture(imported.occlusion_texture)),
            (
                "occlusion_strength",
                Some(ShaderVariableValue::Float(imported.occlusion_strength)),
            ),
            ("ambient_occlusion", Some(ShaderVariableValue::Float(1.0))),
            (
                "alpha_cutoff",
                Some(ShaderVariableValue::Float(
                    imported.alpha_cutoff.unwrap_or(0.0),
                )),
            ),
        ];
        for (name, value) in values {
            if let Some(value) = value {
                if !material.set_variable(name, value) {
                    log::warn!("The PBR shader has no {name} input for imported materials");
                }
            }
        }
        material
    }

    /// The editable variables `module` declares, each with its reflected default.
    fn reflect_variables(module: &naga::Module) -> Vec<ShaderVariable> {
        let mut variables = Vec::new();
        for (_, variable) in module.global_variables.iter() {
            if let Some(binding) = &variable.binding {
                let ty = &module.types[variable.ty];
                if binding.group >= FIRST_MATERIAL_GROUP {
                    Self::shader_variable(
                        module,
                        ty,
                        binding,
                        variable.name.clone().unwrap_or_default(),
                        None,
                        &mut variables,
                    );
                }
            }
        }
        variables
    }

    /// Whether a material on a shader of `module` has a variable for `group` `binding`, so it
    /// can fill that binding: a uniform of a scalar, vector or 4x4 matrix, or a struct with at
    /// least one of those, a 2D texture or a sampler.
    /// Unlike the variables a material takes, which start at [`FIRST_MATERIAL_GROUP`], `group`
    /// may be any group, such as a pass's group that binds a material's bind group.
    pub(crate) fn reflects_binding(module: &naga::Module, group: u32, binding: u32) -> bool {
        let mut variables = Vec::new();
        for (_, variable) in module.global_variables.iter() {
            if let Some(resource) = &variable.binding {
                if (resource.group, resource.binding) == (group, binding) {
                    Self::shader_variable(
                        module,
                        &module.types[variable.ty],
                        resource,
                        variable.name.clone().unwrap_or_default(),
                        None,
                        &mut variables,
                    );
                }
            }
        }
        !variables.is_empty()
    }

    /// Adds each variable the shader declares that the material lacks, with its reflected
    /// default, keeps the variables in binding order, and records each texture slot's color
    /// space.
    fn reconcile_with_shader(&mut self, context: &ReadOnlyAssetContext) {
        let Some(shader_ref) = self.shader.get_ref(&context.registries) else {
            return;
        };
        let shader = shader_ref.read();
        for variable in Self::reflect_variables(&shader.module) {
            if !self.variables.contains(&variable) {
                self.variables.push(variable);
            }
        }
        self.variables.sort();
        self.texture_color_spaces = shader
            .module
            .global_variables
            .iter()
            .filter_map(|(_, variable)| {
                let binding = variable.binding.as_ref()?;
                matches!(
                    shader.module.types[variable.ty].inner,
                    TypeInner::Image { .. }
                )
                .then(|| {
                    let name = variable.name.as_deref().unwrap_or_default();
                    (
                        (binding.group, binding.binding),
                        Self::slot_color_space(name),
                    )
                })
            })
            .collect();
    }

    /// Color space of a texture slot named `name` in its shader: sRGB for a color image,
    /// named as a base color, albedo, diffuse or emissive texture, and linear for data such
    /// as normals, metallic-roughness and occlusion.
    fn slot_color_space(name: &str) -> TextureColorSpace {
        let name = name.to_ascii_lowercase();
        let is_color = ["color", "albedo", "diffuse", "emissive", "emission"]
            .iter()
            .any(|word| name.contains(word));
        if is_color {
            TextureColorSpace::Srgb
        } else {
            TextureColorSpace::Linear
        }
    }

    /// Color space the texture slot at `group` and `binding` samples in.
    pub fn texture_color_space(&self, group: u32, binding: u32) -> TextureColorSpace {
        self.texture_color_spaces
            .get(&(group, binding))
            .copied()
            .unwrap_or_default()
    }

    /// The material's `alpha_cutoff`, below which the PBR shader discards a fragment, or 0 (never
    /// discards) for a material without one.
    pub(crate) fn alpha_cutoff(&self) -> f32 {
        self.variables
            .iter()
            .find(|variable| variable.name == "alpha_cutoff")
            .and_then(|variable| match variable.value {
                ShaderVariableValue::Float(cutoff) => Some(cutoff),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    /// Sets the variable named `name` to `value`. Returns false, and changes nothing, when the
    /// material has no variable of that name.
    pub fn set_variable(&mut self, name: &str, value: ShaderVariableValue) -> bool {
        match self
            .variables
            .iter_mut()
            .find(|variable| variable.name == name)
        {
            Some(variable) => {
                variable.value = value;
                true
            }
            None => false,
        }
    }

    pub(crate) fn collect_textures(
        &self,
        assets: &ReadOnlyAssetContext,
        textures: &mut HashMap<AssetId, Ref<Texture>>,
        default_texture: Ref<Texture>,
    ) {
        for var in self.variables.iter() {
            let texture = var.value.as_texture(assets, default_texture.clone());
            textures.insert(texture.id(), texture);
        }
    }

    pub(crate) fn bind_groups(
        &self,
        device: &wgpu::Device,
        asset_context: &ReadOnlyAssetContext,
        assets: &LockedAssetRenderState,
        default_texture: Ref<Texture>,
    ) -> HashMap<u32, wgpu::BindGroup> {
        let Some(shader) = assets.shader(self.shader.id()) else {
            return Default::default();
        };
        self.bind_group_entries
            .iter()
            .filter_map(|(group, entries)| {
                let entries = entries
                    .iter()
                    .map(|(binding, entry)| -> Option<wgpu::BindGroupEntry<'_>> {
                        Some(wgpu::BindGroupEntry {
                            binding: *binding,
                            resource: match &entry.ty {
                                BindingType::Buffer => {
                                    self.find_buffer(*group, *binding)?.as_entire_binding()
                                }
                                BindingType::Texture => {
                                    let var = self.find_variable(*group, *binding)?;
                                    let texture = var
                                        .value
                                        .as_texture(asset_context, default_texture.clone());
                                    wgpu::BindingResource::TextureView(
                                        assets
                                            .texture(texture.id())?
                                            .view_as(self.texture_color_space(*group, *binding)),
                                    )
                                }
                                BindingType::Sampler => {
                                    let texture = self.find_closest_texture_in_group(
                                        asset_context,
                                        *group,
                                        *binding,
                                        default_texture.clone(),
                                    );
                                    wgpu::BindingResource::Sampler(
                                        &assets.texture(texture.id())?.sampler,
                                    )
                                }
                            },
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((
                    *group,
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: shader.group_layout(*group)?,
                        entries: entries.as_slice(),
                    }),
                ))
            })
            .collect()
    }

    pub(crate) fn bind_group_cache_key(
        &self,
        asset_context: &ReadOnlyAssetContext,
        default_texture: Ref<Texture>,
    ) -> MaterialBindGroupCacheKey {
        let entries = self
            .bind_group_entries
            .iter()
            .flat_map(|(group, entries)| {
                entries.iter().filter_map(|(binding, entry)| {
                    let value = match entry.ty {
                        BindingType::Buffer => MaterialBindGroupCacheValue::Buffer {
                            buffer: self.find_buffer(*group, *binding).cloned(),
                        },
                        BindingType::Texture => {
                            let texture_id = self
                                .find_variable(*group, *binding)?
                                .value
                                .as_texture(asset_context, default_texture.clone())
                                .id();
                            MaterialBindGroupCacheValue::Texture {
                                texture_id,
                                color_space: self.texture_color_space(*group, *binding),
                            }
                        }
                        BindingType::Sampler => {
                            let texture_id = self
                                .find_closest_texture_in_group(
                                    asset_context,
                                    *group,
                                    *binding,
                                    default_texture.clone(),
                                )
                                .id();
                            MaterialBindGroupCacheValue::Sampler { texture_id }
                        }
                    };
                    Some(MaterialBindGroupCacheEntry {
                        group: *group,
                        binding: *binding,
                        value,
                    })
                })
            })
            .collect();

        MaterialBindGroupCacheKey {
            shader_id: self.shader.id(),
            shader_layout_generation: self.shader_layout_generation,
            entries,
        }
    }

    fn find_variable(&self, group: u32, binding: u32) -> Option<&ShaderVariable> {
        self.variable_indices
            .get(&(group, binding))
            .and_then(|index| self.variables.get(*index))
            .filter(|v| v.group == group && v.binding == binding)
            .or_else(|| {
                self.variables
                    .iter()
                    .find(|v| v.group == group && v.binding == binding)
            })
    }

    fn find_buffer(&self, group: u32, binding: u32) -> Option<&wgpu::Buffer> {
        self.buffers.get(&(group, binding))
    }

    fn rebuild_variable_indices(&mut self) {
        self.variable_indices = self
            .variables
            .iter()
            .enumerate()
            .map(|(index, var)| ((var.group, var.binding), index))
            .collect();
    }

    fn find_closest_texture_in_group(
        &self,
        assets: &ReadOnlyAssetContext,
        group: u32,
        binding: u32,
        default_texture: Ref<Texture>,
    ) -> Ref<Texture> {
        // The nearest texture binding, the lower one on a tie: a sampler declared after its
        // texture samples that texture.
        let closest = self
            .variables
            .iter()
            .filter(|v| v.group == group && v.value.binding_type() == BindingType::Texture)
            .min_by_key(|v| {
                (
                    (binding as i32 - v.binding as i32).unsigned_abs(),
                    v.binding,
                )
            });
        match closest {
            Some(var) => var.value.as_texture(assets, default_texture),
            None => default_texture,
        }
    }

    fn shader_variable(
        module: &naga::Module,
        ty: &naga::Type,
        binding: &naga::ResourceBinding,
        name: String,
        offset: Option<u32>,
        variables: &mut Vec<ShaderVariable>,
    ) {
        let mut var = ShaderVariable {
            group: binding.group,
            binding: binding.binding,
            name,
            span: None,
            offset,
            value: ShaderVariableValue::Sampler,
        };
        match &ty.inner {
            TypeInner::Matrix {
                rows,
                columns,
                scalar,
            } => {
                if (*rows, *columns, *scalar)
                    == (
                        VectorSize::Quad,
                        VectorSize::Quad,
                        Scalar {
                            kind: ScalarKind::Float,
                            width: 4,
                        },
                    )
                {
                    var.span = Some(*rows as u32 * *columns as u32 * scalar.width as u32);
                    var.value = ShaderVariableValue::Mat4(Default::default());
                    variables.push(var);
                }
            }
            TypeInner::Vector { size, scalar } => {
                if (scalar.kind, scalar.width) == (ScalarKind::Float, 4) {
                    var.span = Some(*size as u32 * scalar.width as u32);
                    var.value = match size {
                        VectorSize::Bi => ShaderVariableValue::Vec2(Default::default()),
                        VectorSize::Tri => ShaderVariableValue::Vec3(Default::default()),
                        VectorSize::Quad if Self::is_color_variable(&var.name) => {
                            ShaderVariableValue::Color([1.0, 1.0, 1.0, 1.0])
                        }
                        VectorSize::Quad => ShaderVariableValue::Vec4(Default::default()),
                    };
                    variables.push(var);
                }
            }
            TypeInner::Scalar(Scalar { kind, width }) => {
                if let Some((span, value)) = match kind {
                    ScalarKind::Uint if *width as usize == size_of::<u32>() => {
                        Some((*width, ShaderVariableValue::Uint(Default::default())))
                    }
                    ScalarKind::Sint if *width as usize == size_of::<i32>() => {
                        Some((*width, ShaderVariableValue::Int(Default::default())))
                    }
                    ScalarKind::Float if *width as usize == size_of::<f32>() => {
                        Some((*width, ShaderVariableValue::Float(Default::default())))
                    }
                    ScalarKind::Bool => {
                        Some((*width, ShaderVariableValue::Bool(Default::default())))
                    }
                    _ => None,
                } {
                    var.span = Some(span as u32);
                    var.value = match value {
                        ShaderVariableValue::Float(_) if Self::is_unit_multiplier(&var.name) => {
                            ShaderVariableValue::Float(1.0)
                        }
                        value => value,
                    };
                    variables.push(var);
                }
            }
            TypeInner::Image { dim, arrayed, .. } => {
                if (*dim, *arrayed) == (ImageDimension::D2, false) {
                    var.value =
                        ShaderVariableValue::Texture2D(if Self::is_normal_texture(&var.name) {
                            MaterialTexture::Color(Self::FLAT_NORMAL)
                        } else {
                            MaterialTexture::default()
                        });
                    variables.push(var);
                }
            }
            TypeInner::Sampler { .. } => {
                var.value = ShaderVariableValue::Sampler;
                variables.push(var);
            }
            TypeInner::Struct { members, .. } => {
                for member in members.iter() {
                    let ty = &module.types[member.ty];
                    Self::shader_variable(
                        module,
                        ty,
                        binding,
                        member.name.clone().unwrap_or_default(),
                        // A nested struct's members sit at its own offset plus theirs.
                        Some(offset.unwrap_or_default() + member.offset),
                        variables,
                    );
                }
            }
            _ => {}
        }
    }

    /// Texel of a tangent-space normal texture that leaves the normal as it is.
    const FLAT_NORMAL: [f32; 4] = [0.5, 0.5, 1.0, 1.0];

    fn is_color_variable(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        name == "color"
            || name.ends_with("_color")
            || name.ends_with(" color")
            || name.ends_with("_color_factor")
            || name.contains("albedo")
    }

    /// Whether the float variable `name` multiplies another input, so its neutral default is
    /// one rather than zero.
    fn is_unit_multiplier(name: &str) -> bool {
        matches!(name, "normal_scale" | "occlusion_strength")
    }

    fn is_normal_texture(name: &str) -> bool {
        name.to_ascii_lowercase().contains("normal")
    }

    #[inline]
    fn init(&mut self, assets: &ReadOnlyAssetContext) {
        self.reconcile_with_shader(assets);
        self.rebuild_variable_indices();
        self.update_entries(assets);
        self.create_buffers(assets);
        self.shader_layout_generation = self.shader_layout_generation(assets);
    }

    fn shader_layout_generation(&self, assets: &ReadOnlyAssetContext) -> Option<u64> {
        let shader = self.shader.get_ref(&assets.registries)?;
        let generation = shader.read().layout_generation();
        Some(generation)
    }

    /// Follows a hot reload that changed the layout of the material's shader: takes the
    /// shader's variables as it now reflects them, each with the value the material held under
    /// the same name for the same kind of value, wherever the shader now binds it, or else its
    /// reflected default, and rebuilds the bind-group entries and buffers for the new layout. A
    /// variable the shader no longer declares is dropped. Does nothing while the shader's
    /// [`Shader::layout_generation`] is the one the material was built for.
    pub(crate) fn follow_shader(&mut self, assets: &ReadOnlyAssetContext) {
        let generation = self.shader_layout_generation(assets);
        if generation == self.shader_layout_generation {
            return;
        }
        if let Some(shader) = self.shader.get_ref(&assets.registries) {
            let mut previous = std::mem::take(&mut self.variables);
            self.variables = Self::reflect_variables(&shader.read().module)
                .into_iter()
                .map(|mut variable| {
                    let same_kind = |other: &ShaderVariable| {
                        other.name == variable.name
                            && std::mem::discriminant(&other.value)
                                == std::mem::discriminant(&variable.value)
                    };
                    // Two structs may name a member alike: the one at the same slot first.
                    let kept = previous
                        .iter()
                        .position(|other| same_kind(other) && *other == variable)
                        .or_else(|| previous.iter().position(same_kind));
                    if let Some(index) = kept {
                        variable.value = previous.swap_remove(index).value;
                    }
                    variable
                })
                .collect();
        }
        self.bind_group_entries.clear();
        self.buffers.clear();
        self.init(assets);
    }

    fn update_entries(&mut self, context: &ReadOnlyAssetContext) {
        for var in self.variables.iter() {
            self.bind_group_entries
                .entry(var.group)
                .or_default()
                .insert(
                    var.binding,
                    BindGroupEntry {
                        ty: var.value.binding_type(),
                        size: None,
                    },
                );
        }
        let Some(shader_ref) = self.shader.get_ref(&context.registries) else {
            return;
        };
        let shader = shader_ref.read();
        for (_, var) in shader.module.global_variables.iter() {
            if let Some(binding) = &var.binding {
                let ty = &shader.module.types[var.ty];
                if let Some(entry) = self
                    .bind_group_entries
                    .get_mut(&binding.group)
                    .and_then(|e| e.get_mut(&binding.binding))
                {
                    if let BindingType::Buffer = entry.ty {
                        // A uniform's whole type, a struct or a lone scalar, vector or matrix,
                        // rounded up to the 16 bytes uniform buffers are laid out in.
                        entry.size =
                            Some(ty.inner.size(shader.module.to_ctx()).next_multiple_of(16));
                    }
                }
            }
        }
    }

    fn create_buffers(&mut self, assets: &ReadOnlyAssetContext) {
        let device = assets.render_context.device();
        for (group, entries) in self.bind_group_entries.iter() {
            for (binding, entry) in entries {
                if let BindingType::Buffer = entry.ty {
                    log::trace!(
                        "Creating material buffer for group {}, binding {}, size {:?}",
                        group,
                        binding,
                        entry.size
                    );
                    self.buffers.insert(
                        (*group, *binding),
                        device.create_buffer(&wgpu::BufferDescriptor {
                            label: None,
                            size: entry.size.unwrap_or_default() as u64,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        }),
                    );
                }
            }
        }
    }

    pub(crate) fn load_buffers(&mut self, render_context: &RenderContext) {
        let queue = render_context.queue();
        for var in self.variables.iter() {
            if let Some(buffer) = self.buffers.get(&(var.group, var.binding)) {
                queue.write_buffer(
                    buffer,
                    var.offset.unwrap_or_default() as wgpu::BufferAddress,
                    var.value.as_slice(),
                );
            }
        }
    }
}

#[derive(Deserialize)]
struct MaterialData {
    shader: AssetRef<Shader>,
    variables: Vec<ShaderVariable>,
}

impl From<(&ReadOnlyAssetContext, MaterialData)> for Material {
    fn from((assets, value): (&ReadOnlyAssetContext, MaterialData)) -> Self {
        let mut value = Self {
            shader: value.shader,
            variables: value.variables,
            variable_indices: Default::default(),
            texture_color_spaces: Default::default(),
            bind_group_entries: Default::default(),
            buffers: Default::default(),
            shader_layout_generation: None,
        };
        value.init(assets);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::{Material, MaterialData, MaterialTexture, ShaderVariableValue};
    use crate::assets::texture::TextureColorSpace;
    use crate::assets::AssetAccess;
    use crate::context::ReadOnlyAssetContext;
    use crate::render::Shader;
    use crate::test_support::TestAssets;
    use serde_json::json;
    use uuid::Uuid;

    /// The PBR scene shader's asset id.
    const PBR_SHADER: &str = "869cbd32-a789-f19d-3e24-5207f558d1c2";

    fn pbr_from_shader(context: &ReadOnlyAssetContext) -> Material {
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/mesh/pbr")
            .expect("missing pbr shader");
        Material::from_shader(context, shader)
    }

    fn value<'a>(material: &'a Material, name: &str) -> &'a ShaderVariableValue {
        &material
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .unwrap_or_else(|| panic!("the material has no {name}"))
            .value
    }

    fn float(material: &Material, name: &str) -> f32 {
        match value(material, name) {
            ShaderVariableValue::Float(value) => *value,
            _ => panic!("{name} is not a float"),
        }
    }

    fn color_texture(material: &Material, name: &str) -> [f32; 4] {
        match value(material, name) {
            ShaderVariableValue::Texture2D(MaterialTexture::Color(color)) => *color,
            _ => panic!("{name} is not a solid color texture"),
        }
    }

    #[test]
    fn the_pbr_material_reflects_the_gltf_inputs_with_neutral_defaults() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();

        let material = pbr_from_shader(&context);

        let white = [1.0; 4];
        assert!(matches!(
            value(&material, "base_color_factor"),
            ShaderVariableValue::Color(color) if *color == white
        ));
        assert_eq!(float(&material, "normal_scale"), 1.0);
        assert_eq!(float(&material, "occlusion_strength"), 1.0);
        assert_eq!(float(&material, "alpha_cutoff"), 0.0);
        assert_eq!(float(&material, "emission_strength"), 0.0);
        for texture in [
            "base_color_texture",
            "metallic_roughness_texture",
            "emissive_texture",
            "occlusion_texture",
        ] {
            assert_eq!(color_texture(&material, texture), white, "{texture}");
        }
        assert_eq!(
            color_texture(&material, "normal_texture"),
            [0.5, 0.5, 1.0, 1.0],
            "the normal texture defaults to the flat normal"
        );
        for sampler in [
            "base_color_sampler",
            "metallic_roughness_sampler",
            "normal_sampler",
            "emissive_sampler",
            "occlusion_sampler",
        ] {
            assert!(matches!(
                value(&material, sampler),
                ShaderVariableValue::Sampler
            ));
        }
    }

    #[test]
    fn color_slots_sample_as_srgb_and_data_slots_as_linear() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();

        let material = pbr_from_shader(&context);

        let expected = [
            (0, TextureColorSpace::Srgb),
            (3, TextureColorSpace::Linear),
            (5, TextureColorSpace::Linear),
            (7, TextureColorSpace::Srgb),
            (9, TextureColorSpace::Linear),
        ];
        for (binding, color_space) in expected {
            assert_eq!(
                material.texture_color_space(3, binding),
                color_space,
                "binding {binding}"
            );
        }
    }

    #[test]
    fn a_material_saved_before_the_gltf_inputs_loads_unchanged_and_gains_their_defaults() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let texture = "5a4bf5cf-b76a-65ce-5035-327f358f8017";
        // The showcase Fox's material as PR #424 saved it.
        let data: MaterialData = serde_json::from_value(json!({
            "shader": PBR_SHADER,
            "variables": [
                { "group": 3, "binding": 0, "offset": null, "name": "texture_diffuse",
                  "span": null, "value": { "Texture2D": { "Asset": texture } } },
                { "group": 3, "binding": 1, "offset": null, "name": "sampler_diffuse",
                  "span": null, "value": "Sampler" },
                { "group": 3, "binding": 2, "offset": 0, "name": "metallic", "span": 4,
                  "value": { "Float": 0.0 } },
                { "group": 3, "binding": 2, "offset": 4, "name": "roughness", "span": 4,
                  "value": { "Float": 0.8 } },
                { "group": 3, "binding": 2, "offset": 8, "name": "ambient_occlusion", "span": 4,
                  "value": { "Float": 1.0 } }
            ]
        }))
        .expect("the saved material parses");

        let material = Material::from((&context, data));

        assert!(matches!(
            value(&material, "texture_diffuse"),
            ShaderVariableValue::Texture2D(MaterialTexture::Asset(asset))
                if asset.id() == Uuid::parse_str(texture).unwrap()
        ));
        assert_eq!(float(&material, "roughness"), 0.8);
        assert_eq!(float(&material, "ambient_occlusion"), 1.0);
        assert_eq!(
            float(&material, "emission_strength"),
            0.0,
            "a material saved without emission does not emit"
        );
        assert_eq!(float(&material, "normal_scale"), 1.0);
        assert_eq!(
            color_texture(&material, "normal_texture"),
            [0.5, 0.5, 1.0, 1.0]
        );
        let reflected = pbr_from_shader(&context);
        assert_eq!(
            material.variables.len(),
            reflected.variables.len(),
            "every input the shader declares is present once"
        );
        assert!(
            material.variables.is_sorted(),
            "variables are kept in binding order"
        );
        assert_eq!(
            material.texture_color_space(3, 0),
            TextureColorSpace::Srgb,
            "the saved base color slot samples as sRGB"
        );
        // Every binding the shader declares has a resource, so the bind groups build.
        let bindings = material
            .bind_group_entries
            .get(&3)
            .map(|entries| entries.keys().copied().collect::<Vec<_>>());
        assert_eq!(bindings, Some((0..=10).collect()));
    }

    #[test]
    fn the_default_material_file_loads_with_its_saved_values() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();

        let material = context
            .registries
            .assets
            .read()
            .load::<Material>("materials/default")
            .expect("the default material loads");
        let material = material.read();

        assert_eq!(color_texture(&material, "texture_diffuse"), [1.0; 4]);
        assert_eq!(float(&material, "roughness"), 0.0);
        assert_eq!(float(&material, "ambient_occlusion"), 0.15);
        assert_eq!(float(&material, "emission_strength"), 0.0);
        assert_eq!(float(&material, "occlusion_strength"), 1.0);
    }

    #[test]
    fn color_values_are_buffer_backed_as_vec4_f32() {
        let value = ShaderVariableValue::Color([0.25, 0.5, 0.75, 1.0]);

        assert_eq!(value.as_slice().len(), 16);
    }

    #[test]
    fn color_named_vec4_variables_use_color_editor_values() {
        assert!(Material::is_color_variable("base_color"));
        assert!(Material::is_color_variable("albedo"));
        assert!(!Material::is_color_variable("clip_plane"));
    }

    #[test]
    fn material_texture_color_key_clamps_and_quantizes() {
        assert_eq!(
            MaterialTexture::color_key([-1.0, 0.5, 1.0, 2.0]),
            [0, 128, 255, 255]
        );
    }

    #[test]
    fn texture2d_accepts_asset_source_data() {
        let id = Uuid::parse_str("d2863902-8a4c-8f5b-2fef-8df1e3cf693e").unwrap();
        let value: ShaderVariableValue =
            serde_json::from_value(json!({ "Texture2D": { "Asset": id } })).unwrap();

        let ShaderVariableValue::Texture2D(MaterialTexture::Asset(asset)) = value else {
            panic!("expected texture asset source");
        };
        assert_eq!(asset.id(), id);
    }

    #[test]
    fn texture2d_serializes_color_source_data() {
        let value = ShaderVariableValue::Texture2D(MaterialTexture::Color([0.25, 0.5, 0.75, 1.0]));

        assert_eq!(
            serde_json::to_value(value).unwrap(),
            json!({ "Texture2D": { "Color": [0.25, 0.5, 0.75, 1.0] } })
        );
    }
}
