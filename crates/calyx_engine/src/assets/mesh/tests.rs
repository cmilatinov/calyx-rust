use super::{BoneTransform, Instance, Mesh};
use crate::assets::Asset;
use crate::test_support::{test_render_context, TestAssets};
use nalgebra_glm::{IVec4, Vec3, Vec4};

#[test]
fn heavier_bone_weight_shifts_lighter_ones_with_their_bones() {
    let mut bone_ids = [-1; 4];
    let mut bone_weights = [0.0; 4];

    Mesh::insert_bone_weight(0, 0.3, &mut bone_ids, &mut bone_weights);
    Mesh::insert_bone_weight(1, 0.7, &mut bone_ids, &mut bone_weights);

    assert_eq!(bone_ids, [1, 0, -1, -1]);
    assert_eq!(bone_weights, [0.7, 0.3, 0.0, 0.0]);
}

#[test]
fn bone_weights_are_normalised_once_every_bone_is_bound() {
    let (bone_indices, bone_weights) =
        Mesh::bind_bone_weights(1, [vec![(0, 0.15)], vec![(0, 0.35)]]);

    assert_eq!(bone_indices[0], IVec4::new(1, 0, -1, -1));
    assert!((bone_weights[0] - Vec4::new(0.7, 0.3, 0.0, 0.0)).norm() < 1e-6);
}

#[test]
fn every_object_and_material_of_an_obj_is_imported() {
    // A textured quad, and an untextured quad split across two materials.
    let assets = TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let path = assets.fixtures_root().join("obj/TwoObjects.obj");
    let mesh = Mesh::from_file(&context, &path)
        .unwrap_or_else(|e| panic!("failed to import the two objects: {e}"))
        .asset;

    assert_eq!(mesh.vertices.len(), 10);
    assert_eq!(mesh.indices.len(), 4 * 3);
    assert!(mesh
        .indices
        .iter()
        .all(|&index| (index as usize) < mesh.vertices.len()));
    let (min, max) = mesh.vertices.iter().fold(
        (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY)),
        |(min, max), p| (min.inf(p), max.sup(p)),
    );
    assert_eq!((min, max), (Vec3::zeros(), Vec3::new(3.0, 1.0, 1.0)));
    assert_eq!(mesh.normals.len(), mesh.vertices.len());
    // The OBJ carries no tangents: they are generated, unit length and perpendicular to
    // the normals, with a handedness.
    assert_eq!(mesh.tangents.len(), mesh.vertices.len());
    for (tangent, normal) in mesh.tangents.iter().zip(&mesh.normals) {
        assert!((tangent.xyz().norm() - 1.0).abs() < 1e-4);
        assert!(tangent.xyz().dot(normal).abs() < 1e-4);
        assert_eq!(tangent.w.abs(), 1.0);
    }
    assert!(mesh
        .uvs
        .iter()
        .all(|channel| channel.len() == mesh.vertices.len()));
}

#[test]
fn instance_bind_group_is_reused_until_a_buffer_is_reallocated() {
    let render_context = test_render_context();
    let (device, queue) = (render_context.device(), render_context.queue());
    let mut mesh = Mesh::new(&render_context);
    mesh.rebuild_instance_data(device, queue);
    let first = mesh
        .instance_bind_group()
        .cloned()
        .expect("the mesh has an instance bind group");

    mesh.rebuild_instance_data(device, queue);
    assert_eq!(
        mesh.instance_bind_group(),
        Some(&first),
        "unchanged buffers"
    );

    mesh.instances = vec![Instance::default(); 1000];
    mesh.rebuild_instance_data(device, queue);
    let grown = mesh
        .instance_bind_group()
        .cloned()
        .expect("the grown buffer has a bind group");
    assert_ne!(grown, first, "a grown instance buffer rebuilds the group");

    mesh.instances.truncate(1);
    mesh.rebuild_instance_data(device, queue);
    assert_eq!(
        mesh.instance_bind_group(),
        Some(&grown),
        "fewer instances fit the buffer as it is"
    );

    mesh.bone_transforms = vec![BoneTransform::default(); Mesh::MAX_BONES];
    mesh.rebuild_instance_data(device, queue);
    assert_ne!(
        mesh.instance_bind_group(),
        Some(&grown),
        "a reallocated bone buffer rebuilds the group"
    );
}
