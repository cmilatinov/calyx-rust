use super::*;
use crate::assets::AssetRef;
use crate::test_support::test_scene;
use nalgebra_glm::Vec2;

fn patch() -> ComponentFoliage {
    ComponentFoliage {
        prefabs: vec![AssetRef::default(), AssetRef::default()],
        ..Default::default()
    }
}

#[test]
fn seed_reproduces_flat_bounded_instances_and_edits_change_them() {
    let mut foliage = patch();
    foliage.area = Vec2::new(8.0, 5.0);
    foliage.density = 2.5;
    let first = scatter(&foliage);
    assert_eq!(first.len(), 100);
    assert_eq!(first, scatter(&foliage));
    assert!(first.iter().any(|i| i.prefab == 0) && first.iter().any(|i| i.prefab == 1));
    for instance in &first {
        let t = Transform::from(instance.transform);
        assert!(t.position.x.abs() <= 4.0 && t.position.z.abs() <= 2.5);
        assert_eq!(t.position.y, 0.0);
        assert!((0.8..=1.2).contains(&t.scale.x));
    }
    foliage.seed += 1;
    assert_ne!(first, scatter(&foliage));
    foliage.density = 5.0;
    assert_eq!(scatter(&foliage).len(), 200);
    foliage.rotation_range = Vec2::repeat(90.0);
    foliage.scale_range = Vec2::repeat(2.0);
    let rotated = scatter(&foliage);
    let direction = rotated[0].transform * nalgebra_glm::vec4(1.0, 0.0, 0.0, 0.0);
    assert!((direction.z.abs() - 2.0).abs() < 0.0001);
    assert!(direction.x.abs() < 0.0001);
}

#[test]
fn invalid_parameters_are_empty_and_extreme_density_is_bounded() {
    let mut foliage = patch();
    foliage.density = f32::MAX;
    assert_eq!(scatter(&foliage).len(), MAX_INSTANCES);
    for invalid in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
        foliage.density = invalid;
        assert!(scatter(&foliage).is_empty());
    }
    foliage.density = 1.0;
    foliage.prefabs.clear();
    assert!(scatter(&foliage).is_empty());
}

#[test]
fn cache_reuses_layout_until_parameters_or_scene_change_and_removes_deleted_patches() {
    let mut foliage = patch();
    let owner = Uuid::new_v4();
    let mut cache = FoliageCache::default();
    let scene = test_scene();
    cache.begin_frame(scene.id());
    let first = cache.instances(owner, &foliage);
    cache.end_frame();
    cache.begin_frame(scene.id());
    assert!(Arc::ptr_eq(&first, &cache.instances(owner, &foliage)));
    foliage.cast_shadows = false;
    assert!(Arc::ptr_eq(&first, &cache.instances(owner, &foliage)));
    foliage.seed += 1;
    assert!(!Arc::ptr_eq(&first, &cache.instances(owner, &foliage)));
    let changed = cache.instances(owner, &foliage);
    cache.begin_frame(test_scene().id());
    assert!(!Arc::ptr_eq(&changed, &cache.instances(owner, &foliage)));
    cache.begin_frame(scene.id());
    cache.instances(owner, &foliage);
    cache.end_frame();
    cache.begin_frame(scene.id());
    cache.end_frame();
    assert!(cache.layouts.is_empty());
}
