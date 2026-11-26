use crate as calyx_engine;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::Prefab;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Vec2;
use serde::{Deserialize, Serialize};

/// Deterministic visual prefab instances on a flat, local XZ rectangle, centered on its owner.
/// Copies share the owner's visibility, transform, selection and shadow setting;
/// they do not create game objects or colliders. Only static meshes are drawn;
/// scripts, lights, particles and skinned meshes are not instantiated or run.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "e0ded3f3-5cc1-4412-a1ad-40db8a597735"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Foliage", category = "Rendering")]
#[serde(default)]
pub struct ComponentFoliage {
    /// Equally likely prefab choices, preserving every visible static mesh part,
    /// material and root/nested transform. Missing or meshless assets draw nothing.
    pub prefabs: Vec<AssetRef<Prefab>>,
    /// Width (X) and depth (Z) in local units. The ground plane is local Y = 0.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub area: Vec2,
    /// Copies per square local unit, rounded down; limited to 100,000 per patch.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub density: f32,
    /// Repeating a seed and the same parameters reproduces the same layout.
    pub seed: u32,
    /// Minimum and maximum uniform scale. Values must be positive.
    #[reflect_attr(min = 0.001, speed = 0.01)]
    pub scale_range: Vec2,
    /// Minimum and maximum rotation around local Y, in degrees.
    #[reflect_attr(speed = 1.0)]
    pub rotation_range: Vec2,
    /// Whether prefab parts that cast shadows may draw into shadow maps.
    pub cast_shadows: bool,
}

impl Default for ComponentFoliage {
    fn default() -> Self {
        Self {
            prefabs: Vec::new(),
            area: Vec2::new(10.0, 10.0),
            density: 1.0,
            seed: 0,
            scale_range: Vec2::new(0.8, 1.2),
            rotation_range: Vec2::new(0.0, 360.0),
            cast_shadows: true,
        }
    }
}

impl Component for ComponentFoliage {
    fn can_disable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
