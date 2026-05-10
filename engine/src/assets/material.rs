use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::io::BufReader;
use std::path::Path;

use super::{AssetAccess, AssetRef, LoadedAsset};
use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::texture::Texture;
use crate::assets::{Asset, AssetId};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{AssetMap, LockedAssetRenderState, Shader};
use crate::utils::TypeUuid;
use egui_wgpu::{wgpu, RenderState};
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
#[derive(Clone, Serialize)]
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

impl<'de> Deserialize<'de> for MaterialTexture {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        enum TaggedMaterialTexture {
            Asset(AssetRef<Texture>),
            Color([f32; 4]),
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum MaterialTextureData {
            LegacyAsset(AssetRef<Texture>),
            Tagged(TaggedMaterialTexture),
        }

        match MaterialTextureData::deserialize(deserializer)? {
            MaterialTextureData::LegacyAsset(asset) => Ok(Self::Asset(asset)),
            MaterialTextureData::Tagged(TaggedMaterialTexture::Asset(asset)) => {
                Ok(Self::Asset(asset))
            }
            MaterialTextureData::Tagged(TaggedMaterialTexture::Color(color)) => {
                Ok(Self::Color(color))
            }
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
#[derive(TypeUuid, Serialize)]
#[uuid = "f98a7f41-84d4-482d-b7af-a670b07035ae"]
pub struct Material {
    /// Shader asset used by this material.
    pub shader: AssetRef<Shader>,
    /// Editable shader variables.
    pub variables: Vec<ShaderVariable>,
    #[serde(skip)]
    variable_indices: HashMap<(u32, u32), usize>,
    #[serde(skip)]
    /// Cached bind-group entry metadata keyed by group and binding.
    pub bind_group_entries: BTreeMap<u32, BTreeMap<u32, BindGroupEntry>>,
    #[serde(skip)]
    /// GPU buffers created for buffer-backed shader variables.
    pub buffers: HashMap<(u32, u32), wgpu::Buffer>,
}

/// Cached metadata for one bind-group entry.
pub struct BindGroupEntry {
    ty: BindingType,
    size: Option<u32>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct MaterialBindGroupCacheKey {
    shader_id: AssetId,
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
    Buffer { size: Option<u32> },
    Texture { texture_id: AssetId },
    Sampler { texture_id: AssetId },
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
    /// variables.
    pub fn from_shader(assets: &ReadOnlyAssetContext, shader_ref: Ref<Shader>) -> Self {
        let mut material = Self {
            shader: Some(shader_ref.clone()).into(),
            variables: Default::default(),
            variable_indices: Default::default(),
            bind_group_entries: Default::default(),
            buffers: Default::default(),
        };
        {
            let shader = shader_ref.read();
            for (_, variable) in shader.module.global_variables.iter() {
                if let Some(binding) = &variable.binding {
                    let ty = &shader.module.types[variable.ty];
                    if binding.group >= 3 {
                        Self::shader_variable(
                            &shader.module,
                            ty,
                            binding,
                            variable.name.clone().unwrap_or_default(),
                            None,
                            &mut material.variables,
                        );
                    }
                }
            }
        }
        material.init(assets);
        material
    }

    pub(crate) fn collect_textures(
        &self,
        assets: &ReadOnlyAssetContext,
        textures: &mut AssetMap<Texture>,
        default_texture: Ref<Texture>,
    ) {
        for var in self.variables.iter() {
            let texture = var.value.as_texture(assets, default_texture.clone());
            textures.refs.insert(texture.id(), texture);
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
                                        &assets.texture(texture.id())?.view,
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
                        layout: &shader.bind_group_layouts[*group as usize],
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
                        BindingType::Buffer => {
                            MaterialBindGroupCacheValue::Buffer { size: entry.size }
                        }
                        BindingType::Texture => {
                            let texture_id = self
                                .find_variable(*group, *binding)?
                                .value
                                .as_texture(asset_context, default_texture.clone())
                                .id();
                            MaterialBindGroupCacheValue::Texture { texture_id }
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
        let mut closest = u32::MAX;
        let mut closest_index: isize = -1;
        for (i, var) in self
            .variables
            .iter()
            .enumerate()
            .filter(|(_, v)| v.group == group)
        {
            if let BindingType::Texture = var.value.binding_type() {
                let diff = (binding as i32 - var.binding as i32).unsigned_abs();
                if diff < closest {
                    closest = diff;
                    closest_index = i as isize;
                }
            }
        }
        if closest_index < 0 {
            return default_texture;
        }
        self.variables[closest_index as usize]
            .value
            .as_texture(assets, default_texture)
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
                    var.value = value;
                    variables.push(var);
                }
            }
            TypeInner::Image { dim, arrayed, .. } => {
                if (*dim, *arrayed) == (ImageDimension::D2, false) {
                    var.value = ShaderVariableValue::Texture2D(Default::default());
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
                        Some(member.offset),
                        variables,
                    );
                }
            }
            _ => {}
        }
    }

    fn is_color_variable(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        name == "color"
            || name.ends_with("_color")
            || name.ends_with(" color")
            || name.contains("albedo")
    }

    #[inline]
    fn init(&mut self, assets: &ReadOnlyAssetContext) {
        self.rebuild_variable_indices();
        self.update_entries(assets);
        self.create_buffers(assets);
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
                    if let TypeInner::Struct { span, .. } = &ty.inner {
                        entry.size = Some(*span);
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

    pub(crate) fn load_buffers(&mut self, render_state: &RenderState) {
        let queue = &render_state.queue;
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
            bind_group_entries: Default::default(),
            buffers: Default::default(),
        };
        value.init(assets);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::{Material, MaterialTexture, ShaderVariableValue};
    use crate::assets::AssetAccess;
    use serde_json::json;
    use uuid::Uuid;

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
    fn texture2d_accepts_legacy_asset_reference_data() {
        let id = Uuid::parse_str("d2863902-8a4c-8f5b-2fef-8df1e3cf693e").unwrap();
        let value: ShaderVariableValue =
            serde_json::from_value(json!({ "Texture2D": id })).unwrap();

        let ShaderVariableValue::Texture2D(MaterialTexture::Asset(asset)) = value else {
            panic!("expected legacy texture asset reference");
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
