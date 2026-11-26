use super::*;
use crate::math::Transform;
use crate::test_support::test_scene;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};

fn fixture() -> (Ref<Prefab>, Uuid, Uuid, Mat4) {
    let mut scene = test_scene();
    let root = scene.create(None, None);
    let child = scene.create(None, Some(root));
    let leaf = scene.create(None, Some(child));
    let root_matrix = Transform::from_components(
        vec3(3.0, 2.0, -4.0),
        UnitQuaternion::from_axis_angle(&Vec3::y_axis(), 0.5),
        vec3(2.0, 1.0, 3.0),
    )
    .matrix();
    let child_matrix = Transform::from_components(
        vec3(1.0, 0.5, 2.0),
        UnitQuaternion::from_axis_angle(&Vec3::x_axis(), 0.3),
        Vec3::repeat(0.5),
    )
    .matrix();
    scene.set_transform(root, &root_matrix);
    scene.set_transform(child, &child_matrix);
    for object in [root, child, leaf] {
        scene.add_component(
            object,
            ComponentMesh {
                mesh: AssetRef::from_id(Uuid::new_v4()),
                material: AssetRef::from_id(Uuid::new_v4()),
                cast_shadows: object != child,
            },
        );
    }
    let child_id = scene.uuid(child);
    let leaf_id = scene.uuid(leaf);
    (
        Ref::from_id_value(Uuid::new_v4(), scene.create_prefab(root)),
        child_id,
        leaf_id,
        root_matrix * child_matrix,
    )
}

#[test]
fn foliage_prefab_keeps_all_static_parts_and_root_nested_transforms() {
    let (source, child, _, expected) = fixture();
    let mut cache = PrefabParts::default();
    cache.begin_frame();
    let parts = cache.parts(&source);
    assert_eq!(parts.len(), 3);
    assert!((parts[1].transform - expected).norm() < 0.0001);
    assert!((parts[2].transform - expected).norm() < 0.0001);
    assert!(parts[0].cast_shadows && !parts[1].cast_shadows && parts[2].cast_shadows);
    let mesh: ComponentMesh = serde_json::from_value(
        source.read().data.components[&child][&ComponentMesh::type_uuid()].clone(),
    )
    .unwrap();
    assert_eq!(parts[1].mesh, mesh.mesh);
    assert_eq!(parts[1].material, mesh.material);
    cache.end_frame();
    cache.begin_frame();
    assert!(
        Arc::ptr_eq(&parts, &cache.parts(&source)),
        "unchanged visual data reuses extraction"
    );
}

#[test]
fn foliage_prefab_cache_follows_in_place_edits_and_replacement_reloads() {
    let (source, child, _, _) = fixture();
    let mut cache = PrefabParts::default();
    cache.begin_frame();
    let first = cache.parts(&source);
    source
        .write()
        .data
        .components
        .get_mut(&child)
        .unwrap()
        .get_mut(&ComponentMesh::type_uuid())
        .unwrap()["material"] = serde_json::json!(Uuid::new_v4());
    cache.begin_frame();
    let edited = cache.parts(&source);
    assert!(!Arc::ptr_eq(&first, &edited));
    assert_ne!(first[1].material, edited[1].material);
    source
        .write()
        .data
        .disabled_components
        .entry(child)
        .or_default()
        .insert(ComponentMesh::type_uuid());
    cache.begin_frame();
    assert_eq!(
        cache.parts(&source).len(),
        2,
        "disabling one mesh keeps its children"
    );
    source
        .write()
        .data
        .components
        .get_mut(&child)
        .unwrap()
        .get_mut(&ComponentID::type_uuid())
        .unwrap()["visible"] = serde_json::json!(false);
    cache.begin_frame();
    assert_eq!(
        cache.parts(&source).len(),
        1,
        "a hidden ancestor hides every descendant"
    );
    let prefab = source.read();
    let replacement = Ref::from_id_value(
        source.id(),
        Prefab {
            root: child,
            data: prefab.data.clone(),
        },
    );
    drop(prefab);
    replacement
        .write()
        .data
        .components
        .get_mut(&child)
        .unwrap()
        .get_mut(&ComponentID::type_uuid())
        .unwrap()["visible"] = serde_json::json!(true);
    cache.begin_frame();
    assert_eq!(
        cache.parts(&replacement).len(),
        1,
        "reload with same ID and new root drops old parts"
    );
    cache.end_frame();
    cache.begin_frame();
    cache.end_frame();
    assert!(cache.prefabs.is_empty());
}

#[test]
fn foliage_prefab_ignores_unsupported_components_and_malformed_unreachable_branches() {
    let (source, child, leaf, _) = fixture();
    let mut prefab = source.write();
    let root = prefab.root;
    // Script payloads are never deserialized, and cycles cannot loop forever.
    prefab.data.components.get_mut(&root).unwrap().insert(
        Uuid::new_v4(),
        serde_json::json!({"script":"not executable"}),
    );
    prefab
        .data
        .hierarchy
        .entry(leaf)
        .or_default()
        .extend([root, Uuid::new_v4()]);
    let unreachable = Uuid::new_v4();
    let payload = prefab.data.components[&child].clone();
    prefab.data.components.insert(unreachable, payload);
    let mut cache = PrefabParts::default();
    drop(prefab);
    cache.begin_frame();
    assert_eq!(cache.parts(&source).len(), 3);
    source
        .write()
        .data
        .components
        .get_mut(&child)
        .unwrap()
        .insert(
            ComponentTransform::type_uuid(),
            serde_json::json!({"transform":"bad"}),
        );
    cache.begin_frame();
    assert_eq!(
        cache.parts(&source).len(),
        1,
        "invalid parent transforms prune the subtree"
    );
    source.write().root = Uuid::new_v4();
    cache.begin_frame();
    assert!(
        cache.parts(&source).is_empty(),
        "missing root does not render detached data"
    );
}
