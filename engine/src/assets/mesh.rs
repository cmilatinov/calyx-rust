use std::cmp::min;
use std::path::Path;

use glm::{Vec2, vec2, Vec3, vec3};
use russimp::scene::{PostProcess, Scene};
use egui_wgpu::wgpu;
use wgpu::util::DeviceExt;

use crate::assets::Asset;
use crate::assets::error::AssetError;
use crate::render::buffer::{BufferLayout, wgpu_buffer_init_desc};

const CX_MESH_NUM_UV_CHANNELS: usize = 4;
const CX_MESH_MAX_NUM_INSTANCES: usize = 0;

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
    const ATTRIBUTES: [wgpu::VertexAttribute; 6] =
        wgpu::vertex_attr_array![
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
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![
            6 => Float32x4,
            7 => Float32x4,
            8 => Float32x4,
            9 => Float32x4
        ];
}

impl BufferLayout for Instance {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Instance::ATTRIBUTES;
}

#[derive(Default)]
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
    pub(crate) instance_buffer: Option<wgpu::Buffer>,
}

impl Asset for Mesh {
    fn get_file_extensions(&self) -> &'static [&'static str] {
        &["obj"]
    }

    fn load(&mut self, path: &str) -> Result<(), AssetError> {
        self.load(path)
    }
}

impl Mesh {
    pub fn load(&mut self, path: &str) -> Result<(), AssetError> {
        let scene = Scene::from_file(
            path,
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
        self.name = Path::new(path).file_stem().unwrap().to_os_string().into_string().unwrap();
        self.indices = mesh.faces.iter().flat_map(|face| face.0.iter().cloned()).collect();
        self.vertices = vec![Vec3::zeros(); mesh.vertices.len()];
        self.normals = vec![Vec3::zeros(); mesh.vertices.len()];

        let num_uvs: usize = min(mesh.uv_components.len(), CX_MESH_NUM_UV_CHANNELS);
        self.uvs = [
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()]
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

        for uv in &mut self.uvs {
            uv.clear();
        }
    }

    pub fn create_index_buffer(&mut self, device: &wgpu::Device) -> &wgpu::Buffer {
        self.index_buffer = Some(device.create_buffer_init(
            &wgpu_buffer_init_desc(
                wgpu::BufferUsages::INDEX,
                self.indices.as_slice(),
            )
        ));
        self.index_buffer.as_ref().unwrap()
    }

    pub fn create_vertex_buffer(&mut self, device: &wgpu::Device) -> &wgpu::Buffer {
        let vertex_count = self.vertices.len();
        let mut vertices: Vec<Vertex> = Vec::new();
        vertices.resize(vertex_count, Vertex::default());
        for i in 0..vertex_count {
            vertices[i].position = self.vertices[i].into();
            vertices[i].normal = self.normals[i].into();
            vertices[i].uv0 = self.uvs[0][i].into();
            vertices[i].uv1 = self.uvs[1][i].into();
            vertices[i].uv2 = self.uvs[2][i].into();
            vertices[i].uv3 = self.uvs[3][i].into();
        }
        self.vertex_buffer = Some(device.create_buffer_init(
            &wgpu_buffer_init_desc(
                wgpu::BufferUsages::VERTEX,
                vertices.as_slice(),
            )
        ));
        self.vertex_buffer.as_ref().unwrap()
    }

    pub fn create_instance_buffer(&mut self, device: &wgpu::Device) -> &wgpu::Buffer {
        self.instance_buffer = Some(device.create_buffer_init(
            &wgpu_buffer_init_desc(
                wgpu::BufferUsages::VERTEX,
                self.instances.as_slice(),
            )
        ));
        self.instance_buffer.as_ref().unwrap()
    }

    pub fn normalize_mesh_data(&mut self) {
        let vertex_count = self.vertices.len();
        self.normals.resize(vertex_count, Vec3::zeros());
        for i in 0..CX_MESH_NUM_UV_CHANNELS {
            self.uvs[i].resize(vertex_count, Vec2::zeros());
        }
    }

    pub fn rebuild_mesh_data(&mut self, device: &wgpu::Device) {
        self.normalize_mesh_data();
        self.create_index_buffer(device);
        self.create_vertex_buffer(device);
    }

    pub fn rebuild_instance_data(&mut self, device: &wgpu::Device) {
        self.create_instance_buffer(device);
    }

    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }
}
