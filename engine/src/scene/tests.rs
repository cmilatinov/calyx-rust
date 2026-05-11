#[cfg(test)]
mod tests {
    use crate::component::{ComponentCamera, ComponentID};
    use crate::scene::{GameObject, Scene, SceneData, SiblingDir};
    use crate::test_utils::test_scene;
    use nalgebra_glm::{self as glm, Vec3};
    use uuid::Uuid;

    // --- Creation & Identity ---

    #[test]
    fn create() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        assert_eq!(scene.name(go), "Game Object");
        assert!(scene.find(scene.uuid(go)).is_some());
    }

    #[test]
    fn create_increments_name() {
        let mut scene = test_scene();
        let go1 = scene.create(None, None);
        let go2 = scene.create(None, None);
        assert_eq!(scene.name(go1), "Game Object");
        assert_eq!(scene.name(go2), "Game Object (1)");
    }

    #[test]
    fn create_with_custom_name() {
        let mut scene = test_scene();
        let id = ComponentID {
            name: "Player".into(),
            ..Default::default()
        };
        let go = scene.create(Some(id), None);
        assert_eq!(scene.name(go), "Player");
    }

    #[test]
    fn uuid_and_find() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let id = scene.uuid(go);
        assert_ne!(id, Uuid::nil());
        assert_eq!(scene.find(id), Some(go));
    }

    #[test]
    fn find_returns_none_for_unknown_uuid() {
        let scene = test_scene();
        assert!(scene.find(Uuid::new_v4()).is_none());
    }

    #[test]
    fn root_and_root_id() {
        let scene = test_scene();
        let root = scene.root();
        let root_id = scene.root_id();
        assert_eq!(scene.uuid(root), root_id);
    }

    // --- Hierarchy ---

    #[test]
    fn parent_child() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));

        assert_eq!(scene.parent(child), Some(parent));
        let children: Vec<GameObject> = scene.children(parent).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child);
    }

    #[test]
    fn parent_uuid() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));
        assert_eq!(scene.parent_uuid(child), Some(scene.uuid(parent)));
    }

    #[test]
    fn reparent() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, None);
        let child = scene.create(None, Some(a));

        scene.set_parent(child, Some(b));
        assert_eq!(scene.parent(child), Some(b));
        assert_eq!(scene.children(a).count(), 0);
        assert_eq!(scene.children(b).count(), 1);
    }

    #[test]
    fn set_parent_with_sibling_inserts_before() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));
        let new_child = scene.create(None, None);

        scene.set_parent_with_sibling(new_child, Some(parent), Some((c2, SiblingDir::Before)));

        let ordered: Vec<GameObject> = scene.children_ordered(parent).collect();
        assert_eq!(ordered, vec![c1, new_child, c2]);
    }

    #[test]
    fn set_parent_with_sibling_inserts_after() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));
        let new_child = scene.create(None, None);

        scene.set_parent_with_sibling(new_child, Some(parent), Some((c1, SiblingDir::After)));

        let ordered: Vec<GameObject> = scene.children_ordered(parent).collect();
        assert_eq!(ordered, vec![c1, new_child, c2]);
    }

    #[test]
    fn index_in_parent() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));

        assert_eq!(
            scene.index_in_parent(parent, c1, SiblingDir::Before),
            Some(0)
        );
        assert_eq!(
            scene.index_in_parent(parent, c2, SiblingDir::Before),
            Some(1)
        );
        assert_eq!(
            scene.index_in_parent(parent, c1, SiblingDir::After),
            Some(1)
        );
    }

    #[test]
    fn child_at() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));

        assert_eq!(scene.child_at(parent, 0), Some(c1));
        assert_eq!(scene.child_at(parent, 1), Some(c2));
        assert_eq!(scene.child_at(parent, 2), None);
    }

    #[test]
    fn children_walker() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));

        let mut walker = scene.children_walker(parent);
        let mut walked = vec![];
        while let Some(go) = walker.next(&scene) {
            walked.push(go);
        }
        assert_eq!(walked.len(), 2);
        assert!(walked.contains(&c1));
        assert!(walked.contains(&c2));
    }

    #[test]
    fn children_ordered() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));
        let c3 = scene.create(None, Some(parent));

        let ordered: Vec<GameObject> = scene.children_ordered(parent).collect();
        assert_eq!(ordered, vec![c1, c2, c3]);
    }

    #[test]
    fn root_objects() {
        let mut scene = test_scene();
        let go1 = scene.create(None, None);
        let go2 = scene.create(None, None);
        let _child = scene.create(None, Some(go1));

        let roots: Vec<GameObject> = scene.root_objects().collect();
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&go1));
        assert!(roots.contains(&go2));
    }

    #[test]
    fn prefab_root() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        assert_eq!(scene.prefab_root(), Some(go));
    }

    #[test]
    fn objects_iterator() {
        let mut scene = test_scene();
        let go1 = scene.create(None, None);
        let go2 = scene.create(None, None);
        let go3 = scene.create(None, Some(go1));

        let all: Vec<GameObject> = scene.objects().collect();
        assert_eq!(all.len(), 3);
        assert!(all.contains(&go1));
        assert!(all.contains(&go2));
        assert!(all.contains(&go3));
    }

    #[test]
    fn visibility_in_hierarchy_respects_parent_state() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));
        let unrelated = scene.create(None, None);

        scene.write_component::<ComponentID, _>(parent, |id| id.visible = false);

        assert!(!scene.is_visible_in_hierarchy(parent));
        assert!(!scene.is_visible_in_hierarchy(child));
        assert!(scene.is_visible_in_hierarchy(unrelated));
    }

    // --- Deletion ---

    #[test]
    fn delete() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let uuid = scene.uuid(go);

        scene.delete(go);
        scene.flush_deletes();

        assert!(scene.find(uuid).is_none());
    }

    #[test]
    fn prepare_flushes_deletes() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let uuid = scene.uuid(go);

        scene.delete(go);
        scene.prepare();

        assert!(scene.find(uuid).is_none());
    }

    // --- Descendants & Ancestors ---

    #[test]
    fn is_descendant() {
        let mut scene = test_scene();
        let grandparent = scene.create(None, None);
        let parent = scene.create(None, Some(grandparent));
        let child = scene.create(None, Some(parent));
        let unrelated = scene.create(None, None);

        assert!(scene.is_descendant(grandparent, child));
        assert!(scene.is_descendant(grandparent, parent));
        assert!(!scene.is_descendant(grandparent, unrelated));
    }

    #[test]
    fn descendants() {
        let mut scene = test_scene();
        let root_go = scene.create(None, None);
        let child = scene.create(None, Some(root_go));
        let grandchild = scene.create(None, Some(child));

        let desc: Vec<GameObject> = scene.descendants(root_go).collect();
        assert!(desc.contains(&child));
        assert!(desc.contains(&grandchild));
    }

    #[test]
    fn descendants_with() {
        let mut scene = test_scene();
        let root_go = scene.create(None, None);
        let child = scene.create(None, Some(root_go));
        let grandchild = scene.create(None, Some(child));

        scene.add_component(grandchild, ComponentCamera::default());

        let with_camera: Vec<GameObject> =
            scene.descendants_with::<ComponentCamera>(root_go).collect();
        assert_eq!(with_camera.len(), 1);
        assert_eq!(with_camera[0], grandchild);
    }

    #[test]
    fn ancestors() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, Some(a));
        let c = scene.create(None, Some(b));

        let anc: Vec<GameObject> = scene.ancestors(c).collect();
        assert!(anc.contains(&b));
        assert!(anc.contains(&a));
    }

    #[test]
    fn ancestor_with() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, Some(a));
        let c = scene.create(None, Some(b));

        scene.add_component(a, ComponentCamera::default());

        let found = scene.ancestor_with::<ComponentCamera>(c);
        assert_eq!(found, Some(a));
    }

    // --- Transforms ---

    #[test]
    fn local_transform() {
        let mut scene = test_scene();
        let go = scene.create(None, None);

        let matrix = glm::translation(&Vec3::new(1.0, 2.0, 3.0));
        scene.set_transform(go, &matrix);

        let t = scene.transform(go);
        assert!((t.position - Vec3::new(1.0, 2.0, 3.0)).norm() < 1e-5);
    }

    #[test]
    fn world_transform_inherits_parent() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));

        scene.set_transform(parent, &glm::translation(&Vec3::new(10.0, 0.0, 0.0)));
        scene.set_transform(child, &glm::translation(&Vec3::new(0.0, 5.0, 0.0)));

        let world = scene.world_transform(child);
        assert!((world.position - Vec3::new(10.0, 5.0, 0.0)).norm() < 1e-5);
    }

    #[test]
    fn set_world_transform() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));

        scene.set_transform(parent, &glm::translation(&Vec3::new(10.0, 0.0, 0.0)));
        scene.set_world_transform(child, glm::translation(&Vec3::new(10.0, 5.0, 0.0)));

        let local = scene.transform(child);
        assert!((local.position - Vec3::new(0.0, 5.0, 0.0)).norm() < 1e-5);
    }

    #[test]
    fn set_world_transform_preserves_child_scale() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));
        scene.write_component::<crate::component::ComponentTransform, _>(child, |transform| {
            transform.transform.scale = Vec3::new(0.45, 0.45, 0.45);
        });

        let world = scene.world_transform(child);
        scene.set_world_transform(child, world.matrix());

        let local = scene.transform(child);
        assert_eq!(local.scale, Vec3::new(0.45, 0.45, 0.45));
    }

    #[test]
    fn transform_relative_to() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, Some(a));
        let c = scene.create(None, Some(b));

        scene.set_transform(a, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
        scene.set_transform(b, &glm::translation(&Vec3::new(2.0, 0.0, 0.0)));
        scene.set_transform(c, &glm::translation(&Vec3::new(3.0, 0.0, 0.0)));

        let relative = scene.transform_relative_to(c, a);
        assert!((relative.position - Vec3::new(5.0, 0.0, 0.0)).norm() < 1e-5);
    }

    #[test]
    fn clear_transform_cache() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.set_transform(go, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));

        // Populate cache
        let _ = scene.world_transform(go);
        // Clear and verify it still works (recomputes)
        scene.clear_transform_cache();
        let t = scene.world_transform(go);
        assert!((t.position - Vec3::new(1.0, 0.0, 0.0)).norm() < 1e-5);
    }

    #[test]
    fn transform_cache_marks_descendants_dirty() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));

        scene.set_transform(parent, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
        scene.set_transform(child, &glm::translation(&Vec3::new(0.0, 1.0, 0.0)));
        let world = scene.world_transform(child);
        assert!((world.position - Vec3::new(1.0, 1.0, 0.0)).norm() < 1e-5);

        scene.set_transform(parent, &glm::translation(&Vec3::new(5.0, 0.0, 0.0)));
        let world = scene.world_transform(child);
        assert!((world.position - Vec3::new(5.0, 1.0, 0.0)).norm() < 1e-5);
    }

    // --- Components ---

    #[test]
    fn add_and_read_component() {
        let mut scene = test_scene();
        let go = scene.create(None, None);

        scene.add_component(
            go,
            ComponentCamera {
                fov: 90.0,
                ..Default::default()
            },
        );

        let fov = scene.read_component::<ComponentCamera, _, _>(go, |c| c.fov);
        assert_eq!(fov, Some(90.0));
    }

    #[test]
    fn write_component() {
        let mut scene = test_scene();
        let go = scene.create(None, None);

        scene.add_component(go, ComponentCamera::default());
        scene.write_component::<ComponentCamera, _>(go, |c| c.fov = 120.0);

        let fov = scene.read_component::<ComponentCamera, _, _>(go, |c| c.fov);
        assert_eq!(fov, Some(120.0));
    }

    #[test]
    fn read_component_returns_none_when_missing() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let result = scene.read_component::<ComponentCamera, _, _>(go, |c| c.fov);
        assert_eq!(result, None);
    }

    #[test]
    fn bind_component_dyn() {
        let registries = crate::test_utils::test_registries();
        let mut scene = registries.scene();
        let go = scene.create(None, None);

        let camera_uuid = Uuid::parse_str("a85867d2-3e68-42b2-b943-ea78c7c6ddb5").unwrap();
        scene.bind_component_dyn(go, camera_uuid);

        let has_camera = scene.read_component::<ComponentCamera, _, _>(go, |_| true);
        assert_eq!(has_camera, Some(true));
    }

    #[test]
    fn entry_and_entry_mut() {
        let mut scene = test_scene();
        let go = scene.create(None, None);

        // entry (read)
        let entry = scene.entry(go);
        assert!(entry.is_some());
        assert!(entry.unwrap().get_component::<ComponentID>().is_ok());

        // entry_mut (write)
        let entry_mut = scene.entry_mut(go);
        assert!(entry_mut.is_some());
    }

    // --- Camera ---

    #[test]
    fn main_camera() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentCamera::default());

        let camera = scene.main_camera();
        assert!(camera.is_some());
        let (cam_go, _) = camera.unwrap();
        assert_eq!(cam_go, go);
    }

    #[test]
    fn main_camera_returns_none_without_camera() {
        let scene = test_scene();
        assert!(scene.main_camera().is_none());
    }

    // --- Prefab ---

    #[test]
    fn create_and_instantiate_prefab() {
        let registries = crate::test_utils::test_registries();
        let mut scene = registries.scene();

        let parent = scene.create(
            Some(ComponentID {
                name: "PrefabRoot".into(),
                ..Default::default()
            }),
            None,
        );
        scene.create(
            Some(ComponentID {
                name: "PrefabChild".into(),
                ..Default::default()
            }),
            Some(parent),
        );

        let prefab = scene.create_prefab(parent);
        let instance = scene.instantiate_prefab(&prefab, None);
        assert!(instance.is_some());

        let instance_go = instance.unwrap();
        let instance_name = scene.name(instance_go);
        assert_eq!(instance_name, "PrefabRoot");

        // Instance should have a child
        let instance_children: Vec<GameObject> = scene.children(instance_go).collect();
        assert_eq!(instance_children.len(), 1);
        assert_eq!(scene.name(instance_children[0]), "PrefabChild");

        // Instance UUIDs should differ from original
        assert_ne!(scene.uuid(instance_go), scene.uuid(parent));
    }

    // --- Serialization ---

    #[test]
    fn serialization_round_trip_preserves_game_objects() {
        let registries = crate::test_utils::test_registries();
        let mut scene = registries.scene();

        scene.create(
            Some(ComponentID {
                name: "Player".into(),
                ..Default::default()
            }),
            None,
        );
        scene.create(
            Some(ComponentID {
                name: "Enemy".into(),
                ..Default::default()
            }),
            None,
        );

        let data: SceneData = (&scene).into();
        let restored: Scene = (&registries, data).into();

        let names: Vec<String> = restored.objects().map(|go| restored.name(go)).collect();
        assert!(names.contains(&"Player".to_string()));
        assert!(names.contains(&"Enemy".to_string()));
    }

    #[test]
    fn serialization_round_trip_preserves_hierarchy() {
        let registries = crate::test_utils::test_registries();
        let mut scene = registries.scene();

        let parent = scene.create(
            Some(ComponentID {
                name: "Parent".into(),
                ..Default::default()
            }),
            None,
        );
        scene.create(
            Some(ComponentID {
                name: "Child".into(),
                ..Default::default()
            }),
            Some(parent),
        );

        let data: SceneData = (&scene).into();
        let restored: Scene = (&registries, data).into();

        let restored_child = restored
            .objects()
            .find(|go| restored.name(*go) == "Child")
            .expect("Child not found");
        let restored_parent = restored
            .parent(restored_child)
            .expect("Child has no parent");
        assert_eq!(restored.name(restored_parent), "Parent");
    }

    #[test]
    fn serialization_round_trip_preserves_components() {
        let registries = crate::test_utils::test_registries();
        let mut scene = registries.scene();

        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentCamera {
                fov: 42.0,
                near_plane: 0.5,
                far_plane: 500.0,
                ..Default::default()
            },
        );

        let data: SceneData = (&scene).into();
        let restored: Scene = (&registries, data).into();

        let restored_go = restored.objects().next().expect("no game objects");
        assert_eq!(
            restored.read_component::<ComponentCamera, _, _>(restored_go, |c| c.fov),
            Some(42.0)
        );
        assert_eq!(
            restored.read_component::<ComponentCamera, _, _>(restored_go, |c| c.far_plane),
            Some(500.0)
        );
    }

    #[test]
    fn scene_clone_is_equivalent() {
        let registries = crate::test_utils::test_registries();
        let mut scene = registries.scene();

        let parent = scene.create(
            Some(ComponentID {
                name: "A".into(),
                ..Default::default()
            }),
            None,
        );
        scene.create(
            Some(ComponentID {
                name: "B".into(),
                ..Default::default()
            }),
            Some(parent),
        );

        let cloned = scene.clone();

        let original_names: Vec<String> = scene.objects().map(|go| scene.name(go)).collect();
        let cloned_names: Vec<String> = cloned.objects().map(|go| cloned.name(go)).collect();
        assert_eq!(original_names.len(), cloned_names.len());
        for name in &original_names {
            assert!(cloned_names.contains(name));
        }
    }

    #[test]
    fn scene_snapshot_restores_equivalent_scene() {
        let registries = crate::test_utils::test_registries();
        let mut scene = registries.scene();

        let parent = scene.create(
            Some(ComponentID {
                name: "Snapshot Parent".into(),
                ..Default::default()
            }),
            None,
        );
        scene.create(
            Some(ComponentID {
                name: "Snapshot Child".into(),
                ..Default::default()
            }),
            Some(parent),
        );

        let restored = scene.snapshot().into_scene(&registries);

        let restored_parent = restored
            .objects()
            .find(|go| restored.name(*go) == "Snapshot Parent")
            .expect("missing restored parent");
        let restored_child = restored
            .children_ordered(restored_parent)
            .find(|go| restored.name(*go) == "Snapshot Child")
            .expect("missing restored child");

        assert_eq!(restored.parent(restored_child), Some(restored_parent));
    }
}
