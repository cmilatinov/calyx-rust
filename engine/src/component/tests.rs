#[cfg(test)]
mod tests {
    use crate::assets::AssetRef;
    use crate::component::*;
    use crate::net::ComponentNetworkObject;
    use crate::scene::SceneData;
    use crate::test_utils::{test_registries, test_scene};
    use nalgebra_glm::Vec3;
    use rapier3d::dynamics::RigidBodyType;
    use uuid::Uuid;

    #[test]
    fn id_has_unique_uuid() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, None);
        assert_ne!(scene.uuid(a), Uuid::nil());
        assert_ne!(scene.uuid(b), Uuid::nil());
        assert_ne!(scene.uuid(a), scene.uuid(b));
    }

    #[test]
    fn transform_default_is_identity() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let t = scene.transform(go);
        assert!((t.position - Vec3::zeros()).norm() < 1e-5);
        assert!((t.scale - Vec3::from_element(1.0)).norm() < 1e-5);
    }

    #[test]
    fn camera_disabled_not_found_by_main_camera() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentCamera {
                enabled: false,
                ..Default::default()
            },
        );
        assert!(scene.main_camera().is_none());
    }

    #[test]
    fn rigid_body_dirty_on_creation() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());
        assert_eq!(
            scene.read_component::<ComponentRigidBody, _, _>(go, |c| c.dirty),
            Some(true)
        );
    }

    #[test]
    fn all_components_round_trip() {
        let registries = test_registries();
        let mut scene = registries.scene();
        let go = scene.create(
            Some(ComponentID {
                name: "TestObj".into(),
                visible: false,
                ..Default::default()
            }),
            None,
        );

        scene.set_transform(go, &nalgebra_glm::translation(&Vec3::new(1.0, 2.0, 3.0)));

        scene.add_component(
            go,
            ComponentCamera {
                fov: 1.5,
                near_plane: 0.2,
                far_plane: 999.0,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentDirectionalLight {
                intensity: 0.42,
                active: false,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentPointLight {
                radius: 25.0,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentSkyLight {
                intensity: 0.7,
                active: false,
                ..Default::default()
            },
        );

        let mesh_id = Uuid::new_v4();
        scene.add_component(
            go,
            ComponentMesh {
                mesh: AssetRef::from_id(mesh_id),
                ..Default::default()
            },
        );
        scene.add_component(go, ComponentSkinnedMesh::default());
        scene.add_component(
            go,
            ComponentBone {
                name: "spine_01".into(),
                index: 5,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 2.0 },
                friction: 0.3,
                density: 50.0,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentRigidBody {
                mass: 42.0,
                gravity_scale: 0.5,
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentNetworkObject {
                id: 777,
                owner_id: 99u64,
            },
        );

        let data: SceneData = (&scene).into();
        let restored = crate::scene::Scene::from((&registries, data));
        let rgo = restored
            .objects()
            .next()
            .expect("no game objects after restore");

        assert_eq!(restored.name(rgo), "TestObj");
        assert_eq!(
            restored.read_component::<ComponentID, _, _>(rgo, |c| c.visible),
            Some(false)
        );

        let t = restored.transform(rgo);
        assert!((t.position - Vec3::new(1.0, 2.0, 3.0)).norm() < 1e-5);

        assert_eq!(
            restored.read_component::<ComponentCamera, _, _>(rgo, |c| (c.fov, c.far_plane)),
            Some((1.5, 999.0))
        );
        assert_eq!(
            restored.read_component::<ComponentDirectionalLight, _, _>(rgo, |c| (
                c.active,
                c.intensity
            )),
            Some((false, 0.42))
        );
        assert_eq!(
            restored.read_component::<ComponentPointLight, _, _>(rgo, |c| c.radius),
            Some(25.0)
        );
        assert_eq!(
            restored.read_component::<ComponentSkyLight, _, _>(rgo, |c| (c.active, c.intensity)),
            Some((false, 0.7))
        );
        assert_eq!(
            restored.read_component::<ComponentMesh, _, _>(rgo, |c| c.mesh.id()),
            Some(mesh_id)
        );
        assert_eq!(
            restored.read_component::<ComponentBone, _, _>(rgo, |c| c.name.clone()),
            Some("spine_01".to_string())
        );
        assert_eq!(
            restored.read_component::<ComponentCollider, _, _>(rgo, |c| (c.friction, c.density)),
            Some((0.3, 50.0))
        );
        assert_eq!(
            restored.read_component::<ComponentRigidBody, _, _>(rgo, |c| (c.mass, c.gravity_scale)),
            Some((42.0, 0.5))
        );
        assert_eq!(
            restored.read_component::<ComponentNetworkObject, _, _>(rgo, |c| c.id),
            Some(777)
        );
    }
}
