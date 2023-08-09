use std::cmp::min;
use glm::{Mat4, Vec2, vec2, Vec3, vec3};
use russimp::scene::{PostProcess, Scene};
use super::error::MeshError;

const CX_MESH_UV_CHANNELS: usize = 4;
const CX_MESH_NUM_ATTRIBUTES: usize = 7;

#[repr(usize)]
enum MeshAttributes {
    Vertices,
    Normals,
    Uv0,
    Uv1,
    Uv2,
    Uv3,
    Instances
}

pub struct Mesh {
    indices: Vec<u32>,
    vertices: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: [Vec<Vec2>; CX_MESH_UV_CHANNELS],
    enabled_attributes: [bool; CX_MESH_NUM_ATTRIBUTES]
}

impl Mesh {
    pub fn load(path: &str) -> Result<Self, MeshError> {
        let scene = Scene::from_file(path,
        vec![PostProcess::Triangulate ,
        PostProcess::GenerateSmoothNormals ,
        PostProcess::FlipUVs ,
        PostProcess::JoinIdenticalVertices]
        )?;

        // Assuming you want to load the first mesh in the scene
        let mesh = scene.meshes.get(0).ok_or(MeshError::MeshNotFound)?;

        let mut indices: Vec<u32> = mesh.faces.iter().flat_map(|face| face.0.iter().cloned()).collect();
        let mut vertices: Vec<Vec3> = Vec::with_capacity(mesh.vertices.len());
        let mut normals: Vec<Vec3> = Vec::with_capacity(mesh.vertices.len());

        let num_uvs: usize = min(mesh.uv_components.len(), CX_MESH_UV_CHANNELS);
        let mut uvs: [Vec<Vec2>; 4] = [
            Vec::with_capacity(mesh.vertices.len()),
            Vec::with_capacity(mesh.vertices.len()),
            Vec::with_capacity(mesh.vertices.len()),
            Vec::with_capacity(mesh.vertices.len())
        ];

        let mut enabled_attributes = [true; CX_MESH_NUM_ATTRIBUTES];

        for i in num_uvs..CX_MESH_UV_CHANNELS {
            enabled_attributes[i + MeshAttributes::Uv0 as usize] = false;
        }

        for (i, vertex) in mesh.vertices.iter().enumerate() {
            vertices[i] = vec3(vertex.x, vertex.y, vertex.z);
            normals[i] = vec3(mesh.normals[i].x, mesh.normals[i].y, mesh.normals[i].z);

            for j in 0..num_uvs {
                let tex_coord = mesh.texture_coords[j].as_ref().ok_or(MeshError::MeshNotFound)?[i];
                uvs[j][i] = vec2(tex_coord.x, tex_coord.y);
            }
        }

        Ok(Mesh {
            indices,
            vertices,
            normals,
            uvs,
            enabled_attributes
        })
    }

    pub fn clear(&mut self) {
        self.indices.clear();
        self.vertices.clear();
        self.normals.clear();

        for uv in &mut self.uvs {
            uv.clear();
        }
    }
}
