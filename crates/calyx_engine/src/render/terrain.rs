use crate::assets::mesh::Mesh;
use crate::component::ComponentTerrain;
use crate::core::Ref;
use crate::math::{Heightfield, HeightfieldExtent};
use crate::render::RenderContext;
use crate::scene::SceneId;

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// The heightfield is compared by identity: a reloaded heightmap texture
// decodes a new one. Bitwise keys keep NaN parameters from rebuilding every
// frame.
#[derive(PartialEq)]
struct MeshKey {
    heightfield: usize,
    parameters: [u32; 3],
}

impl MeshKey {
    fn new(heightfield: &Arc<Heightfield>, extent: &HeightfieldExtent) -> Self {
        Self {
            heightfield: Arc::as_ptr(heightfield) as usize,
            parameters: [extent.size.x, extent.size.y, extent.height_scale].map(f32::to_bits),
        }
    }
}

struct CachedMesh {
    key: MeshKey,
    // Keeps the keyed heightfield alive, so its address is not reused.
    _heightfield: Arc<Heightfield>,
    mesh: Option<Ref<Mesh>>,
    seen: bool,
}

/// The terrain meshes a scene renderer draws, one per terrain owner, rebuilt
/// only when the owner's heightfield, size or height scale changes.
#[derive(Default)]
pub(super) struct TerrainCache {
    scene: Option<SceneId>,
    meshes: HashMap<Uuid, CachedMesh>,
}

impl TerrainCache {
    pub fn begin_frame(&mut self, scene: SceneId) {
        if self.scene != Some(scene) {
            self.meshes.clear();
            self.scene = Some(scene);
        }
        for mesh in self.meshes.values_mut() {
            mesh.seen = false;
        }
    }

    /// The mesh of `owner`'s terrain over `heightfield`, or `None` when the
    /// terrain's extent is not valid.
    pub fn mesh(
        &mut self,
        render_context: &RenderContext,
        owner: Uuid,
        terrain: &ComponentTerrain,
        heightfield: &Arc<Heightfield>,
    ) -> Option<Ref<Mesh>> {
        let extent = terrain.extent();
        let key = MeshKey::new(heightfield, &extent);
        if self
            .meshes
            .get(&owner)
            .is_none_or(|cached| cached.key != key)
        {
            let mesh = build_mesh(render_context, heightfield, &extent);
            self.meshes.insert(
                owner,
                CachedMesh {
                    key,
                    _heightfield: heightfield.clone(),
                    mesh,
                    seen: true,
                },
            );
        }
        let cached = self.meshes.get_mut(&owner)?;
        cached.seen = true;
        cached.mesh.clone()
    }

    pub fn end_frame(&mut self) {
        self.meshes.retain(|_, mesh| mesh.seen);
    }
}

fn build_mesh(
    render_context: &RenderContext,
    heightfield: &Heightfield,
    extent: &HeightfieldExtent,
) -> Option<Ref<Mesh>> {
    let geometry = heightfield.geometry(extent)?;
    let mut mesh = Mesh::new(render_context);
    mesh.vertices = geometry.positions;
    mesh.normals = geometry.normals;
    mesh.tangents = geometry.tangents;
    mesh.uvs[0] = geometry.uvs;
    mesh.indices = geometry.indices;
    mesh.mark_dirty();
    // The draw list batches and looks meshes up by id, so each built mesh
    // gets its own.
    Some(Ref::from_id_value(Uuid::new_v4(), mesh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_render_context, test_scene};
    use nalgebra_glm::vec2;

    #[test]
    fn cache_rebuilds_only_for_a_new_heightfield_or_extent_and_drops_removed_terrains() {
        let render_context = test_render_context();
        let field = Arc::new(Heightfield::new(3, 3, vec![0.0; 9]).unwrap());
        let mut terrain = ComponentTerrain {
            size: vec2(4.0, 4.0),
            ..Default::default()
        };
        let owner = Uuid::new_v4();
        let scene = test_scene();
        let mut cache = TerrainCache::default();
        cache.begin_frame(scene.id());
        let first = cache
            .mesh(&render_context, owner, &terrain, &field)
            .unwrap();
        assert_eq!(first.read().vertices.len(), 9);
        assert_eq!(first.read().indices.len(), 24);
        assert_ne!(first.id(), Uuid::nil());
        cache.end_frame();

        cache.begin_frame(scene.id());
        let same = cache
            .mesh(&render_context, owner, &terrain, &field)
            .unwrap();
        assert_eq!(same.id(), first.id());
        terrain.cast_shadows = false;
        let same = cache
            .mesh(&render_context, owner, &terrain, &field)
            .unwrap();
        assert_eq!(same.id(), first.id());
        terrain.height_scale += 1.0;
        let scaled = cache
            .mesh(&render_context, owner, &terrain, &field)
            .unwrap();
        assert_ne!(scaled.id(), first.id());
        let reloaded = Arc::new(Heightfield::new(3, 3, vec![0.0; 9]).unwrap());
        let rebuilt = cache
            .mesh(&render_context, owner, &terrain, &reloaded)
            .unwrap();
        assert_ne!(rebuilt.id(), scaled.id());
        terrain.size.x = 0.0;
        assert!(cache
            .mesh(&render_context, owner, &terrain, &reloaded)
            .is_none());
        cache.end_frame();

        cache.begin_frame(scene.id());
        cache.end_frame();
        assert!(cache.meshes.is_empty());
    }
}
