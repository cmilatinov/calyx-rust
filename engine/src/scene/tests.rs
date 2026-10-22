#[cfg(test)]
mod tests {
    use crate as engine;
    use crate::component::{Component, ComponentCamera, ComponentID, ReflectComponent};
    use crate::core::Ref;
    use crate::reflect::{Reflect, ReflectDefault};
    use crate::scene::game_object_ref::RemapScope;
    use crate::scene::{GameObject, GameObjectRef, Scene, SceneData, SiblingDir};
    use crate::test_support::test_scene;
    use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
    use nalgebra_glm::{self as glm, Vec3};
    use serde::{Deserialize, Serialize};
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

        scene.set_parent(child, Some(b)).unwrap();
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

        scene
            .set_parent_with_sibling(new_child, Some(parent), Some((c2, SiblingDir::Before)))
            .unwrap();

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

        scene
            .set_parent_with_sibling(new_child, Some(parent), Some((c1, SiblingDir::After)))
            .unwrap();

        let ordered: Vec<GameObject> = scene.children_ordered(parent).collect();
        assert_eq!(ordered, vec![c1, new_child, c2]);
    }

    #[test]
    fn set_parent_rejects_self_parenting() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let go = scene.create(None, Some(parent));

        assert!(scene.set_parent(go, Some(go)).is_err());
        assert_eq!(scene.parent(go), Some(parent));
        assert_eq!(scene.children_ordered(parent).collect::<Vec<_>>(), vec![go]);
    }

    #[test]
    fn set_parent_rejects_grandchild_parent() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, Some(a));
        let c = scene.create(None, Some(b));

        assert!(scene.set_parent(a, Some(c)).is_err());
        assert!(scene
            .set_parent_with_sibling(a, Some(c), Some((c, SiblingDir::Before)))
            .is_err());
        assert_eq!(scene.parent(a), Some(scene.root()));
        assert_eq!(scene.parent(b), Some(a));
        assert_eq!(scene.parent(c), Some(b));
        assert_eq!(scene.children_ordered(a).collect::<Vec<_>>(), vec![b]);
        assert_eq!(scene.children_ordered(b).collect::<Vec<_>>(), vec![c]);
    }

    #[test]
    fn set_parent_with_sibling_moves_within_same_parent() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));
        let c3 = scene.create(None, Some(parent));

        // Forward: c0 before c3 -> c1, c2, c0, c3 (not a swap).
        scene
            .set_parent_with_sibling(c0, Some(parent), Some((c3, SiblingDir::Before)))
            .unwrap();
        assert_eq!(
            scene.children_ordered(parent).collect::<Vec<_>>(),
            vec![c1, c2, c0, c3]
        );

        // Backward: c3 before c1 -> c3, c1, c2, c0.
        scene
            .set_parent_with_sibling(c3, Some(parent), Some((c1, SiblingDir::Before)))
            .unwrap();
        assert_eq!(
            scene.children_ordered(parent).collect::<Vec<_>>(),
            vec![c3, c1, c2, c0]
        );

        // After the last sibling.
        scene
            .set_parent_with_sibling(c3, Some(parent), Some((c0, SiblingDir::After)))
            .unwrap();
        assert_eq!(
            scene.children_ordered(parent).collect::<Vec<_>>(),
            vec![c1, c2, c0, c3]
        );
    }

    #[test]
    fn set_parent_without_sibling_appends_within_same_parent() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));

        scene.set_parent(c0, Some(parent)).unwrap();
        assert_eq!(
            scene.children_ordered(parent).collect::<Vec<_>>(),
            vec![c1, c2, c0]
        );
    }

    #[test]
    fn set_parent_with_unrelated_sibling_appends() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let other = scene.create(None, None);
        let stray = scene.create(None, Some(other));

        scene
            .set_parent_with_sibling(c0, Some(parent), Some((stray, SiblingDir::Before)))
            .unwrap();
        assert_eq!(scene.children_ordered(parent).collect::<Vec<_>>(), vec![c0]);
    }

    #[test]
    fn set_parent_with_self_as_sibling_reparents_to_new_parent() {
        let mut scene = test_scene();
        let old_parent = scene.create(None, None);
        let new_parent = scene.create(None, None);
        let go = scene.create(None, Some(old_parent));
        let existing = scene.create(None, Some(new_parent));

        scene
            .set_parent_with_sibling(go, Some(new_parent), Some((go, SiblingDir::Before)))
            .unwrap();
        assert_eq!(scene.parent(go), Some(new_parent));
        assert_eq!(
            scene.children_ordered(new_parent).collect::<Vec<_>>(),
            vec![existing, go]
        );
        assert_eq!(scene.children(old_parent).count(), 0);
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

    // HashSet drain order is randomized per set, so repeat to hit both orders.
    const DELETE_ORDER_ITERATIONS: usize = 32;

    #[test]
    fn delete_parent_and_child_queued_together() {
        for swap in [false, true] {
            for _ in 0..DELETE_ORDER_ITERATIONS {
                let mut scene = test_scene();
                let keep = scene.create(None, None);
                let parent = scene.create(None, None);
                let child = scene.create(None, Some(parent));
                let grandchild = scene.create(None, Some(child));
                let (parent_uuid, child_uuid) = (scene.uuid(parent), scene.uuid(child));
                let grandchild_uuid = scene.uuid(grandchild);

                if swap {
                    scene.delete(child);
                    scene.delete(parent);
                } else {
                    scene.delete(parent);
                    scene.delete(child);
                }
                scene.delete(grandchild);
                scene.flush_deletes();

                assert!(scene.find(parent_uuid).is_none());
                assert!(scene.find(child_uuid).is_none());
                assert!(scene.find(grandchild_uuid).is_none());
                assert!(scene.find(scene.uuid(keep)).is_some());
            }
        }
    }

    #[test]
    fn delete_twice() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let uuid = scene.uuid(go);

        scene.delete(go);
        scene.delete(go);
        scene.flush_deletes();
        assert!(scene.find(uuid).is_none());

        scene.delete(go);
        scene.flush_deletes();
    }

    #[test]
    fn delete_stale_handle() {
        let mut scene = test_scene();
        let stale = scene.create(None, None);
        let other = scene.create(None, None);
        scene.delete(stale);
        scene.flush_deletes();

        scene.delete(stale);
        scene.flush_deletes();

        assert!(scene.find(scene.uuid(other)).is_some());
    }

    #[test]
    fn delete_root_is_ignored() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let root = scene.root();

        scene.delete(root);
        scene.flush_deletes();

        assert!(scene.find(scene.uuid(go)).is_some());
        assert!(scene.find(scene.root_id()).is_some());
    }

    #[test]
    fn delete_in_cyclic_hierarchy_does_not_hang() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, Some(a));
        let leaf = scene.create(None, Some(b));
        // The scene refuses the edge that would close the cycle.
        assert!(scene.set_parent(a, Some(b)).is_err());

        scene.delete(a);
        scene.delete(leaf);
        scene.flush_deletes();

        assert!(scene.find(scene.uuid(leaf)).is_none());
    }

    #[test]
    fn delete_removes_cycle_when_several_members_are_queued() {
        let mut scene = test_scene();
        let a = scene.create(None, None);
        let b = scene.create(None, Some(a));
        // The scene refuses the edge that would close the cycle.
        assert!(scene.set_parent(a, Some(b)).is_err());

        scene.delete(a);
        scene.delete(b);
        scene.flush_deletes();

        assert!(scene.find(scene.uuid(a)).is_none());
        assert!(scene.find(scene.uuid(b)).is_none());
    }

    #[test]
    fn delete_keeps_sibling_order_contiguous() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));
        let c3 = scene.create(None, Some(parent));

        scene.delete(c1);
        scene.delete(c2);
        scene.flush_deletes();

        let ordered: Vec<GameObject> = scene.children_ordered(parent).collect();
        assert_eq!(ordered, vec![c0, c3]);

        let c4 = scene.create(None, Some(parent));
        let ordered: Vec<GameObject> = scene.children_ordered(parent).collect();
        assert_eq!(ordered, vec![c0, c3, c4]);
        assert_eq!(scene.child_at(parent, 0), Some(c0));
        assert_eq!(scene.child_at(parent, 1), Some(c3));
        assert_eq!(scene.child_at(parent, 2), Some(c4));
    }

    // --- Sibling order characterization ---

    /// Asserts that `parent`'s children are exactly `expected`, in order, and
    /// that their sibling indices are contiguous from zero.
    fn assert_siblings(scene: &Scene, parent: GameObject, expected: &[GameObject]) {
        let ordered: Vec<GameObject> = scene.children_ordered(parent).collect();
        assert_eq!(ordered, expected);
        for (index, child) in expected.iter().enumerate() {
            assert_eq!(scene.child_at(parent, index as i32), Some(*child));
            assert_eq!(
                scene.index_in_parent(parent, *child, SiblingDir::Before),
                Some(index as i32)
            );
            assert_eq!(
                scene.index_in_parent(parent, *child, SiblingDir::After),
                Some(index as i32 + 1)
            );
        }
        assert_eq!(scene.child_at(parent, expected.len() as i32), None);
    }

    #[test]
    fn sibling_order_follows_creation_under_root_and_parent() {
        let mut scene = test_scene();
        let root = scene.root();
        let a = scene.create(None, None);
        let b = scene.create(None, None);
        let a0 = scene.create(None, Some(a));
        let a1 = scene.create(None, Some(a));

        assert_siblings(&scene, root, &[a, b]);
        assert_siblings(&scene, a, &[a0, a1]);
        assert_siblings(&scene, b, &[]);
    }

    #[test]
    fn deleting_a_subtree_closes_the_gap_among_its_siblings() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let c1 = scene.create(None, Some(parent));
        let g0 = scene.create(None, Some(c1));
        let g1 = scene.create(None, Some(c1));
        let c2 = scene.create(None, Some(parent));
        let (g0_uuid, g1_uuid) = (scene.uuid(g0), scene.uuid(g1));

        scene.delete(c1);
        scene.flush_deletes();

        assert!(scene.find(g0_uuid).is_none());
        assert!(scene.find(g1_uuid).is_none());
        assert_siblings(&scene, parent, &[c0, c2]);

        let c3 = scene.create(None, Some(parent));
        assert_siblings(&scene, parent, &[c0, c2, c3]);
    }

    #[test]
    fn deleting_the_first_and_last_child_keeps_the_rest_in_order() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));
        let c3 = scene.create(None, Some(parent));

        scene.delete(c0);
        scene.delete(c3);
        scene.flush_deletes();

        assert_siblings(&scene, parent, &[c1, c2]);
    }

    #[test]
    fn reparenting_closes_the_gap_in_the_old_parent() {
        let mut scene = test_scene();
        let old_parent = scene.create(None, None);
        let new_parent = scene.create(None, None);
        let c0 = scene.create(None, Some(old_parent));
        let c1 = scene.create(None, Some(old_parent));
        let c2 = scene.create(None, Some(old_parent));
        let n0 = scene.create(None, Some(new_parent));

        scene.set_parent(c1, Some(new_parent)).unwrap();

        assert_siblings(&scene, old_parent, &[c0, c2]);
        assert_siblings(&scene, new_parent, &[n0, c1]);

        let c3 = scene.create(None, Some(old_parent));
        assert_siblings(&scene, old_parent, &[c0, c2, c3]);
    }

    #[test]
    fn reparenting_a_subtree_keeps_its_children_in_order() {
        let mut scene = test_scene();
        let old_parent = scene.create(None, None);
        let new_parent = scene.create(None, None);
        let moved = scene.create(None, Some(old_parent));
        let g0 = scene.create(None, Some(moved));
        let g1 = scene.create(None, Some(moved));
        let g2 = scene.create(None, Some(moved));

        scene.set_parent(moved, Some(new_parent)).unwrap();

        assert_siblings(&scene, moved, &[g0, g1, g2]);
        assert_siblings(&scene, new_parent, &[moved]);
        assert_siblings(&scene, old_parent, &[]);
    }

    #[test]
    fn inserting_relative_to_a_sibling_shifts_the_later_siblings() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let c1 = scene.create(None, Some(parent));
        let c2 = scene.create(None, Some(parent));
        let before = scene.create(None, None);
        let after = scene.create(None, None);

        scene
            .set_parent_with_sibling(before, Some(parent), Some((c1, SiblingDir::Before)))
            .unwrap();
        assert_siblings(&scene, parent, &[c0, before, c1, c2]);

        scene
            .set_parent_with_sibling(after, Some(parent), Some((c2, SiblingDir::After)))
            .unwrap();
        assert_siblings(&scene, parent, &[c0, before, c1, c2, after]);

        scene
            .set_parent_with_sibling(c2, Some(parent), Some((c0, SiblingDir::Before)))
            .unwrap();
        assert_siblings(&scene, parent, &[c2, c0, before, c1, after]);
    }

    #[test]
    fn rejected_reparent_leaves_sibling_order_unchanged() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let c0 = scene.create(None, Some(parent));
        let c1 = scene.create(None, Some(parent));
        let g0 = scene.create(None, Some(c1));

        assert!(scene.set_parent_with_sibling(c1, Some(g0), None).is_err());

        assert_siblings(&scene, parent, &[c0, c1]);
        assert_siblings(&scene, c1, &[g0]);
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

    // --- Transform writes keep descendants' world transforms current ---

    /// Builds a parent at x=1 with a child at local y=1 and caches the child's
    /// world transform, so a write that forgets to invalidate the subtree
    /// leaves the child at its stale position.
    fn parent_with_cached_child() -> (Scene, GameObject, GameObject) {
        let mut scene = crate::test_support::test_registries().scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));
        scene.set_transform(parent, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
        scene.set_transform(child, &glm::translation(&Vec3::new(0.0, 1.0, 0.0)));
        assert_child_at(&scene, child, Vec3::new(1.0, 1.0, 0.0));
        (scene, parent, child)
    }

    fn assert_child_at(scene: &Scene, child: GameObject, expected: Vec3) {
        let world = scene.world_transform(child).position;
        assert!(
            (world - expected).norm() < 1e-5,
            "child world position {world:?}, expected {expected:?}"
        );
    }

    #[test]
    fn set_transform_on_parent_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        scene.set_transform(parent, &glm::translation(&Vec3::new(5.0, 0.0, 0.0)));
        assert_child_at(&scene, child, Vec3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn set_world_transform_on_parent_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        scene.set_world_transform(parent, glm::translation(&Vec3::new(5.0, 0.0, 0.0)));
        assert_child_at(&scene, child, Vec3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn write_component_on_parent_transform_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        scene.write_component::<crate::component::ComponentTransform, _>(parent, |c| {
            c.transform.position = Vec3::new(5.0, 0.0, 0.0);
        });
        assert_child_at(&scene, child, Vec3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn add_component_replacing_parent_transform_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        let mut transform = crate::component::ComponentTransform::default();
        transform.transform.position = Vec3::new(5.0, 0.0, 0.0);
        scene.add_component(parent, transform);
        assert_child_at(&scene, child, Vec3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn bind_component_dyn_resetting_parent_transform_moves_child() {
        use crate::utils::TypeUuid;
        let (mut scene, parent, child) = parent_with_cached_child();
        scene.bind_component_dyn(parent, crate::component::ComponentTransform::type_uuid());
        assert_child_at(&scene, child, Vec3::new(0.0, 1.0, 0.0));
    }

    /// Any transform instance serves as the type-erased handle the dynamic
    /// write methods take, as a registry prototype does.
    fn transform_prototype() -> crate::component::ComponentTransform {
        crate::component::ComponentTransform::default()
    }

    #[test]
    fn write_component_dyn_on_parent_transform_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        let prototype = transform_prototype();
        let written = scene.write_component_dyn(parent, &prototype, |instance| {
            let c = instance
                .as_reflect_mut()
                .downcast_mut::<crate::component::ComponentTransform>()
                .expect("instance should be a transform");
            c.transform.position = Vec3::new(5.0, 0.0, 0.0);
        });
        assert!(written.is_some());
        assert_child_at(&scene, child, Vec3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn edit_component_dyn_on_parent_transform_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        let prototype = transform_prototype();
        let edit = unsafe {
            scene.edit_component_dyn(parent, &prototype, |instance, scene| {
                let c = instance
                    .as_reflect_mut()
                    .downcast_mut::<crate::component::ComponentTransform>()
                    .expect("instance should be a transform");
                c.transform.position = Vec3::new(5.0, 0.0, 0.0);
                scene.name(child)
            })
        }
        .expect("the parent should have a transform");
        assert_eq!(edit.value, scene.name(child));
        assert_ne!(edit.before, edit.after);
        assert_child_at(&scene, child, Vec3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn edit_component_dyn_without_a_change_reports_equal_values() {
        let (mut scene, parent, child) = parent_with_cached_child();
        let prototype = transform_prototype();
        let edit = unsafe { scene.edit_component_dyn(parent, &prototype, |_, _| ()) }
            .expect("the parent should have a transform");
        assert_eq!(edit.before, edit.after);
        assert_child_at(&scene, child, Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn remove_component_on_parent_transform_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        scene.remove_component::<crate::component::ComponentTransform>(parent);
        assert_child_at(&scene, child, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn remove_component_dyn_on_parent_transform_moves_child() {
        let (mut scene, parent, child) = parent_with_cached_child();
        use crate::utils::TypeUuid;
        assert!(
            scene.remove_component_dyn(parent, crate::component::ComponentTransform::type_uuid())
        );
        assert_child_at(&scene, child, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn transform_cache_is_not_reused_after_node_is_recycled() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();

        let template = scene.create(None, None);
        scene.set_transform(template, &glm::translation(&Vec3::new(2.0, 0.0, 0.0)));
        let prefab = scene.create_prefab(template);

        let deleted = scene.create(None, None);
        scene.set_transform(deleted, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
        // Populate a clean cache entry for the node that is about to be freed.
        let world = scene.world_transform(deleted);
        assert!((world.position - Vec3::new(1.0, 0.0, 0.0)).norm() < 1e-5);

        scene.delete(deleted);
        scene.flush_deletes();

        let instance = scene
            .instantiate_prefab(&prefab, None)
            .expect("prefab should instantiate");
        assert_eq!(
            instance.node, deleted.node,
            "the new object should reuse the deleted object's node"
        );

        let world = scene.world_transform(instance);
        assert!((world.position - Vec3::new(2.0, 0.0, 0.0)).norm() < 1e-5);
    }

    #[test]
    fn parentless_prefab_root_starts_with_fresh_world_transform() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();

        let template = scene.create(None, None);
        scene.set_transform(template, &glm::translation(&Vec3::new(3.0, 0.0, 0.0)));
        let prefab = scene.create_prefab(template);

        let instance = scene
            .instantiate_prefab(&prefab, None)
            .expect("prefab should instantiate");
        let world = scene.world_transform(instance);
        assert!((world.position - Vec3::new(3.0, 0.0, 0.0)).norm() < 1e-5);
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
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let go = scene.create(None, None);

        let camera_uuid = Uuid::parse_str("a85867d2-3e68-42b2-b943-ea78c7c6ddb5").unwrap();
        scene.bind_component_dyn(go, camera_uuid);

        let has_camera = scene.read_component::<ComponentCamera, _, _>(go, |_| true);
        assert_eq!(has_camera, Some(true));
    }

    #[test]
    fn entry() {
        let mut scene = test_scene();
        let go = scene.create(None, None);

        let entry = scene.entry(go);
        assert!(entry.is_some());
        assert!(entry.unwrap().get_component::<ComponentID>().is_ok());
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
        let registries = crate::test_support::test_registries();
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
        let instance_go = instance.expect("prefab should instantiate");
        let instance_name = scene.name(instance_go);
        assert_eq!(instance_name, "PrefabRoot");

        // Instance should have a child
        let instance_children: Vec<GameObject> = scene.children(instance_go).collect();
        assert_eq!(instance_children.len(), 1);
        assert_eq!(scene.name(instance_children[0]), "PrefabChild");

        // Instance UUIDs should differ from original
        assert_ne!(scene.uuid(instance_go), scene.uuid(parent));
    }

    #[test]
    fn instantiate_prefab_returns_error_for_missing_root() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let root = scene.create(None, None);
        let mut prefab = scene.create_prefab(root);
        prefab.root = Uuid::new_v4();
        let object_count = scene.root_objects().count();

        let result = scene.instantiate_prefab(&prefab, None);

        assert!(matches!(
            result,
            Err(crate::scene::SceneError::InvalidPrefab(_))
        ));
        assert_eq!(scene.root_objects().count(), object_count);
    }

    #[test]
    fn instantiate_prefab_returns_error_for_hierarchy_cycle() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let root = scene.create(None, None);
        let a = scene.create(None, None);
        let b = scene.create(None, None);
        let mut prefab = scene.create_prefab(root);
        for go in [a, b] {
            prefab.data.components.insert(
                scene.uuid(go),
                scene.create_prefab(go).data.components[&scene.uuid(go)].clone(),
            );
        }
        prefab.data.hierarchy.clear();
        prefab
            .data
            .hierarchy
            .insert(scene.uuid(a), vec![scene.uuid(b)]);
        prefab
            .data
            .hierarchy
            .insert(scene.uuid(b), vec![scene.uuid(a)]);
        let object_count = scene.root_objects().count();
        let entity_count = scene.world().len();

        let result = scene.instantiate_prefab(&prefab, None);

        assert!(matches!(
            result,
            Err(crate::scene::SceneError::HierarchyCycle)
        ));
        assert_eq!(scene.root_objects().count(), object_count);
        assert_eq!(scene.world().len(), entity_count);
    }

    #[test]
    fn instantiate_prefab_returns_error_for_hierarchy_entry_outside_prefab() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let root = scene.create(None, None);
        let mut prefab = scene.create_prefab(root);
        prefab
            .data
            .hierarchy
            .insert(scene.uuid(root), vec![Uuid::new_v4()]);
        let entity_count = scene.world().len();

        let result = scene.instantiate_prefab(&prefab, None);

        assert!(matches!(
            result,
            Err(crate::scene::SceneError::InvalidPrefab(_))
        ));
        assert_eq!(scene.world().len(), entity_count);
    }

    #[test]
    fn instantiate_prefab_returns_error_for_disconnected_or_duplicate_parent_objects() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let root = scene.create(None, None);
        let other = scene.create(None, None);
        let (root_id, other_id) = (scene.uuid(root), scene.uuid(other));
        let mut prefab = scene.create_prefab(root);
        prefab.data.components.insert(
            other_id,
            scene.create_prefab(other).data.components[&other_id].clone(),
        );
        let entity_count = scene.world().len();

        let disconnected = scene.instantiate_prefab(&prefab, None);
        prefab
            .data
            .hierarchy
            .insert(root_id, vec![other_id, other_id]);
        let duplicate = scene.instantiate_prefab(&prefab, None);

        assert!(matches!(
            disconnected,
            Err(crate::scene::SceneError::InvalidPrefab(_))
        ));
        assert!(matches!(
            duplicate,
            Err(crate::scene::SceneError::InvalidPrefab(_))
        ));
        assert_eq!(scene.world().len(), entity_count);
    }

    #[test]
    fn instantiate_prefab_returns_error_for_missing_id_component() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let root = scene.create(None, None);
        let mut prefab = scene.create_prefab(root);
        prefab
            .data
            .components
            .get_mut(&prefab.root)
            .unwrap()
            .remove(&<ComponentID as crate::utils::TypeUuid>::type_uuid());
        let entity_count = scene.world().len();

        let result = scene.instantiate_prefab(&prefab, None);

        assert!(matches!(
            result,
            Err(crate::scene::SceneError::InvalidPrefab(_))
        ));
        assert_eq!(scene.world().len(), entity_count);
    }

    // --- Prefab reference remapping ---

    #[derive(Default, Clone, TypeUuid, Serialize, Deserialize, Reflect)]
    #[uuid = "4f0c8a3e-2b71-4d6a-9e15-c7a3b9d20f84"]
    #[reflect(Default)]
    #[repr(C)]
    struct RefSlot {
        target: GameObjectRef,
    }

    #[derive(Default, Clone, TypeUuid, Serialize, Deserialize, Reflect)]
    #[uuid = "8c5d1f2a-3e47-4b9c-a6d0-71f2e9b4c385"]
    #[reflect(Default)]
    #[repr(C)]
    struct RefPair(f32, GameObjectRef);

    #[derive(Default, Clone, TypeUuid, Serialize, Deserialize, Reflect)]
    #[uuid = "e2a64c17-9b3f-4d85-b0c6-5f1e8a7d2c93"]
    enum RefChoice {
        #[default]
        Empty,
        Named {
            target: GameObjectRef,
        },
    }

    /// Component holding references in every shape serde can reach.
    #[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
    #[uuid = "b7d2e915-6c3a-4f08-8a4e-1d9f5c27e6b3"]
    #[reflect(Default, TypeUuidDynamic, Component)]
    #[repr(C)]
    struct NestedRefComponent {
        direct: GameObjectRef,
        slot: RefSlot,
        pair: RefPair,
        choice: RefChoice,
        targets: Vec<GameObjectRef>,
        #[serde(skip)]
        #[reflect_skip]
        transient: GameObjectRef,
    }

    impl Component for NestedRefComponent {}

    /// Builds a prefab whose root holds the component built from the root and
    /// child ids, instantiates it, and returns the instantiated root and child.
    fn instantiate_with_refs(
        scene: &mut Scene,
        component: impl FnOnce(Uuid, Uuid) -> NestedRefComponent,
    ) -> (GameObject, GameObject) {
        let root = scene.create(None, None);
        let child = scene.create(None, Some(root));
        let refs = component(scene.uuid(root), scene.uuid(child));
        scene.add_component(root, refs);
        let prefab = scene.create_prefab(root);
        let instance = scene
            .instantiate_prefab(&prefab, None)
            .expect("prefab should instantiate");
        let instance_child = scene.children(instance).next().unwrap();
        (instance, instance_child)
    }

    fn with_component<R>(
        scene: &Scene,
        game_object: GameObject,
        f: impl FnOnce(&NestedRefComponent) -> R,
    ) -> R {
        let entry = scene.entry(game_object).unwrap();
        f(entry.get_component::<NestedRefComponent>().unwrap())
    }

    fn deserialize_ref(id: Uuid) -> Uuid {
        serde_json::from_value::<GameObjectRef>(serde_json::to_value(id).unwrap())
            .unwrap()
            .id()
    }

    #[test]
    fn instantiate_prefab_remaps_reference_inside_nested_struct() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();

        let (instance, instance_child) =
            instantiate_with_refs(&mut scene, |_, child| NestedRefComponent {
                slot: RefSlot {
                    target: GameObjectRef::new(child),
                },
                ..Default::default()
            });

        let slot_target = with_component(&scene, instance, |c| c.slot.target.id());
        assert_eq!(slot_target, scene.uuid(instance_child));
    }

    #[test]
    fn instantiate_prefab_remaps_references_inside_list() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();

        let (instance, instance_child) =
            instantiate_with_refs(&mut scene, |root, child| NestedRefComponent {
                targets: vec![GameObjectRef::new(child), GameObjectRef::new(root)],
                ..Default::default()
            });

        let targets = with_component(&scene, instance, |c| {
            c.targets.iter().map(GameObjectRef::id).collect::<Vec<_>>()
        });
        assert_eq!(
            targets,
            vec![scene.uuid(instance_child), scene.uuid(instance)]
        );
    }

    #[test]
    fn instantiate_prefab_remaps_references_inside_enum_variant_and_tuple_struct() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();

        let (instance, instance_child) =
            instantiate_with_refs(&mut scene, |root, child| NestedRefComponent {
                direct: GameObjectRef::new(root),
                pair: RefPair(1.0, GameObjectRef::new(child)),
                choice: RefChoice::Named {
                    target: GameObjectRef::new(child),
                },
                ..Default::default()
            });

        let (direct, pair, choice) = with_component(&scene, instance, |c| {
            let RefChoice::Named { target } = &c.choice else {
                panic!("expected the named variant");
            };
            (c.direct.id(), c.pair.1.id(), target.id())
        });
        let new_child = scene.uuid(instance_child);
        assert_eq!(direct, scene.uuid(instance));
        assert_eq!(pair, new_child);
        assert_eq!(choice, new_child);
    }

    #[test]
    fn instantiate_prefab_leaves_nil_and_outside_references_unchanged() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let outside = scene.create(None, None);
        let outside_id = scene.uuid(outside);

        let (instance, _) = instantiate_with_refs(&mut scene, |_, _| NestedRefComponent {
            slot: RefSlot {
                target: GameObjectRef::new(outside_id),
            },
            targets: vec![GameObjectRef::new(outside_id), GameObjectRef::default()],
            ..Default::default()
        });

        let (slot_target, targets, direct) = with_component(&scene, instance, |c| {
            (
                c.slot.target.id(),
                c.targets.iter().map(GameObjectRef::id).collect::<Vec<_>>(),
                c.direct.id(),
            )
        });
        assert_eq!(slot_target, outside_id);
        assert_eq!(targets, vec![outside_id, Uuid::nil()]);
        assert_eq!(direct, Uuid::nil());
    }

    #[test]
    fn instantiate_prefab_does_not_remap_serde_skipped_references() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();

        let (instance, _) = instantiate_with_refs(&mut scene, |_, child| NestedRefComponent {
            transient: GameObjectRef::new(child),
            ..Default::default()
        });

        let transient = with_component(&scene, instance, |c| c.transient.id());
        assert_eq!(transient, Uuid::nil());
    }

    #[test]
    fn remap_scope_ends_after_instantiate_prefab() {
        let registries = crate::test_support::test_registries();
        let mut scene = registries.scene();
        let root = scene.create(None, None);
        let root_id = scene.uuid(root);
        let prefab = scene.create_prefab(root);
        scene.instantiate_prefab(&prefab, None).unwrap();

        assert_eq!(deserialize_ref(root_id), root_id);
    }

    #[test]
    fn remap_scope_restores_the_previous_map_when_nested() {
        let (outer_from, outer_to) = (Uuid::new_v4(), Uuid::new_v4());
        let (inner_from, inner_to) = (Uuid::new_v4(), Uuid::new_v4());

        let outer = RemapScope::enter([(outer_from, outer_to)].into_iter().collect());
        {
            let _inner = RemapScope::enter([(inner_from, inner_to)].into_iter().collect());
            assert_eq!(deserialize_ref(inner_from), inner_to);
            assert_eq!(deserialize_ref(outer_from), outer_from);
        }
        assert_eq!(deserialize_ref(outer_from), outer_to);
        drop(outer);
        assert_eq!(deserialize_ref(outer_from), outer_from);
    }

    // --- Serialization ---

    #[test]
    fn serialization_round_trip_preserves_game_objects() {
        let registries = crate::test_support::test_registries();
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
        let registries = crate::test_support::test_registries();
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
        let registries = crate::test_support::test_registries();
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
        let registries = crate::test_support::test_registries();
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
        let registries = crate::test_support::test_registries();
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

    #[test]
    fn scene_manager_restores_authoring_scene_snapshot() {
        let mut game = crate::test_support::test_game_context();
        game.scenes.current_scene_mut().create(
            Some(ComponentID {
                name: "Kept".into(),
                ..Default::default()
            }),
            None,
        );
        let snapshot = game.scenes.current_scene_snapshot();
        game.scenes.current_scene_mut().create(
            Some(ComponentID {
                name: "Discarded".into(),
                ..Default::default()
            }),
            None,
        );

        game.scenes.start_simulation();
        assert!(game.scenes.has_simulation_scene());
        game.scenes.restore_current_scene_snapshot(snapshot);

        let names = game
            .scenes
            .current_scene()
            .objects()
            .map(|go| game.scenes.current_scene().name(go))
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "Kept"));
        assert!(!names.iter().any(|name| name == "Discarded"));
        assert!(!game.scenes.has_simulation_scene());
    }

    #[test]
    fn scene_manager_starts_configured_scene_without_replacing_authoring_scene() {
        let mut game = crate::test_support::test_game_context();
        game.scenes.current_scene_mut().create(
            Some(ComponentID {
                name: "Authoring".into(),
                ..Default::default()
            }),
            None,
        );
        let mut configured = test_scene();
        configured.create(
            Some(ComponentID {
                name: "Configured".into(),
                ..Default::default()
            }),
            None,
        );

        game.scenes
            .start_simulation_from_scene(Ref::new(configured).readonly());

        let authoring_names = game
            .scenes
            .current_scene()
            .objects()
            .map(|go| game.scenes.current_scene().name(go))
            .collect::<Vec<_>>();
        let simulation_names = game
            .scenes
            .simulation_scene()
            .objects()
            .map(|go| game.scenes.simulation_scene().name(go))
            .collect::<Vec<_>>();
        assert!(authoring_names.iter().any(|name| name == "Authoring"));
        assert!(!authoring_names.iter().any(|name| name == "Configured"));
        assert!(simulation_names.iter().any(|name| name == "Configured"));
    }

    #[test]
    fn scene_manager_resumes_configured_simulation_without_reloading() {
        let mut game = crate::test_support::test_game_context();
        let mut configured = test_scene();
        configured.create(
            Some(ComponentID {
                name: "Configured".into(),
                ..Default::default()
            }),
            None,
        );
        game.scenes
            .start_simulation_from_scene(Ref::new(configured).readonly());
        game.scenes.simulation_scene_mut().create(
            Some(ComponentID {
                name: "Retained".into(),
                ..Default::default()
            }),
            None,
        );
        game.scenes.pause_simulation();

        game.scenes
            .start_simulation_from_scene(Ref::new(test_scene()).readonly());

        assert!(game.scenes.is_simulating());
        assert!(game.scenes.simulation_scene().objects().any(|go| game
            .scenes
            .simulation_scene()
            .name(go)
            == "Retained"));
    }

    #[test]
    fn scene_snapshot_serializes_like_scene() {
        let mut scene = crate::test_support::test_scene();
        scene.create(
            Some(ComponentID {
                name: "Snapshot Object".into(),
                ..Default::default()
            }),
            None,
        );

        let scene_json = serde_json::to_value(&scene).expect("scene should serialize");
        let snapshot_json =
            serde_json::to_value(scene.snapshot()).expect("snapshot should serialize");

        assert_eq!(snapshot_json, scene_json);
    }
}
