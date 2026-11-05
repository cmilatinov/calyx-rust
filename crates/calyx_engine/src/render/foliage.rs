use crate::component::ComponentFoliage;
use crate::math::Transform;
use crate::scene::SceneId;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

const MAX_INSTANCES: usize = 100_000;

#[derive(Debug, PartialEq)]
pub(super) struct FoliageInstance {
    pub entry: usize,
    pub transform: Mat4,
}

// Asset identities and the owner's transform do not change the local layout.
// Bitwise keys also keep malformed NaN parameters from regenerating every frame.
#[derive(PartialEq)]
struct LayoutKey {
    entries: usize,
    seed: u32,
    parameters: [u32; 7],
}

impl From<&ComponentFoliage> for LayoutKey {
    fn from(foliage: &ComponentFoliage) -> Self {
        Self {
            entries: foliage.entries.len(),
            seed: foliage.seed,
            parameters: [
                foliage.area.x,
                foliage.area.y,
                foliage.density,
                foliage.scale_range.x,
                foliage.scale_range.y,
                foliage.rotation_range.x,
                foliage.rotation_range.y,
            ]
            .map(f32::to_bits),
        }
    }
}

struct CachedLayout {
    key: LayoutKey,
    instances: Arc<[FoliageInstance]>,
    seen: bool,
}

#[derive(Default)]
pub(super) struct FoliageCache {
    scene: Option<SceneId>,
    layouts: HashMap<Uuid, CachedLayout>,
}

impl FoliageCache {
    pub fn begin_frame(&mut self, scene: SceneId) {
        if self.scene != Some(scene) {
            self.layouts.clear();
            self.scene = Some(scene);
        }
        for layout in self.layouts.values_mut() {
            layout.seen = false;
        }
    }

    pub fn instances(&mut self, owner: Uuid, foliage: &ComponentFoliage) -> Arc<[FoliageInstance]> {
        let key = LayoutKey::from(foliage);
        let layout = self.layouts.entry(owner).or_insert_with(|| CachedLayout {
            key: LayoutKey::from(foliage),
            instances: scatter(foliage).into(),
            seen: true,
        });
        if layout.key != key {
            layout.key = key;
            layout.instances = scatter(foliage).into();
        }
        layout.seen = true;
        layout.instances.clone()
    }

    pub fn end_frame(&mut self) {
        self.layouts.retain(|_, layout| layout.seen);
    }
}

fn scatter(foliage: &ComponentFoliage) -> Vec<FoliageInstance> {
    let parameters = [
        foliage.area.x,
        foliage.area.y,
        foliage.density,
        foliage.scale_range.x,
        foliage.scale_range.y,
        foliage.rotation_range.x,
        foliage.rotation_range.y,
    ];
    if foliage.entries.is_empty()
        || parameters.iter().any(|v| !v.is_finite())
        || parameters[..5].iter().any(|v| *v <= 0.0)
    {
        return Vec::new();
    }
    let count = ((foliage.area.x as f64 * foliage.area.y as f64 * foliage.density as f64).floor()
        as usize)
        .min(MAX_INSTANCES);
    let mut random = Random(foliage.seed as u64);
    let mut instances = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = (random.next() as u64 * foliage.entries.len() as u64 >> 32) as usize;
        let position = Vec3::new(
            (random.unit() - 0.5) * foliage.area.x,
            0.0,
            (random.unit() - 0.5) * foliage.area.y,
        );
        let scale = random.between(foliage.scale_range.x, foliage.scale_range.y);
        let yaw = random
            .between(foliage.rotation_range.x, foliage.rotation_range.y)
            .to_radians();
        instances.push(FoliageInstance {
            entry,
            transform: Transform::from_components(
                position,
                UnitQuaternion::from_axis_angle(&Vec3::y_axis(), yaw),
                Vec3::repeat(scale),
            )
            .matrix(),
        });
    }
    instances
}

// Fixed algorithm, independent of library versions and platform RNGs.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16777216.0
    }
    fn between(&mut self, a: f32, b: f32) -> f32 {
        // Weighted interpolation avoids overflow for large, opposite-sign bounds.
        let t = self.unit();
        (a.min(b) as f64 * (1.0 - t as f64) + a.max(b) as f64 * t as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::FoliageEntry;
    use crate::test_support::test_scene;
    use nalgebra_glm::Vec2;

    fn patch() -> ComponentFoliage {
        ComponentFoliage {
            entries: vec![FoliageEntry::default(), FoliageEntry::default()],
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
        assert!(first.iter().any(|i| i.entry == 0) && first.iter().any(|i| i.entry == 1));
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
        foliage.entries.clear();
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
}
