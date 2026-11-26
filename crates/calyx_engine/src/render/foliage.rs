use crate::component::ComponentFoliage;
use crate::core::Ref;
use crate::math::Transform;
use crate::scene::{Prefab, SceneId};

use nalgebra::UnitQuaternion;
use nalgebra_glm::{Mat4, Vec3};
pub(super) use prefabs::FoliagePart;
use prefabs::PrefabParts;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

mod prefabs;

const MAX_INSTANCES: usize = 100_000;

#[derive(Debug, PartialEq)]
pub(super) struct FoliageInstance {
    pub prefab: usize,
    pub transform: Mat4,
}

// Asset identities and the owner's transform do not change the local layout.
// Bitwise keys also keep malformed NaN parameters from regenerating every frame.
#[derive(PartialEq)]
struct LayoutKey {
    prefabs: usize,
    seed: u32,
    parameters: [u32; 7],
}

impl From<&ComponentFoliage> for LayoutKey {
    fn from(foliage: &ComponentFoliage) -> Self {
        Self {
            prefabs: foliage.prefabs.len(),
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
    prefabs: PrefabParts,
}

impl FoliageCache {
    pub fn begin_frame(&mut self, scene: SceneId) {
        self.prefabs.begin_frame();
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

    pub fn parts(&mut self, prefab: &Ref<Prefab>) -> Arc<[FoliagePart]> {
        self.prefabs.parts(prefab)
    }

    pub fn end_frame(&mut self) {
        self.prefabs.end_frame();
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
    if foliage.prefabs.is_empty()
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
        let prefab = ((random.next() as u64 * foliage.prefabs.len() as u64) >> 32) as usize;
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
            prefab,
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
mod tests;
