use crate::component::ComponentSkinnedMesh;
use crate::render::draw_collection::{skinned_mesh_world_transform, skinning_instance_index};
use crate::scene::GameObjectRef;
use crate::test_support::test_scene;
use nalgebra_glm as glm;

#[test]
fn skinned_mesh_is_placed_by_its_root_bone_not_its_own_node() {
    // A model root, a node converting axes and units, and the skinned mesh below it, as
    // imported FBX and glTF models are laid out.
    let mut scene = test_scene();
    let root = scene.create(None, None);
    scene.set_transform(root, &glm::translation(&glm::vec3(5.0, 0.0, 0.0)));
    let converter = scene.create(None, Some(root));
    scene.set_transform(
        converter,
        &(glm::rotation(-90f32.to_radians(), &glm::Vec3::x())
            * glm::scaling(&glm::Vec3::repeat(100.0))),
    );
    let mesh_object = scene.create(None, Some(converter));
    let skinned_mesh = ComponentSkinnedMesh {
        root_bone: GameObjectRef::new(scene.uuid(root)),
        ..Default::default()
    };

    let placed = skinned_mesh_world_transform(&scene, mesh_object, &skinned_mesh).matrix();

    assert_eq!(placed, scene.world_transform(root).matrix());
    assert_ne!(placed, scene.world_transform(mesh_object).matrix());
}

#[test]
fn skinned_mesh_without_a_root_bone_keeps_its_own_transform() {
    let mut scene = test_scene();
    let parent = scene.create(None, None);
    scene.set_transform(parent, &glm::scaling(&glm::Vec3::repeat(2.0)));
    let mesh_object = scene.create(None, Some(parent));

    let placed =
        skinned_mesh_world_transform(&scene, mesh_object, &ComponentSkinnedMesh::default());

    assert_eq!(placed.matrix(), scene.world_transform(mesh_object).matrix());
}

#[test]
fn mesh_without_bones_has_no_instance_index() {
    assert_eq!(skinning_instance_index(0, 0), None);
    assert_eq!(skinning_instance_index(8, 0), None);
}

#[test]
fn instance_index_counts_uploaded_bone_sets() {
    assert_eq!(skinning_instance_index(0, 4), Some(0));
    assert_eq!(skinning_instance_index(8, 4), Some(2));
}
