use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use egui::Color32;
use glm::{Mat4, Vec2, Vec3, Vec4};
use naga::{ImageDimension, ScalarKind, TypeInner, VectorSize};
use serde::{Deserialize, Serialize};

use crate::assets::error::AssetError;
use crate::assets::texture::Texture2D;
use crate::assets::Asset;
use crate::core::{OptionRef, Ref};
use crate::render::Shader;

pub type MaterialVariables = BTreeMap<ShaderVariableParams, ShaderVariable>;

#[derive(Serialize, Deserialize)]
pub struct ShaderVariableParams {
    group: u32,
    binding: u32,
    offset: Option<u32>,
    name: Option<String>,
    span: Option<u32>,
}

impl PartialEq for ShaderVariableParams {
    fn eq(&self, other: &Self) -> bool {
        self.group == other.group && self.binding == other.group && self.offset == other.offset
    }
}

impl Eq for ShaderVariableParams {}

impl PartialOrd for ShaderVariableParams {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.group
            .partial_cmp(&other.group)
            .or_else(|| self.binding.partial_cmp(&other.binding))
            .or_else(|| self.offset.partial_cmp(&other.offset))
    }
}

impl Ord for ShaderVariableParams {
    fn cmp(&self, other: &Self) -> Ordering {
        self.group
            .cmp(&other.group)
            .then_with(|| self.binding.cmp(&other.binding))
            .then_with(|| self.offset.cmp(&other.offset))
    }
}

#[derive(Serialize, Deserialize)]
pub enum ShaderVariable {
    Int(i32),
    Uint(u32),
    Float(f32),
    Bool(bool),
    Color(Color32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Mat4(Mat4),
    Texture2D(OptionRef<Texture2D>),
    Sampler,
    Struct(HashMap<String, ShaderVariable>),
    Array(Vec<ShaderVariable>),
}

#[derive(Serialize, Deserialize)]
pub struct Material {
    shader: Ref<Shader>,
    variables: MaterialVariables,
}

impl Asset for Material {
    fn get_file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["mat"]
    }

    fn from_file(path: &Path) -> Result<Self, AssetError>
    where
        Self: Sized,
    {
        let material_str = fs::read_to_string(path).map_err(|_| AssetError::LoadError)?;
        let material =
            serde_json::from_str(material_str.as_str()).map_err(|_| AssetError::LoadError)?;
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
                    // if binding.group >= 2 {
                    Self::shader_variable(
                        &shader.module,
                        ty,
                        binding,
                        variable.name.clone(),
                        None,
                        &mut material.variables,
                    );
                    // }
                }
            }
        }
        material
    }

    fn shader_variable(
        module: &naga::Module,
        ty: &naga::Type,
        binding: &naga::ResourceBinding,
        name: Option<String>,
        offset: Option<u32>,
        variables: &mut MaterialVariables,
    ) {
        let mut params = ShaderVariableParams {
            group: binding.group,
            binding: binding.binding,
            name,
            span: None,
            offset,
        };
        match &ty.inner {
            TypeInner::Matrix {
                rows,
                columns,
                width,
            } => {
                if (*rows, *columns, *width) == (VectorSize::Quad, VectorSize::Quad, 4) {
                    params.span = Some(*rows as u32 * *columns as u32 * *width as u32);
                    variables.insert(params, ShaderVariable::Mat4(Default::default()));
                }
            }
            TypeInner::Vector { size, kind, width } => {
                if (*kind, *width) == (ScalarKind::Float, 4) {
                    params.span = Some(*size as u32 * *width as u32);
                    variables.insert(
                        params,
                        match size {
                            VectorSize::Bi => ShaderVariable::Vec2(Default::default()),
                            VectorSize::Tri => ShaderVariable::Vec3(Default::default()),
                            VectorSize::Quad => ShaderVariable::Vec4(Default::default()),
                        },
                    );
                }
            }
            TypeInner::Scalar { kind, width } => {
                if let Some((span, var)) = match kind {
                    ScalarKind::Uint if *width == std::mem::size_of::<u32>() => {
                        Some((*width, ShaderVariable::Uint(Default::default())))
                    }
                    ScalarKind::Sint if *width == std::mem::size_of::<i32>() => {
                        Some((*width, ShaderVariable::Int(Default::default())))
                    }
                    ScalarKind::Float if *width == std::mem::size_of::<f32>() => {
                        Some((*width, ShaderVariable::Float(Default::default())))
                    }
                    ScalarKind::Bool => Some((*width, ShaderVariable::Bool(Default::default()))),
                    _ => None,
                } {
                    params.span = Some(span as u32);
                    variables.insert(params, var);
                }
            }
            TypeInner::Image { dim, arrayed, .. } => {
                if (*dim, *arrayed) == (ImageDimension::D2, false) {
                    variables.insert(params, ShaderVariable::Texture2D(OptionRef::default()));
                }
            }
            TypeInner::Sampler { .. } => {
                variables.insert(params, ShaderVariable::Sampler);
            }
            TypeInner::Struct { members, .. } => {
                for member in members.iter() {
                    let ty = &module.types[member.ty];
                    Self::shader_variable(
                        module,
                        ty,
                        binding,
                        member.name.clone(),
                        Some(member.offset),
                        variables,
                    );
                }
            }
            _ => {}
        }
    }
}
