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
