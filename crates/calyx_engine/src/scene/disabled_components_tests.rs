//! Tests of the scene's component enabled state.

use legion::storage::ComponentTypeId;
use serde_json::json;
use uuid::Uuid;

use crate::component::{
    ComponentCamera, ComponentCollider, ComponentDirectionalLight,
    ComponentGlobalPostProcessVolume, ComponentID, ComponentMesh, ComponentPointLight,
    ComponentRigidBody, ComponentSkinnedMesh, ComponentTransform,
};
use crate::context::ReadOnlyRegistryContext;
use crate::scene::{GameObject, Scene, SceneData};
use crate::test_support::test_registries;
use crate::utils::TypeUuid;

/// A scene holding one object with a camera and a point light.
fn scene_with_camera(registries: &ReadOnlyRegistryContext) -> (Scene, GameObject) {
    let mut scene = registries.scene();
    let go = scene.create(None, None);
    scene.add_component(go, ComponentCamera::default());
    scene.add_component(go, ComponentPointLight::default());
    (scene, go)
}

fn reload(registries: &ReadOnlyRegistryContext, scene: &Scene) -> Scene {
    let saved = serde_json::to_string(scene).unwrap();
    (
        registries,
        serde_json::from_str::<SceneData>(&saved).unwrap(),
    )
        .into()
}

#[test]
fn components_are_enabled_by_default() {
    let registries = test_registries();
    let (scene, go) = scene_with_camera(&registries);

    assert!(scene.is_component_enabled::<ComponentCamera>(go));
    assert!(scene.is_component_enabled::<ComponentPointLight>(go));
    // A component the object lacks reads as enabled too.
    assert!(scene.is_component_enabled::<ComponentRigidBody>(go));
    let data: SceneData = (&scene).into();
    assert!(data.disabled_components.is_empty());
    let saved = serde_json::to_value(&scene).unwrap();
    assert!(
        saved.get("disabled_components").is_none(),
        "a scene with nothing disabled saves as before: {saved}"
    );
}

#[test]
fn toggling_a_component_is_reported_as_a_write() {
    let registries = test_registries();
    let (mut scene, go) = scene_with_camera(&registries);
    let camera = ComponentCamera::default();
    let version = scene.write_version(go, &camera);

    assert!(scene.set_component_enabled::<ComponentCamera>(go, false));
    assert!(!scene.is_component_enabled::<ComponentCamera>(go));
    assert!(scene.is_component_enabled::<ComponentPointLight>(go));
    assert!(!scene.is_component_enabled_dyn(go, &camera));
    assert_ne!(scene.write_version(go, &camera), version);

    // Setting the state it already has changes nothing.
    let version = scene.write_version(go, &camera);
    assert!(scene.set_component_enabled::<ComponentCamera>(go, false));
    assert_eq!(scene.write_version(go, &camera), version);

    assert!(scene.set_component_enabled_dyn(go, ComponentCamera::type_uuid(), true));
    assert!(scene.is_component_enabled::<ComponentCamera>(go));
}

#[test]
fn only_types_whose_systems_honor_the_state_opt_in() {
    let registries = test_registries();
    let components = registries.components.read();
    for type_uuid in [
        ComponentCamera::type_uuid(),
        ComponentCollider::type_uuid(),
        ComponentRigidBody::type_uuid(),
        ComponentGlobalPostProcessVolume::type_uuid(),
    ] {
        assert!(components.can_disable(type_uuid), "{type_uuid}");
    }
    for type_uuid in [
        ComponentID::type_uuid(),
        ComponentTransform::type_uuid(),
        ComponentMesh::type_uuid(),
        ComponentSkinnedMesh::type_uuid(),
        ComponentPointLight::type_uuid(),
        ComponentDirectionalLight::type_uuid(),
        Uuid::new_v4(),
    ] {
        assert!(!components.can_disable(type_uuid), "{type_uuid}");
    }
}

#[test]
fn only_present_components_of_switchable_types_can_be_disabled() {
    let registries = test_registries();
    let (mut scene, go) = scene_with_camera(&registries);
    scene.add_component(go, ComponentMesh::default());

    assert!(!scene.set_component_enabled::<ComponentRigidBody>(go, false));
    assert!(!scene.set_component_enabled::<ComponentID>(go, false));
    assert!(!scene.set_component_enabled::<ComponentTransform>(go, false));
    assert!(!scene.set_component_enabled::<ComponentPointLight>(go, false));
    assert!(!scene.set_component_enabled::<ComponentMesh>(go, false));
    assert!(!scene.set_component_enabled_dyn(go, ComponentTransform::type_uuid(), false));
    assert!(!scene.set_component_enabled_dyn(go, ComponentPointLight::type_uuid(), false));
    assert!(!scene.set_component_enabled_dyn(go, ComponentMesh::type_uuid(), false));
    assert!(!scene.set_component_enabled_dyn(go, Uuid::new_v4(), false));
    assert!(scene.is_component_enabled::<ComponentTransform>(go));
    assert!(scene.is_component_enabled::<ComponentPointLight>(go));
    assert!(scene.is_component_enabled::<ComponentMesh>(go));
    let data: SceneData = (&scene).into();
    assert!(data.disabled_components.is_empty());
}

#[test]
fn a_saved_entry_for_a_type_that_cannot_be_disabled_is_ignored() {
    let registries = test_registries();
    let (mut scene, go) = scene_with_camera(&registries);
    scene.set_component_enabled::<ComponentCamera>(go, false);
    let id = scene.uuid(go);

    // Saved with the light listed too, as a file written before the light
    // had to opt in, or by hand, may be.
    let mut saved = serde_json::to_value(&scene).unwrap();
    saved["disabled_components"][id.to_string()] = json!([
        ComponentCamera::type_uuid().to_string(),
        ComponentPointLight::type_uuid().to_string(),
    ]);
    let data: SceneData = serde_json::from_value(saved).unwrap();
    assert!(!data.is_component_enabled(id, ComponentPointLight::type_uuid()));

    let loaded: Scene = (&registries, data).into();
    let loaded_go = loaded.find(id).unwrap();
    assert!(!loaded.is_component_enabled::<ComponentCamera>(loaded_go));
    assert!(
        loaded.is_component_enabled::<ComponentPointLight>(loaded_go),
        "the light loads enabled"
    );
    let resaved: SceneData = (&loaded).into();
    assert_eq!(
        resaved
            .disabled_components
            .get(&id)
            .cloned()
            .unwrap_or_default(),
        [ComponentCamera::type_uuid()].into(),
        "the light's entry is not saved again"
    );
}

#[test]
fn the_state_survives_a_save_and_load() {
    let registries = test_registries();
    let (mut scene, go) = scene_with_camera(&registries);
    scene.set_component_enabled::<ComponentCamera>(go, false);
    let id = scene.uuid(go);

    let data: SceneData = (&scene).into();
    assert_eq!(
        data.disabled_components
            .get(&id)
            .cloned()
            .unwrap_or_default(),
        [ComponentCamera::type_uuid()].into()
    );
    let loaded = reload(&registries, &scene);
    let loaded_go = loaded.find(id).unwrap();
    assert!(!loaded.is_component_enabled::<ComponentCamera>(loaded_go));
    assert!(loaded.is_component_enabled::<ComponentPointLight>(loaded_go));

    // A snapshot, as undo takes, and a clone carry it too.
    let restored = scene.restore_snapshot(scene.snapshot());
    assert!(!restored.is_component_enabled::<ComponentCamera>(restored.find(id).unwrap()));
    let copy = scene.clone();
    assert!(!copy.is_component_enabled::<ComponentCamera>(copy.find(id).unwrap()));
}

#[test]
fn removing_a_component_or_its_object_drops_its_entry() {
    let registries = test_registries();
    let (mut scene, go) = scene_with_camera(&registries);
    scene.add_component(go, ComponentCollider::default());
    scene.set_component_enabled::<ComponentCamera>(go, false);
    scene.set_component_enabled::<ComponentCollider>(go, false);

    scene.remove_component::<ComponentCamera>(go);
    scene.add_component(go, ComponentCamera::default());
    assert!(
        scene.is_component_enabled::<ComponentCamera>(go),
        "a component added again starts enabled"
    );
    assert!(scene.remove_component_dyn(go, ComponentCollider::type_uuid()));
    assert!(scene.is_component_enabled::<ComponentCollider>(go));

    scene.set_component_enabled::<ComponentCamera>(go, false);
    scene.delete(go);
    scene.flush_deletes();
    assert!(scene.is_type_enabled(go.entity, ComponentTypeId::of::<ComponentCamera>()));
    let data: SceneData = (&scene).into();
    assert!(data.disabled_components.is_empty());
}

#[test]
fn the_state_follows_a_prefab_and_its_instances() {
    let registries = test_registries();
    let (mut scene, go) = scene_with_camera(&registries);
    scene.add_component(go, ComponentCollider::default());
    let child = scene.create(None, Some(go));
    scene.add_component(child, ComponentCollider::default());
    scene.set_component_enabled::<ComponentCamera>(go, false);
    scene.set_component_enabled::<ComponentCollider>(child, false);

    // Saved with the prefab, and through its file.
    let prefab = scene.create_prefab(go);
    let saved = serde_json::to_string(&prefab).unwrap();
    let prefab: crate::scene::PrefabData = serde_json::from_str(&saved).unwrap();
    let prefab: crate::scene::Prefab = prefab.into();

    for _ in 0..2 {
        let instance = scene.instantiate_prefab(&prefab, None).unwrap();
        assert_ne!(instance, go);
        assert!(!scene.is_component_enabled::<ComponentCamera>(instance));
        assert!(scene.is_component_enabled::<ComponentCollider>(instance));
        let instance_child = scene.children(instance).next().unwrap();
        assert!(!scene.is_component_enabled::<ComponentCollider>(instance_child));
    }

    // A linked instance follows the prefab when it updates, unless it
    // overrides the state.
    let prefab_id = Uuid::new_v4();
    let (linked, _) = scene
        .instantiate_linked_prefab(prefab_id, &prefab, None)
        .unwrap();
    let (overriding, _) = scene
        .instantiate_linked_prefab(prefab_id, &prefab, None)
        .unwrap();
    scene.set_component_enabled::<ComponentCollider>(overriding, false);
    scene.set_component_enabled::<ComponentCamera>(go, true);
    let updated = scene.create_prefab(go);
    scene.update_prefab_instances(prefab_id, &updated).unwrap();

    assert!(scene.is_component_enabled::<ComponentCamera>(linked));
    assert!(scene.is_component_enabled::<ComponentCollider>(linked));
    assert!(scene.is_component_enabled::<ComponentCamera>(overriding));
    assert!(
        !scene.is_component_enabled::<ComponentCollider>(overriding),
        "the instance keeps its own state as an override"
    );
}

#[test]
fn old_enabled_fields_load_as_disabled_entries() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let objects: Vec<GameObject> = (0..5).map(|_| scene.create(None, None)).collect();
    scene.add_component(objects[0], ComponentCamera::default());
    scene.add_component(objects[1], ComponentCollider::default());
    scene.add_component(objects[2], ComponentRigidBody::default());
    scene.add_component(objects[3], ComponentGlobalPostProcessVolume::default());
    scene.add_component(objects[4], ComponentCamera::default());
    let ids: Vec<Uuid> = objects.iter().map(|go| scene.uuid(*go)).collect();

    // Saved as before the scene kept the state: each payload carries its
    // own flag, false on the first four objects and true on the last.
    let mut saved = serde_json::to_value(&scene).unwrap();
    let types = [
        ComponentCamera::type_uuid(),
        ComponentCollider::type_uuid(),
        ComponentRigidBody::type_uuid(),
        ComponentGlobalPostProcessVolume::type_uuid(),
        ComponentCamera::type_uuid(),
    ];
    for (index, (id, type_uuid)) in ids.iter().zip(types).enumerate() {
        saved["components"][id.to_string()][type_uuid.to_string()]["enabled"] = json!(index == 4);
    }
    // A post-process effect's own flag is not the volume's and stays put.
    saved["components"][ids[3].to_string()][types[3].to_string()]["settings"]["bloom"]["enabled"] =
        json!(true);

    let data: SceneData = serde_json::from_value(saved).unwrap();
    for (index, id) in ids.iter().enumerate() {
        let payload = &data.components[id][&types[index]];
        assert!(payload.get("enabled").is_none(), "{payload}");
        assert_eq!(data.is_component_enabled(*id, types[index]), index == 4);
    }
    assert_eq!(
        data.components[&ids[3]][&types[3]]["settings"]["bloom"]["enabled"],
        json!(true)
    );

    let loaded: Scene = (&registries, data).into();
    let find = |index: usize| loaded.find(ids[index]).unwrap();
    assert!(!loaded.is_component_enabled::<ComponentCamera>(find(0)));
    assert!(!loaded.is_component_enabled::<ComponentCollider>(find(1)));
    assert!(!loaded.is_component_enabled::<ComponentRigidBody>(find(2)));
    assert!(!loaded.is_component_enabled::<ComponentGlobalPostProcessVolume>(find(3)));
    assert!(loaded.is_component_enabled::<ComponentCamera>(find(4)));
    assert_eq!(
        loaded.main_camera().map(|(camera, _)| camera),
        Some(find(4))
    );
}

#[test]
fn query_helpers_filter_on_the_state() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let root = scene.create(None, None);
    let on = scene.create(None, Some(root));
    let off = scene.create(None, Some(on));
    let leaf = scene.create(None, Some(off));
    scene.add_component(on, ComponentRigidBody::default());
    scene.add_component(off, ComponentRigidBody::default());
    scene.set_component_enabled::<ComponentRigidBody>(off, false);

    assert_eq!(
        scene
            .descendants_with_enabled::<ComponentRigidBody>(root)
            .collect::<Vec<_>>(),
        vec![on]
    );
    assert_eq!(scene.ancestor_with::<ComponentRigidBody>(leaf), Some(off));
    assert_eq!(
        scene.ancestor_with_enabled::<ComponentRigidBody>(leaf),
        Some(on)
    );
}
