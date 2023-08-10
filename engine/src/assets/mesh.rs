use std::cmp::min;
use std::ffi::CString;
use std::fmt::Formatter;
use std::sync::Arc;
use assets_manager::{AnyCache, BoxedError, Compound, SharedString};
use glm::{Mat4, Vec2, vec2, Vec3, vec3};
use russimp::scene::{PostProcess, Scene};
use serde::{Deserialize, Deserializer};
use serde::de::{Error, Visitor};
use crate::assets::Asset;
use crate::assets::error::AssetError;
use crate::core::refs;
use crate::core::refs::Ref;

const CX_MESH_UV_CHANNELS: usize = 4;
const CX_MESH_NUM_ATTRIBUTES: usize = 7;

#[repr(usize)]
enum MeshAttributes {
    Vertices,
    Normals,
    UV0,
    UV1,
    UV2,
    UV3,
    Instances,
}

#[derive(Default)]
pub struct Mesh {
    pub name: String,
    indices: Vec<u32>,
    vertices: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: [Vec<Vec2>; CX_MESH_UV_CHANNELS],
    enabled_attributes: [bool; CX_MESH_NUM_ATTRIBUTES],
}

impl Asset for Mesh {
    fn get_extensions(&self) -> &'static [&'static str] {
        &["obj"]
    }

    fn load(&mut self, path: &str) -> Result<(), AssetError> {
        self.load(path)
    }
}

impl Mesh {
    pub fn load(&mut self, path: &str) -> Result<(), AssetError> {
        let scene = Scene::from_file(path,
            vec![
                PostProcess::Triangulate ,
                PostProcess::GenerateSmoothNormals ,
                PostProcess::FlipUVs ,
                PostProcess::JoinIdenticalVertices
            ]
        )?;

        // Assuming you want to load the first mesh in the scene
        let mesh = scene.meshes.get(0).ok_or(AssetError::NotFound)?;

        self.name = "deez".to_string();
        self.indices = mesh.faces.iter().flat_map(|face| face.0.iter().cloned()).collect();
        self.vertices = vec![Vec3::zeros(); mesh.vertices.len()];
        self.normals = vec![Vec3::zeros(); mesh.vertices.len()];

        let num_uvs: usize = min(mesh.uv_components.len(), CX_MESH_UV_CHANNELS);
        self.uvs = [
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()]
        ];

        self.enabled_attributes = [true; CX_MESH_NUM_ATTRIBUTES];

        for i in num_uvs..CX_MESH_UV_CHANNELS {
            self.enabled_attributes[i + MeshAttributes::UV0 as usize] = false;
        }

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
}
