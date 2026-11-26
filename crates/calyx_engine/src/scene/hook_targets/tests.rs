use super::HookTargets;
use crate::component::{ComponentCamera, ComponentMesh};
use crate::scene::GameObject;
use crate::test_support::test_scene;
use legion::storage::ComponentTypeId;

fn camera() -> Option<ComponentTypeId> {
    Some(ComponentTypeId::of::<ComponentCamera>())
}

#[test]
fn visits_only_the_objects_holding_the_component_in_world_order() {
    let mut scene = test_scene();
    let [a, b, c, d] = [(); 4].map(|_| scene.create(None, None));
    scene.add_component(a, ComponentCamera::default());
    scene.add_component(c, ComponentCamera::default());
    let mut targets = HookTargets::new(&scene);

    let mut all = Vec::new();
    targets.visit_holding(&mut scene, None, |_, go| all.push(go));
    assert_eq!(
        all.len(),
        4,
        "no component visits every object but the root"
    );
    assert!([a, b, c, d].iter().all(|go| all.contains(go)));

    let mut visited = Vec::new();
    targets.visit_holding(&mut scene, camera(), |_, go| visited.push(go));
    let in_order: Vec<GameObject> = all.into_iter().filter(|go| [a, c].contains(go)).collect();
    assert_eq!(visited, in_order);
}

#[test]
fn an_object_gaining_or_losing_the_component_mid_pass_is_visited_as_it_then_is() {
    let mut scene = test_scene();
    let [first, lost, gained, passed] = [(); 4].map(|_| scene.create(None, None));
    scene.add_component(first, ComponentCamera::default());
    scene.add_component(lost, ComponentCamera::default());
    // A mesh puts `gained` in an archetype after the cameras', so it
    // comes after `first` in world order; `passed` stays before it.
    scene.add_component(gained, ComponentMesh::default());
    let mut targets = HookTargets::new(&scene);
    let mut order = Vec::new();
    targets.visit_holding(&mut scene, None, |_, go| order.push(go));
    assert_eq!(order, [passed, first, lost, gained]);

    let mut visited = Vec::new();
    targets.visit_holding(&mut scene, camera(), |scene, go| {
        visited.push(go);
        if go == first {
            scene.remove_component::<ComponentCamera>(lost);
            scene.add_component(gained, ComponentCamera::default());
            scene.add_component(passed, ComponentCamera::default());
        }
    });
    assert_eq!(
            visited,
            [first, gained],
            "an object losing the camera before its turn is skipped, one gaining it              before its turn is visited, and one already passed is not visited again"
        );
}
