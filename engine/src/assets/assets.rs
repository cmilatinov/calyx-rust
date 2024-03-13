use eframe::wgpu;
use glm::{vec2, vec3};

use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture2D;
use crate::assets::AssetRegistry;
use crate::core::Ref;

const SCREEN_SPACE_QUAD: &str = "screen_space_quad";
const BLACK_TEXTURE_2D: &str = "black_texture_2d";
const BLACK_TEXTURE_CUBE: &str = "black_texture_sube";

pub struct Assets;

impl Assets {
    pub fn missing_texture() -> Option<Ref<Texture2D>> {
        AssetRegistry::get()
            .load::<Texture2D>("textures/missing")
            .ok()
    }

    pub fn black_texture_2d() -> Option<Ref<Texture2D>> {
        AssetRegistry::get().load_or_create(BLACK_TEXTURE_2D, || {
            Texture2D::new(
                &wgpu::TextureDescriptor {
                    label: Some(BLACK_TEXTURE_2D),
                    size: wgpu::Extent3d {
                        width: 16,
                        height: 16,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                None,
                false,
            )
        })
    }

    pub fn black_texture_cube() -> Option<Ref<Texture2D>> {
        AssetRegistry::get().load_or_create(BLACK_TEXTURE_CUBE, || {
            Texture2D::new(
                &wgpu::TextureDescriptor {
                    label: Some(BLACK_TEXTURE_CUBE),
                    size: wgpu::Extent3d {
                        width: 16,
                        height: 16,
                        depth_or_array_layers: 6,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                Some(wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::Cube),
                    ..Default::default()
                }),
                false,
            )
        })
    }

    pub fn cube() -> Option<Ref<Mesh>> {
        AssetRegistry::get().load::<Mesh>("meshes/cube").ok()
    }

    pub fn sphere() -> Option<Ref<Mesh>> {
        AssetRegistry::get().load::<Mesh>("meshes/sphere").ok()
    }

    pub fn cylinder() -> Option<Ref<Mesh>> {
        AssetRegistry::get().load::<Mesh>("meshes/cylinder").ok()
    }

    pub fn screen_space_quad() -> Option<Ref<Mesh>> {
        AssetRegistry::get().load_or_create(SCREEN_SPACE_QUAD, || {
            let mut quad = Mesh {
                indices: vec![0, 1, 2, 1, 0, 3],
                vertices: vec![
                    vec3(-1.0, -1.0, 0.0),
                    vec3(1.0, 1.0, 0.0),
                    vec3(-1.0, 1.0, 0.0),
                    vec3(1.0, -1.0, 0.0),
                ],
                normals: vec![
                    vec3(0.0, 0.0, -1.0),
                    vec3(0.0, 0.0, -1.0),
                    vec3(0.0, 0.0, -1.0),
                    vec3(0.0, 0.0, -1.0),
                ],
                uvs: [
                    vec![
                        vec2(0.0, 0.0),
                        vec2(1.0, 1.0),
                        vec2(0.0, 1.0),
                        vec2(1.0, 0.0),
                    ],
                    vec![],
                    vec![],
                    vec![],
                ],
                ..Default::default()
            };
            quad.mark_dirty();
            quad
        })
    }

    pub fn wire_circle() -> Mesh {
        const RESOLUTION: usize = 72;
        let mut circle = Mesh::default();
        circle.vertices.resize(RESOLUTION + 1, vec3(0.0, 0.0, 0.0));
        circle.normals.resize(RESOLUTION + 1, vec3(0.0, 0.0, 0.0));
        for i in 0..RESOLUTION {
            let angle = (i as f32) * 360.0 / (RESOLUTION as f32);
            let vertex = vec3(angle.to_radians().cos(), angle.to_radians().sin(), 0.0);
            circle.vertices[i] = vertex;
            circle.normals[i] = vertex;
        }
        circle.vertices[RESOLUTION] = circle.vertices[0];
        circle.normals[RESOLUTION] = circle.normals[0];
        circle.mark_dirty();
        circle
    }

    pub fn wire_cube() -> Mesh {
        let mut cube = Mesh {
            indices: vec![
                0, 1, 1, 2, 2, 3, 3, 0, 4, 5, 5, 6, 6, 7, 7, 4, 0, 4, 1, 5, 2, 6, 3, 7,
            ],
            vertices: vec![
                vec3(-0.5, -0.5, -0.5),
                vec3(-0.5, 0.5, -0.5),
                vec3(0.5, 0.5, -0.5),
                vec3(0.5, -0.5, -0.5),
                vec3(-0.5, -0.5, 0.5),
                vec3(-0.5, 0.5, 0.5),
                vec3(0.5, 0.5, 0.5),
                vec3(0.5, -0.5, 0.5),
            ],
            normals: vec![
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
            ],
            ..Default::default()
        };
        cube.mark_dirty();
        cube
    }
}
