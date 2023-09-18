use std::cmp::min;
use std::path::{PathBuf};

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use glm::{vec2, vec3, Vec2, Vec3};
use russimp::scene::{PostProcess, Scene};

use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};

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

pub struct Mesh {
    pub name: String,
    pub indices: Vec<u32>,
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: [Vec<Vec2>; CX_MESH_NUM_UV_CHANNELS],

    pub(crate) dirty: bool,
    pub(crate) instances: Vec<[[f32; 4]; 4]>,
    pub(crate) index_buffer: Option<wgpu::Buffer>,
    pub(crate) vertex_buffer: Option<wgpu::Buffer>,
    pub(crate) instance_buffer: ResizableBuffer,
}

impl Default for Mesh {
    fn default() -> Self {
        Self {
            name: String::new(),
            indices: Vec::new(),
            vertices: Vec::new(),
            normals: Vec::new(),
            uvs: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            dirty: false,
            instances: Vec::new(),
            index_buffer: None,
            vertex_buffer: None,
            instance_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            ),
        }
    }
}

impl Asset for Mesh {
    fn get_file_extensions(&self) -> &'static [&'static str] {
        &["obj"]
    }
    fn load(&mut self, path: PathBuf) -> Result<(), AssetError> {
        self.load(path)
    }
}

impl Mesh {
    pub fn load(&mut self, path: PathBuf) -> Result<(), AssetError> {
        let scene = Scene::from_file(
            path.to_str().unwrap(),
            vec![
                PostProcess::Triangulate,
                PostProcess::GenerateSmoothNormals,
                PostProcess::FlipUVs,
                PostProcess::JoinIdenticalVertices,
            ],
        )?;

        // Assuming you want to load the first mesh in the scene
        let mesh = scene.meshes.get(0).ok_or(AssetError::NotFound)?;

        self.dirty = true;
        self.name = String::from(path.file_stem().unwrap().to_str().unwrap());
        self.indices = mesh
            .faces
            .iter()
            .flat_map(|face| face.0.iter().cloned())
            .collect();
        self.vertices = vec![Vec3::zeros(); mesh.vertices.len()];
        self.normals = vec![Vec3::zeros(); mesh.vertices.len()];

        let num_uvs: usize = min(mesh.uv_components.len(), CX_MESH_NUM_UV_CHANNELS);
        self.uvs = [
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
        ];

        for (i, vertex) in mesh.vertices.iter().enumerate() {
            self.vertices[i] = vec3(vertex.x, vertex.y, vertex.z);
            self.normals[i] = vec3(mesh.normals[i].x, mesh.normals[i].y, mesh.normals[i].z);

            for j in 0..num_uvs {
                if let Some(tex_coord) = mesh.texture_coords[j].as_ref() {
                    self.uvs[j][i] = vec2(tex_coord[i].x, tex_coord[i].y);
                }
            }
        }

        Ok(())
    }

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
            .write_buffer(device, queue, self.instances.as_slice());
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
