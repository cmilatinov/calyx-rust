use std::cmp::min;
use std::collections::HashMap;
use std::path::Path;

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use glm::{vec2, vec3, vec4, IVec4, Vec2, Vec3, Vec4};
use russimp::scene::{PostProcess, Scene};

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::utils::TypeUuid;

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
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x2,
        5 => Float32x2
    ];
}

impl BufferLayout for Vertex {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Vertex::ATTRIBUTES;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    model: [[f32; 4]; 4],
}

impl Instance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        6 => Float32x4,
        7 => Float32x4,
        8 => Float32x4,
        9 => Float32x4
    ];
}

impl BufferLayout for Instance {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Instance::ATTRIBUTES;
}

#[derive(TypeUuid)]
#[uuid = "792d264b-de6f-4431-b59f-76f18fdb3bfe"]
pub struct Mesh {
    pub indices: Vec<u32>,
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: [Vec<Vec2>; CX_MESH_NUM_UV_CHANNELS],
    pub bones: HashMap<String, usize>,
    pub bone_indices: Vec<IVec4>,
    pub bone_weights: Vec<Vec4>,

    pub(crate) dirty: bool,
    pub(crate) instances: Vec<[[f32; 4]; 4]>,
    pub(crate) index_buffer: Option<wgpu::Buffer>,
    pub(crate) vertex_buffer: Option<wgpu::Buffer>,
    pub(crate) instance_buffer: ResizableBuffer,
}

impl Mesh {
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
            bones: Default::default(),
            bone_indices: Default::default(),
            bone_weights: Default::default(),
            dirty: false,
            instances: Default::default(),
            index_buffer: None,
            vertex_buffer: None,
            instance_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            ),
        }
    }
}

impl Asset for Mesh {
    fn get_file_extensions() -> &'static [&'static str] {
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
            bones.insert(bone.name.clone(), bone_index);
            for weight in &bone.weights {
                let vertex_bone_ids = bone_indices[weight.vertex_id as usize].as_mut_slice();
                if let Some((index, bone_id)) = vertex_bone_ids
                    .iter_mut()
                    .enumerate()
                    .find(|(_, bone_id)| **bone_id != -1)
                {
                    *bone_id = bone_index as i32;
                    bone_weights[weight.vertex_id as usize][index] = weight.weight;
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
        }
        self.vertex_buffer = Some(device.create_buffer_init(&wgpu_buffer_init_desc(
            wgpu::BufferUsages::VERTEX,
            vertices.as_slice(),
        )));
    }

    fn rebuild_instance_buffer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.instance_buffer
            .write_buffer(device, queue, self.instances.as_slice(), None);
    }

    fn normalize_mesh_data(&mut self) {
        let vertex_count = self.vertices.len();
        self.normals.resize(vertex_count, Vec3::zeros());
        for i in 0..CX_MESH_NUM_UV_CHANNELS {
            self.uvs[i].resize(vertex_count, Vec2::zeros());
        }
    }

    pub(crate) fn rebuild_mesh_data(&mut self, device: &wgpu::Device) {
        self.normalize_mesh_data();
        self.rebuild_index_buffer(device);
        self.rebuild_vertex_buffer(device);
    }

    pub(crate) fn rebuild_instance_data(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.rebuild_instance_buffer(device, queue);
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}
