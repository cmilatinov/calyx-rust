use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use egui::Color32;
use egui_wgpu::{wgpu, RenderState};
use naga::{ImageDimension, Scalar, ScalarKind, TypeInner, VectorSize};
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::texture::Texture;
use crate::assets::Asset;
use crate::core::Ref;
use crate::render::{AssetMap, LockedAssetRenderState, RenderContext, Shader};
use crate::utils::TypeUuid;

use super::{Assets, LoadedAsset};

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
        self.group
            .partial_cmp(&other.group)
            .or_else(|| self.binding.partial_cmp(&other.binding))
            .or_else(|| self.offset.partial_cmp(&other.offset))
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
    Texture2D(Option<Ref<Texture>>),
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

    pub fn as_texture(&self) -> Ref<Texture> {
        if let ShaderVariableValue::Texture2D(texture) = self {
            texture
                .clone()
                .unwrap_or(Assets::missing_texture().unwrap())
        } else {
            unreachable!()
        }
    }
}

#[derive(Serialize, Deserialize, TypeUuid)]
#[uuid = "f98a7f41-84d4-482d-b7af-a670b07035ae"]
#[serde(from = "MaterialShadow")]
pub struct Material {
    pub shader: Ref<Shader>,
    pub variables: Vec<ShaderVariable>,
    #[serde(skip_serializing, skip_deserializing)]
    pub bind_group_entries: BTreeMap<u32, BTreeMap<u32, BindGroupEntry>>,
    #[serde(skip_serializing, skip_deserializing)]
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

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let material_str = fs::read_to_string(path).map_err(|_| AssetError::LoadError)?;
        let mut material: Material = serde_json::from_str(material_str.as_str()).unwrap();
        material.init();
        Ok(LoadedAsset::new(material))
    }
}

impl Material {
    pub fn from_shader(shader_ref: Ref<Shader>) -> Self {
        let mut material = Self {
            shader: shader_ref,
            variables: Default::default(),
            bind_group_entries: Default::default(),
            buffers: Default::default(),
        };
        {
            let shader = material.shader.read();
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
        material.init();
        material
    }

    pub(crate) fn collect_textures(&self, textures: &mut AssetMap<Texture>) {
        let missing_texture = Assets::missing_texture().unwrap();
        for var in self.variables.iter() {
            if let ShaderVariableValue::Texture2D(texture) = &var.value {
                let texture = texture.clone().unwrap_or(missing_texture.clone());
                textures.refs.insert(texture.id(), texture);
            }
        }
    }

    pub(crate) fn bind_groups(
        &self,
        device: &wgpu::Device,
        assets: &LockedAssetRenderState,
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
                                    let texture = var.value.as_texture();
                                    wgpu::BindingResource::TextureView(
                                        &assets.texture(texture.id()).view,
                                    )
                                }
                                BindingType::Sampler => {
                                    let texture =
                                        self.find_closest_texture_in_group(*group, *binding);
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

    fn find_closest_texture_in_group(&self, group: u32, binding: u32) -> Ref<Texture> {
        let mut closest = u32::MAX;
        let mut closest_index: isize = -1;
        for (i, var) in self
            .variables
            .iter()
            .enumerate()
            .filter(|(_, v)| v.group == group)
        {
            if let BindingType::Texture = var.value.binding_type() {
                let diff = (binding as i32 - var.binding as i32).abs() as u32;
                if diff < closest {
                    closest = diff;
                    closest_index = i as isize;
                }
            }
        }
        self.variables[closest_index as usize].value.as_texture()
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
                    ScalarKind::Uint if *width as usize == std::mem::size_of::<u32>() => {
                        Some((*width, ShaderVariableValue::Uint(Default::default())))
                    }
                    ScalarKind::Sint if *width as usize == std::mem::size_of::<i32>() => {
                        Some((*width, ShaderVariableValue::Int(Default::default())))
                    }
                    ScalarKind::Float if *width as usize == std::mem::size_of::<f32>() => {
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
                    var.value = ShaderVariableValue::Texture2D(Option::<Ref<_>>::default());
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
    fn init(&mut self) {
        self.update_entries();
        self.create_buffers();
    }

    fn update_entries(&mut self) {
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
        let shader = self.shader.read();
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

    fn create_buffers(&mut self) {
        let device = RenderContext::device().unwrap();
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
struct MaterialShadow {
    shader: Ref<Shader>,
    variables: Vec<ShaderVariable>,
}

impl From<MaterialShadow> for Material {
    fn from(value: MaterialShadow) -> Self {
        let mut value = Self {
            shader: value.shader,
            variables: value.variables,
            bind_group_entries: Default::default(),
            buffers: Default::default(),
        };
        value.init();
        value
    }
}
