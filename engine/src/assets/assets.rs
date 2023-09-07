use glm::{vec2, vec3};
use crate::assets::AssetRegistry;
use crate::assets::mesh::Mesh;
use crate::core::OptionRef;

const SCREEN_SPACE_QUAD: &str = "screen_space_quad";

pub struct Assets;

impl Assets {
    pub fn screen_space_quad() -> OptionRef<Mesh> {
        let mut registry = AssetRegistry::get_mut();
        if let Some(mesh) = registry.load::<Mesh>(SCREEN_SPACE_QUAD).ok() {
            return Some(mesh);
        }
        let mut quad = Mesh::default();
        quad.indices = vec![
            0, 1, 2,
            1, 0, 3
        ];
        quad.vertices = vec![
            vec3(-1.0, -1.0, 0.0),
            vec3(1.0, 1.0, 0.0),
            vec3(-1.0, 1.0, 0.0),
            vec3(1.0, -1.0, 0.0)
        ];
        quad.normals = vec![
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, 1.0)
        ];
        quad.uvs[0] = vec![
            vec2(0.0, 0.0),
            vec2(1.0, 1.0),
            vec2(0.0, 1.0),
            vec2(1.0, 0.0)
        ];
        quad.mark_dirty();
        registry.create(SCREEN_SPACE_QUAD, quad).ok()
    }
}