use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use egui::Color32;
use glm::{Mat4, Vec2, Vec3, Vec4};
use naga::{ImageDimension, ScalarKind, TypeInner, VectorSize};
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::texture::Texture2D;
use crate::assets::Asset;
use crate::core::Ref;
use crate::render::Shader;
use crate::utils::TypeUuid;

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
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Mat4(Mat4),
    Texture2D(Option<Ref<Texture2D>>),
    Sampler,
}

#[derive(Serialize, Deserialize, TypeUuid)]
#[uuid = "f98a7f41-84d4-482d-b7af-a670b07035ae"]
pub struct Material {
    pub shader: Ref<Shader>,
    pub variables: Vec<ShaderVariable>,
}

impl Asset for Material {
    fn get_file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxmat"]
    }

    fn from_file(path: &Path) -> Result<Self, AssetError>
    where
        Self: Sized,
    {
        let material_str = fs::read_to_string(path).map_err(|_| AssetError::LoadError)?;
        let material = serde_json::from_str(material_str.as_str()).unwrap();
        Ok(material)
    }
}

impl Material {
    pub fn from_shader(shader_ref: Ref<Shader>) -> Self {
        let mut material = Self {
            shader: shader_ref,
            variables: Default::default(),
        };
        {
            let shader = material.shader.read().unwrap();
            for (_, variable) in shader.module.global_variables.iter() {
                if let Some(binding) = &variable.binding {
                    let ty = &shader.module.types[variable.ty];
                    if binding.group >= 2 {
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
        material
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
                width,
            } => {
                if (*rows, *columns, *width) == (VectorSize::Quad, VectorSize::Quad, 4) {
                    var.span = Some(*rows as u32 * *columns as u32 * *width as u32);
                    var.value = ShaderVariableValue::Mat4(Default::default());
                    variables.push(var);
                }
            }
            TypeInner::Vector { size, kind, width } => {
                if (*kind, *width) == (ScalarKind::Float, 4) {
                    var.span = Some(*size as u32 * *width as u32);
                    var.value = match size {
                        VectorSize::Bi => ShaderVariableValue::Vec2(Default::default()),
                        VectorSize::Tri => ShaderVariableValue::Vec3(Default::default()),
                        VectorSize::Quad => ShaderVariableValue::Vec4(Default::default()),
                    };
                    variables.push(var);
                }
            }
            TypeInner::Scalar { kind, width } => {
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
}
