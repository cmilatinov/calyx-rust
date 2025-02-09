use std::cmp::min;
use std::collections::HashMap;
use std::path::Path;

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use glm::{vec2, vec3, vec4, IVec4, Mat4, Vec2, Vec3, Vec4};
use russimp::scene::{PostProcess, Scene};

use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::render::RenderContext;
use crate::utils::TypeUuid;
use crate::{self as engine, math};

use super::LoadedAsset;

const CX_MESH_NUM_UV_CHANNELS: usize = 4;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv0: [f32; 2],
    uv1: [f32; 2],
    uv2: [f32; 2],
    uv3: [f32; 2],
    bone_indices: [i32; 4],
    bone_weights: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x2,
        5 => Float32x2,
        6 => Sint32x4,
        7 => Float32x4
    ];
}

impl BufferLayout for Vertex {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Vertex::ATTRIBUTES;
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshUniforms {
    pub num_bones: u32,
    pub _padding: [u32; 3],
    pub instances: [Instance; Mesh::MAX_INSTANCES],
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub bone_transform_index: i32,
    pub _padding: [u32; 3],
    pub transform: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BoneTransform {
    pub transform: [[f32; 4]; 4],
}

#[derive(Debug)]
pub struct BoneInfo {
    pub index: usize,
    pub inverse_bind_transform: Mat4,
}

#[derive(TypeUuid)]
#[uuid = "792d264b-de6f-4431-b59f-76f18fdb3bfe"]
pub struct Mesh {
    pub indices: Vec<u32>,
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: [Vec<Vec2>; CX_MESH_NUM_UV_CHANNELS],
    pub bone_indices: Vec<IVec4>,
    pub bone_weights: Vec<Vec4>,
    pub bones: HashMap<String, BoneInfo>,

    pub(crate) dirty: bool,
    pub(crate) instances: Vec<Instance>,
    pub(crate) bone_transforms: Vec<BoneTransform>,
    pub(crate) index_buffer: Option<wgpu::Buffer>,
    pub(crate) vertex_buffer: Option<wgpu::Buffer>,
    pub(crate) instance_buffer: wgpu::Buffer,
    pub(crate) bone_buffer: ResizableBuffer,

    pub(crate) instance_bind_group_layout: wgpu::BindGroupLayout,
}

impl Mesh {
    pub const MAX_INSTANCES: usize = 30;
    pub const MAX_BONES: usize = 100;
    // const ATTRIBUTE_VERTEX: u32 = 0;
    // const ATTRIBUTE_NORMAL: u32 = 1;
    // const ATTRIBUTE_UV0: u32 = 2;
    // const ATTRIBUTE_UV1: u32 = 3;
    // const ATTRIBUTE_UV2: u32 = 4;
    // const ATTRIBUTE_UV3: u32 = 5;
    // const ATTRIBUTE_BONE_INDICES: u32 = 6;
    // const ATTRIBUTE_BONE_WEIGHTS: u32 = 7;
    // const ATTRIBUTE_MODEL0: u32 = 6;
    // const ATTRIBUTE_MODEL1: u32 = 7;
    // const ATTRIBUTE_MODEL2: u32 = 8;
    // const ATTRIBUTE_MODEL3: u32 = 9;
}

impl Default for Mesh {
    fn default() -> Self {
        let device = RenderContext::device().unwrap();

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance_buffer"),
            size: (std::mem::size_of::<MeshUniforms>()) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });

        let instance_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("instance_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        Self {
            indices: Default::default(),
            vertices: Default::default(),
            normals: Default::default(),
            uvs: [
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            ],
            bone_indices: Default::default(),
            bone_weights: Default::default(),
            bones: Default::default(),
            dirty: false,
            instances: Default::default(),
            bone_transforms: Default::default(),
            index_buffer: None,
            vertex_buffer: None,
            instance_buffer,
            bone_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            ),
            instance_bind_group_layout,
        }
    }
}

impl Asset for Mesh {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Mesh"
    }

    fn file_extensions() -> &'static [&'static str] {
        &["obj"]
    }

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError> {
        let scene = Scene::from_file(
            path.to_str().unwrap(),
            vec![
                PostProcess::Triangulate,
                PostProcess::GenerateSmoothNormals,
                PostProcess::FlipUVs,
                PostProcess::FlipWindingOrder,
                PostProcess::JoinIdenticalVertices,
            ],
        )?;

        // Assuming you want to load the first mesh in the scene
        let mesh = scene.meshes.get(0).ok_or(AssetError::NotFound)?;
        Ok(LoadedAsset::new(Mesh::from_russimp_mesh(mesh)))
    }
}

impl Mesh {
    fn insert_bone_weight(
        bone_index: i32,
        weight: f32,
        bone_ids: &mut [i32],
        bone_weights: &mut [f32],
    ) {
        if let Some(index) = bone_weights
            .iter()
            .enumerate()
            .find(|(i, w)| bone_ids[*i] == -1 || **w < weight)
            .map(|(i, _)| i)
        {
            if bone_ids[index] != -1 {
                bone_ids[index..].rotate_right(1);
                bone_weights[index..].rotate_left(1);
            }
            bone_ids[index] = bone_index;
            bone_weights[index] = weight;
        }
    }

    pub fn from_russimp_mesh(mesh: &russimp::mesh::Mesh) -> Self {
        let indices = mesh
            .faces
            .iter()
            .flat_map(|face| face.0.iter().cloned())
            .collect();
        let mut vertices = vec![Vec3::zeros(); mesh.vertices.len()];
        let mut normals = vec![Vec3::zeros(); mesh.vertices.len()];

        let num_uvs: usize = min(mesh.uv_components.len(), CX_MESH_NUM_UV_CHANNELS);
        let mut uvs = [
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
        ];

        for (i, vertex) in mesh.vertices.iter().enumerate() {
            vertices[i] = vec3(vertex.x, vertex.y, vertex.z);
            normals[i] = vec3(mesh.normals[i].x, mesh.normals[i].y, mesh.normals[i].z);
            for (j, coord) in uvs.iter_mut().enumerate().take(num_uvs) {
                if let Some(tex_coord) = mesh.texture_coords[j].as_ref() {
                    coord[i] = vec2(tex_coord[i].x, tex_coord[i].y);
                }
            }
        }

        let mut bones = HashMap::new();
        let mut bone_indices = vec![vec4::<i32>(-1, -1, -1, -1); mesh.vertices.len()];
        let mut bone_weights = vec![vec4(0.0, 0.0, 0.0, 0.0); mesh.vertices.len()];

        for (bone_index, bone) in mesh.bones.iter().enumerate() {
            let inverse_bind_transform = math::mat4_from_russimp(&bone.offset_matrix);
            bones.insert(
                bone.name.clone(),
                BoneInfo {
                    index: bone_index,
                    inverse_bind_transform,
                },
            );
            for weight in &bone.weights {
                let vertex_bone_ids = bone_indices[weight.vertex_id as usize].as_mut_slice();
                let vertex_bone_weights = bone_weights[weight.vertex_id as usize].as_mut_slice();
                Self::insert_bone_weight(
                    bone_index as i32,
                    weight.weight,
                    vertex_bone_ids,
                    vertex_bone_weights,
                );
            }
            for (index, weight) in bone_weights.iter_mut().enumerate() {
                let vertex_bone_ids = bone_indices[index];
                let sum = vertex_bone_ids.as_slice().iter().enumerate().fold(
                    0.0,
                    |sum, (idx, bone_index)| {
                        sum + if *bone_index >= 0 {
                            weight.as_slice()[idx]
                        } else {
                            0.0
                        }
                    },
                );
                if sum > 0.00001 {
                    for w in weight.as_mut_slice() {
                        *w = *w / sum;
                    }
                }
            }
        }

        Self {
            indices,
            vertices,
            normals,
            uvs,
            bones,
            bone_indices,
            bone_weights,
            dirty: true,
            ..Default::default()
        }
    }
}

impl Mesh {
    pub fn clear(&mut self) {
        self.indices.clear();
        self.vertices.clear();
        self.normals.clear();
        self.instances.clear();
        for uv in &mut self.uvs {
            uv.clear();
        }
    }

    pub fn rebuild_index_buffer(&mut self, device: &wgpu::Device) {
        self.index_buffer = Some(device.create_buffer_init(&wgpu_buffer_init_desc(
            wgpu::BufferUsages::INDEX,
            self.indices.as_slice(),
        )));
    }

    fn rebuild_vertex_buffer(&mut self, device: &wgpu::Device) {
        let vertex_count = self.vertices.len();
        let mut vertices: Vec<Vertex> = Vec::new();
        vertices.resize(vertex_count, Vertex::default());
        for (i, vertex) in vertices.iter_mut().enumerate().take(vertex_count) {
            vertex.position = self.vertices[i].into();
            vertex.normal = self.normals[i].into();
            vertex.uv0 = self.uvs[0][i].into();
            vertex.uv1 = self.uvs[1][i].into();
            vertex.uv2 = self.uvs[2][i].into();
            vertex.uv3 = self.uvs[3][i].into();
            vertex.bone_indices = self.bone_indices[i].into();
            vertex.bone_weights = self.bone_weights[i].into();
        }
        self.vertex_buffer = Some(device.create_buffer_init(&wgpu_buffer_init_desc(
            wgpu::BufferUsages::VERTEX,
            vertices.as_slice(),
        )));
    }

    fn rebuild_instance_buffer(&self, queue: &wgpu::Queue) {
        let mut instances: [Instance; Self::MAX_INSTANCES] = Default::default();
        for (i, instance) in self.instances.iter().take(Self::MAX_INSTANCES).enumerate() {
            instances[i] = *instance;
        }
        let uniforms = MeshUniforms {
            num_bones: self.bones.len() as u32,
            _padding: Default::default(),
            instances,
        };
        queue.write_buffer(
            &self.instance_buffer,
            0 as wgpu::BufferAddress,
            bytemuck::cast_slice(&[uniforms].as_slice()),
        );
    }

    fn rebuild_bone_buffer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.bone_buffer.resize(
            device,
            16 + (self.bone_transforms.len() * std::mem::size_of::<BoneTransform>())
                .max(std::mem::size_of::<BoneTransform>()) as u64,
        );
        self.bone_buffer
            .write_buffer(device, queue, &[self.bone_transforms.len() as u32], None);
        self.bone_buffer
            .write_buffer(device, queue, &self.bone_transforms, Some(16));
    }

    fn normalize_mesh_data(&mut self) {
        let vertex_count = self.vertices.len();
        self.normals.resize(vertex_count, Vec3::zeros());
        for i in 0..CX_MESH_NUM_UV_CHANNELS {
            self.uvs[i].resize(vertex_count, Vec2::zeros());
        }
        self.bone_indices
            .resize(vertex_count, IVec4::new(-1, -1, -1, -1));
        self.bone_weights.resize(vertex_count, Vec4::zeros());
    }

    pub(crate) fn rebuild_mesh_data(&mut self, device: &wgpu::Device) {
        self.normalize_mesh_data();
        self.rebuild_index_buffer(device);
        self.rebuild_vertex_buffer(device);
    }

    pub(crate) fn rebuild_instance_data(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.rebuild_instance_buffer(queue);
        self.rebuild_bone_buffer(device, queue);
    }

    pub(crate) fn instance_bind_group(&self, device: &wgpu::Device) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("instance_bind_group"),
            layout: &self.instance_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.instance_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bone_buffer.get_wgpu_buffer().as_entire_binding(),
                },
            ],
        })
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}
