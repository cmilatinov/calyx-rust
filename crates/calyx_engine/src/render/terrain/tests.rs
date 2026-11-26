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
