use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::io::BufReader;
use std::path::Path;

use super::{AssetAccess, AssetRef, LoadedAsset};
use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::texture::Texture;
use crate::assets::Asset;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{AssetMap, LockedAssetRenderState, Shader};
use crate::utils::TypeUuid;
use egui::Color32;
use egui_wgpu::{wgpu, RenderState};
use naga::{ImageDimension, Scalar, ScalarKind, TypeInner, VectorSize};
use serde::{Deserialize, Serialize};

pub enum BindingType {
    Buffer,
    Sampler,
    Texture,
}

#[derive(Serialize, Deserialize)]
pub struct ShaderVariable {
    pub group: u32,
    pub binding: u32,
    pub offset: Option<u32>,
    pub name: String,
    pub span: Option<u32>,
    pub value: ShaderVariableValue,
}

impl PartialEq for ShaderVariable {
    fn eq(&self, other: &Self) -> bool {
        self.group == other.group && self.binding == other.group && self.offset == other.offset
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

#[derive(Serialize, Deserialize)]
pub enum ShaderVariableValue {
    Int(i32),
    Uint(u32),
    Float(f32),
    Bool(bool),
    Color(Color32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat4([[f32; 4]; 4]),
    Texture2D(AssetRef<Texture>),
    Sampler,
}

impl ShaderVariableValue {
    pub fn binding_type(&self) -> BindingType {
        match self {
            ShaderVariableValue::Sampler => BindingType::Sampler,
            ShaderVariableValue::Texture2D(_) => BindingType::Texture,
            _ => BindingType::Buffer,
        }
    }

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

    pub fn as_texture(&self, assets: &ReadOnlyAssetContext, default: Ref<Texture>) -> Ref<Texture> {
        let ShaderVariableValue::Texture2D(texture) = self else {
            return default;
        };
        texture.get_ref(assets).unwrap_or(default)
    }
}

#[derive(TypeUuid, Serialize)]
#[uuid = "f98a7f41-84d4-482d-b7af-a670b07035ae"]
pub struct Material {
    pub shader: AssetRef<Shader>,
    pub variables: Vec<ShaderVariable>,
    #[serde(skip)]
    pub bind_group_entries: BTreeMap<u32, BTreeMap<u32, BindGroupEntry>>,
    #[serde(skip)]
    pub buffers: HashMap<(u32, u32), wgpu::Buffer>,
}

pub struct BindGroupEntry {
    ty: BindingType,
    size: Option<u32>,
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
            .map_err(|_| AssetError::LoadError)?;
        let reader = BufReader::new(file);
        let data: MaterialData =
            serde_json::from_reader(reader).map_err(|_| AssetError::LoadError)?;
        let material: Material = (assets, data).into();
        Ok(LoadedAsset::new(material))
    }
}

impl Material {
    pub fn from_shader(assets: &ReadOnlyAssetContext, shader_ref: Ref<Shader>) -> Self {
        let mut material = Self {
            shader: Some(shader_ref.clone()).into(),
            variables: Default::default(),
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
        let shader = assets.shader(self.shader.id());
        self.bind_group_entries
            .iter()
            .map(|(group, entries)| {
                let entries = entries
                    .iter()
                    .map(|(binding, entry)| {
                        let var = self.find_variable(*group, *binding);
                        wgpu::BindGroupEntry {
                            binding: *binding,
                            resource: match &entry.ty {
                                BindingType::Buffer => {
                                    self.find_buffer(*group, *binding).as_entire_binding()
                                }
                                BindingType::Texture => {
                                    let texture = var
                                        .value
                                        .as_texture(asset_context, default_texture.clone());
                                    wgpu::BindingResource::TextureView(
                                        &assets.texture(texture.id()).view,
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
                                        &assets.texture(texture.id()).sampler,
                                    )
                                }
                            },
                        }
                    })
                    .collect::<Vec<_>>();
                (
                    *group,
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: &shader.bind_group_layouts[*group as usize],
                        entries: entries.as_slice(),
                    }),
                )
            })
            .collect()
    }

    fn find_variable(&self, group: u32, binding: u32) -> &ShaderVariable {
        self.variables
            .iter()
            .find(|v| v.group == group && v.binding == binding)
            .unwrap()
    }

    fn find_buffer(&self, group: u32, binding: u32) -> &wgpu::Buffer {
        self.buffers.get(&(group, binding)).unwrap()
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

    #[inline]
    fn init(&mut self, assets: &ReadOnlyAssetContext) {
        self.update_entries(assets);
        self.create_buffers(assets);
    }

    fn update_entries(&mut self, assets: &ReadOnlyAssetContext) {
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
        let Some(shader_ref) = self.shader.get_ref(assets) else {
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
            bind_group_entries: Default::default(),
            buffers: Default::default(),
        };
        value.init(assets);
        value
    }
}
