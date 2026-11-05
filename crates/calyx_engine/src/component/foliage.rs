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
mod tests {
    use super::*;
    use crate::scene::SceneData;
    use crate::test_support::test_registries;

    #[test]
    fn foliage_is_registered_and_round_trips_through_a_scene() {
        let registries = test_registries();
        assert!(registries
            .components
            .read()
            .component(ComponentFoliage::type_uuid())
            .is_some());
        let mut scene = registries.scene();
        let owner = scene.create(None, None);
        let prefab = uuid::Uuid::new_v4();
        scene.add_component(
            owner,
            ComponentFoliage {
                prefabs: vec![AssetRef::from_id(prefab)],
                seed: 42,
                density: 2.5,
                ..Default::default()
            },
        );
        let data: SceneData = (&scene).into();
        let json = serde_json::to_value(&data).unwrap();
        let restored: SceneData = serde_json::from_value(json).unwrap();
        let value = &restored.components[&scene.uuid(owner)][&ComponentFoliage::type_uuid()];
        let foliage: ComponentFoliage = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(foliage.seed, 42);
        assert_eq!(foliage.density, 2.5);
        assert_eq!(foliage.prefabs, vec![AssetRef::<Prefab>::from_id(prefab)]);
    }
}
