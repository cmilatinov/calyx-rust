use crate::assets::mesh::Mesh;
use crate::assets::AssetRegistry;
use crate::core::OptionRef;
use glm::{vec2, vec3};

const SCREEN_SPACE_QUAD: &str = "screen_space_quad";

pub struct Assets;

impl Assets {
    pub fn screen_space_quad() -> OptionRef<Mesh> {
        let mut registry = AssetRegistry::get_mut();
        if let Some(mesh) = registry.load::<Mesh>(SCREEN_SPACE_QUAD).ok() {
            return Some(mesh);
        }
        let mut quad = Mesh::default();
        quad.indices = vec![0, 1, 2, 1, 0, 3];
        quad.vertices = vec![
            vec3(-1.0, -1.0, 0.0),
            vec3(1.0, 1.0, 0.0),
            vec3(-1.0, 1.0, 0.0),
            vec3(1.0, -1.0, 0.0),
        ];
        quad.normals = vec![
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, 1.0),
        ];
        quad.uvs[0] = vec![
            vec2(0.0, 0.0),
            vec2(1.0, 1.0),
            vec2(0.0, 1.0),
            vec2(1.0, 0.0),
        ];
        quad.mark_dirty();
        registry.create(SCREEN_SPACE_QUAD, quad).ok()
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
        let mut cube = Mesh::default();
        cube.indices = vec![
            0, 1, 1, 2, 2, 3, 3, 0, 4, 5, 5, 6, 6, 7, 7, 4, 0, 4, 1, 5, 2, 6, 3, 7,
        ];
        cube.vertices = vec![
            vec3(-0.5, -0.5, -0.5),
            vec3(-0.5, 0.5, -0.5),
            vec3(0.5, 0.5, -0.5),
            vec3(0.5, -0.5, -0.5),
            vec3(-0.5, -0.5, 0.5),
            vec3(-0.5, 0.5, 0.5),
            vec3(0.5, 0.5, 0.5),
            vec3(0.5, -0.5, 0.5),
        ];
        cube.normals = vec![
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
        ];
        cube.mark_dirty();
        cube
    }
}
