use super::*;
use crate::component::{ComponentPointLight, ComponentSkinnedMesh};
use crate::context::ReadOnlyRegistryContext;
use crate::scene::GameObjectRef;
use crate::scene::SceneData;
use crate::test_support::test_registries;
use nalgebra_glm::{self as glm, Vec3};

/// A prefab authored in a scene of its own, a crate with a lit lid, and a
/// level scene holding one linked instance of it moved to x = 3.
struct Fixture {
    registries: ReadOnlyRegistryContext,
    prefab_id: Uuid,
    authoring: Scene,
    body: GameObject,
    level: Scene,
    instance: GameObject,
}

impl Fixture {
    fn new() -> Self {
        let registries = test_registries();
        let mut authoring = registries.scene();
        let body = create(&mut authoring, "Crate", None);
        let lid = create(&mut authoring, "Lid", Some(body));
        authoring.add_component(lid, ComponentPointLight::default());
        let prefab_id = Uuid::new_v4();
        let mut level = registries.scene();
        let (instance, dropped) = level
            .instantiate_linked_prefab(prefab_id, &authoring.create_prefab(body), None)
            .expect("prefab should instantiate");
        assert!(dropped.is_empty());
        level.set_transform(instance, &glm::translation(&Vec3::new(3.0, 0.0, 0.0)));
        Self {
            registries,
            prefab_id,
            authoring,
            body,
            level,
            instance,
        }
    }

    fn prefab(&self) -> Prefab {
        self.authoring.create_prefab(self.body)
    }

    /// The authoring object named `name`.
    fn source(&self, name: &str) -> GameObject {
        named(&self.authoring, name)
    }

    /// The level object named `name`.
    fn target(&self, name: &str) -> GameObject {
        named(&self.level, name)
    }

    fn update(&mut self) -> PrefabUpdate {
        let prefab = self.prefab();
        self.level
            .update_prefab_instances(self.prefab_id, &prefab)
            .expect("instances should update")
    }
}

fn create(scene: &mut Scene, name: &str, parent: Option<GameObject>) -> GameObject {
    let id = ComponentID {
        name: name.into(),
        ..Default::default()
    };
    scene.create(Some(id), parent)
}

fn named(scene: &Scene, name: &str) -> GameObject {
    scene
        .objects()
        .find(|object| scene.name(*object) == name)
        .unwrap_or_else(|| panic!("{name} should exist"))
}

fn names(scene: &Scene) -> Vec<String> {
    let mut names = scene
        .objects()
        .map(|object| scene.name(object))
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn ids(scene: &Scene) -> BTreeSet<Uuid> {
    scene.objects().map(|object| scene.uuid(object)).collect()
}

fn light(scene: &Scene, game_object: GameObject) -> Option<(f32, f32)> {
    scene.read_component::<ComponentPointLight, _, _>(game_object, |light| {
        (light.radius, light.intensity)
    })
}

fn set_light(scene: &mut Scene, game_object: GameObject, radius: f32, intensity: f32) {
    scene
        .write_component::<ComponentPointLight, _>(game_object, |light| {
            light.radius = radius;
            light.intensity = intensity;
        })
        .expect("object should have a light");
}

#[test]
fn instances_record_their_prefab_objects_and_keep_the_links_when_saved() {
    let fixture = Fixture::new();
    let prefab = fixture.prefab();
    let level = &fixture.level;

    let root_link = level.prefab_link(fixture.instance).expect("root is linked");
    assert_eq!(root_link.prefab, fixture.prefab_id);
    assert_eq!(root_link.object, prefab.root);
    assert_eq!(
        root_link.objects,
        prefab.data.components.keys().copied().collect()
    );
    let lid_link = level
        .prefab_link(fixture.target("Lid"))
        .expect("lid is linked");
    assert_eq!(
        lid_link.object,
        fixture.authoring.uuid(fixture.source("Lid"))
    );
    assert!(lid_link.objects.is_empty());

    let json = serde_json::to_string(level).expect("scene should serialize");
    let data: SceneData = serde_json::from_str(&json).expect("scene should parse");
    assert_eq!(data.prefab_links.len(), 2);
    let (loaded, _) = Scene::load(&fixture.registries, data);
    for object in level.objects() {
        let loaded_object = loaded.find(level.uuid(object)).unwrap();
        assert_eq!(loaded.prefab_link(loaded_object), level.prefab_link(object));
    }
    assert_eq!(loaded.linked_prefabs(), BTreeSet::from([fixture.prefab_id]));

    // A prefab made from the instance carries no links.
    assert!(level
        .create_prefab(fixture.instance)
        .data
        .prefab_links
        .is_empty());
}

#[test]
fn prefab_edits_reach_the_instance_only_when_it_updates() {
    let mut fixture = Fixture::new();
    let before = ids(&fixture.level);
    let lid = fixture.source("Lid");
    fixture
        .authoring
        .set_transform(lid, &glm::translation(&Vec3::new(0.0, 2.0, 0.0)));
    set_light(&mut fixture.authoring, lid, 4.0, 7.0);
    let body = fixture.body;
    fixture
        .authoring
        .set_transform(body, &glm::translation(&Vec3::new(-5.0, 0.0, 0.0)));

    assert!(
        fixture
            .level
            .transform(fixture.target("Lid"))
            .position
            .norm()
            < 1e-5
    );
    let update = fixture.update();

    assert_eq!(update.instances, 1);
    assert_eq!(update.kept_overrides, 0);
    let level = &fixture.level;
    let lid = fixture.target("Lid");
    assert!((level.transform(lid).position - Vec3::new(0.0, 2.0, 0.0)).norm() < 1e-5);
    assert_eq!(light(level, lid), Some((4.0, 7.0)));
    assert!(
        (level.transform(fixture.instance).position - Vec3::new(3.0, 0.0, 0.0)).norm() < 1e-5,
        "the instance keeps its root transform"
    );
    assert_eq!(ids(level), before, "the instance keeps its object ids");
}

#[test]
fn overrides_survive_an_update() {
    let mut fixture = Fixture::new();
    let lid = fixture.target("Lid");
    // Overrides: the lid's light, a component the instance added, and
    // the root's name.
    set_light(&mut fixture.level, lid, 9.0, 1.0);
    fixture
        .level
        .add_component(fixture.instance, ComponentPointLight::default());
    fixture
        .level
        .write_component::<ComponentID, _>(fixture.instance, |id| id.name = "Big Crate".into());

    let source_lid = fixture.source("Lid");
    set_light(&mut fixture.authoring, source_lid, 2.0, 5.0);
    fixture
        .authoring
        .set_transform(source_lid, &glm::translation(&Vec3::new(0.0, 1.0, 0.0)));
    let update = fixture.update();

    let level = &fixture.level;
    assert_eq!(update.kept_overrides, 2, "the light and the name");
    assert_eq!(
        light(level, lid),
        Some((9.0, 1.0)),
        "the light override stays"
    );
    assert!(
        (level.transform(lid).position - Vec3::new(0.0, 1.0, 0.0)).norm() < 1e-5,
        "the lid's transform, not overridden, follows the prefab"
    );
    assert!(
        light(level, fixture.instance).is_some(),
        "the added light stays"
    );
    assert_eq!(level.name(fixture.instance), "Big Crate");

    // A second update with nothing new changes nothing.
    let before = serde_json::to_value(&fixture.level).unwrap();
    fixture.update();
    assert_eq!(serde_json::to_value(&fixture.level).unwrap(), before);
}

#[test]
fn objects_the_prefab_adds_or_removes_reach_the_instance() {
    let mut fixture = Fixture::new();
    let source_lid = fixture.source("Lid");
    let handle = create(&mut fixture.authoring, "Handle", Some(source_lid));
    create(&mut fixture.authoring, "Knob", Some(handle));
    let update = fixture.update();

    assert_eq!(update.added, 2);
    let level = &fixture.level;
    let level_handle = fixture.target("Handle");
    assert_eq!(level.parent(level_handle), Some(fixture.target("Lid")));
    assert_eq!(level.parent(fixture.target("Knob")), Some(level_handle));
    assert_ne!(level.uuid(level_handle), fixture.authoring.uuid(handle));
    assert_eq!(
        level.prefab_link(level_handle).map(|link| link.object),
        Some(fixture.authoring.uuid(handle))
    );

    // The instance adds a child to the lid and overrides the knob; the
    // prefab then removes the lid with the handle and the knob.
    let level_lid = fixture.target("Lid");
    let marker = create(&mut fixture.level, "Marker", Some(level_lid));
    let knob = fixture.target("Knob");
    fixture
        .level
        .set_transform(knob, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
    fixture.authoring.delete(source_lid);
    fixture.authoring.flush_deletes();
    let update = fixture.update();

    let level = &fixture.level;
    assert_eq!(update.removed, 2, "the lid and the handle go");
    assert_eq!(update.unlinked, 1, "the overridden knob stays");
    assert_eq!(names(level), ["Crate", "Knob", "Marker"]);
    assert_eq!(level.parent(marker), Some(fixture.instance));
    assert_eq!(level.parent(knob), Some(fixture.instance));
    assert!(level.prefab_link(knob).is_none());
}

#[test]
fn a_member_moved_out_of_its_instance_still_updates_where_it_is() {
    let mut fixture = Fixture::new();
    let lid = fixture.target("Lid");
    fixture
        .level
        .set_parent(lid, None)
        .expect("the lid should move to the scene root");
    let lid_parent = fixture.level.parent(lid);
    assert_ne!(lid_parent, Some(fixture.instance));
    let source_lid = fixture.source("Lid");
    set_light(&mut fixture.authoring, source_lid, 2.0, 5.0);
    let handle = create(&mut fixture.authoring, "Handle", Some(source_lid));

    let update = fixture.update();

    let level = &fixture.level;
    assert_eq!(
        light(level, lid),
        Some((2.0, 5.0)),
        "the lid takes the edit"
    );
    assert_eq!(
        level.parent(lid),
        lid_parent,
        "the lid keeps its chosen parent"
    );
    assert_eq!(update.added, 1);
    let level_handle = fixture.target("Handle");
    assert_eq!(level.parent(level_handle), Some(lid));
    assert_eq!(
        level.prefab_link(level_handle).map(|link| link.object),
        Some(fixture.authoring.uuid(handle))
    );
}

#[test]
fn a_reference_to_an_object_the_instance_deleted_does_not_reach_the_prefab_object() {
    let mut fixture = Fixture::new();
    let source_lid = fixture.source("Lid");
    let skin = ComponentSkinnedMesh {
        root_bone: GameObjectRef::new(fixture.authoring.uuid(source_lid)),
        ..Default::default()
    };
    let body = fixture.body;
    fixture.authoring.add_component(body, skin);
    fixture.update();
    let target = |level: &Scene, instance: GameObject| {
        level
            .read_component::<ComponentSkinnedMesh, _, _>(instance, |skin| skin.root_bone.id())
            .expect("the instance has the skinned mesh")
    };
    let lid = fixture.target("Lid");
    assert_eq!(
        target(&fixture.level, fixture.instance),
        fixture.level.uuid(lid)
    );

    fixture.level.delete(lid);
    fixture.level.flush_deletes();
    fixture.update();

    let level = &fixture.level;
    let target_id = target(level, fixture.instance);
    assert_ne!(
        target_id,
        fixture.authoring.uuid(source_lid),
        "the reference does not fall back to the prefab's own object"
    );
    assert!(level.find(target_id).is_none());
    let before = serde_json::to_value(&fixture.level).unwrap();
    fixture.update();
    assert_eq!(serde_json::to_value(&fixture.level).unwrap(), before);
}

#[test]
fn deleting_the_instance_root_unlinks_the_members_that_survive_it() {
    let mut fixture = Fixture::new();
    let lid = fixture.target("Lid");
    fixture
        .level
        .set_parent(lid, None)
        .expect("the lid should move to the scene root");
    fixture.level.delete(fixture.instance);
    fixture.level.flush_deletes();

    let level = &fixture.level;
    assert_eq!(names(level), ["Lid"]);
    assert!(level.prefab_link(lid).is_none(), "the lid is unlinked");
    assert!(
        level.linked_prefabs().is_empty(),
        "the scene holds no instance of the prefab"
    );
}

/// A fixture whose prefab root holds a skinned mesh with the lid as its
/// root bone, updated into the instance.
fn fixture_referencing_the_lid() -> Fixture {
    let mut fixture = Fixture::new();
    let source_lid = fixture.source("Lid");
    let skin = ComponentSkinnedMesh {
        root_bone: GameObjectRef::new(fixture.authoring.uuid(source_lid)),
        ..Default::default()
    };
    let body = fixture.body;
    fixture.authoring.add_component(body, skin);
    fixture.update();
    fixture
}

fn root_bone(fixture: &Fixture) -> Uuid {
    fixture
        .level
        .read_component::<ComponentSkinnedMesh, _, _>(fixture.instance, |skin| skin.root_bone.id())
        .expect("the instance has the skinned mesh")
}

#[test]
fn a_reference_to_an_object_the_prefab_removed_keeps_the_object_the_instance_kept() {
    let mut fixture = fixture_referencing_the_lid();
    let lid = fixture.target("Lid");
    set_light(&mut fixture.level, lid, 9.0, 1.0);
    let source_lid = fixture.source("Lid");
    fixture.authoring.delete(source_lid);
    fixture.authoring.flush_deletes();

    let update = fixture.update();

    assert_eq!(update.unlinked, 1, "the overridden lid stays");
    let lid_id = fixture.level.uuid(lid);
    assert_eq!(root_bone(&fixture), lid_id);
    fixture.update();
    assert_eq!(
        root_bone(&fixture),
        lid_id,
        "a later update keeps the reference"
    );
}

#[test]
fn a_reference_to_an_object_the_prefab_removed_reaches_no_object_once_it_is_deleted() {
    let mut fixture = fixture_referencing_the_lid();
    let source_lid = fixture.source("Lid");
    let source_lid_id = fixture.authoring.uuid(source_lid);
    fixture.authoring.delete(source_lid);
    fixture.authoring.flush_deletes();

    let update = fixture.update();

    assert_eq!(update.removed, 1);
    for _ in 0..2 {
        let target = root_bone(&fixture);
        assert_ne!(target, source_lid_id, "it does not reach the prefab object");
        assert!(fixture.level.find(target).is_none());
        fixture.update();
    }
}

#[test]
fn an_object_the_prefab_adds_under_a_moved_member_of_a_deleted_parent_is_added() {
    let mut fixture = Fixture::new();
    let source_lid = fixture.source("Lid");
    let source_hinge = create(&mut fixture.authoring, "Hinge", Some(source_lid));
    fixture.update();
    let hinge = fixture.target("Hinge");
    fixture
        .level
        .set_parent(hinge, Some(fixture.instance))
        .expect("the hinge should move under the root");
    let lid = fixture.target("Lid");
    fixture.level.delete(lid);
    fixture.level.flush_deletes();
    create(&mut fixture.authoring, "Pin", Some(source_hinge));

    let update = fixture.update();

    assert_eq!(update.added, 1);
    let level = &fixture.level;
    assert_eq!(level.parent(fixture.target("Pin")), Some(hinge));
    assert_eq!(names(level), ["Crate", "Hinge", "Pin"]);
}

#[test]
fn a_prefab_whose_root_changed_fails_the_update_and_changes_nothing() {
    let mut fixture = Fixture::new();
    let pallet = create(&mut fixture.authoring, "Pallet", None);
    let body = fixture.body;
    fixture
        .authoring
        .set_parent(body, Some(pallet))
        .expect("the crate should move under the pallet");
    let source_lid = fixture.source("Lid");
    set_light(&mut fixture.authoring, source_lid, 2.0, 5.0);
    let prefab = fixture.authoring.create_prefab(pallet);
    let before = serde_json::to_value(&fixture.level).unwrap();

    let result = fixture
        .level
        .update_prefab_instances(fixture.prefab_id, &prefab);

    assert!(
        matches!(&result, Err(SceneError::InvalidPrefab(reason)) if reason.contains("root")),
        "{result:?}"
    );
    assert_eq!(serde_json::to_value(&fixture.level).unwrap(), before);
}

#[test]
fn an_object_the_instance_deleted_stays_deleted() {
    let mut fixture = Fixture::new();
    let lid = fixture.target("Lid");
    fixture.level.delete(lid);
    fixture.level.flush_deletes();
    let source_lid = fixture.source("Lid");
    set_light(&mut fixture.authoring, source_lid, 2.0, 5.0);
    create(&mut fixture.authoring, "Handle", Some(source_lid));

    let update = fixture.update();

    assert_eq!(update.added, 0);
    assert_eq!(names(&fixture.level), ["Crate"]);
}

#[test]
fn only_linked_instances_of_the_prefab_update() {
    let mut fixture = Fixture::new();
    let unlinked = fixture
        .level
        .instantiate_prefab(&fixture.prefab(), None)
        .expect("prefab should instantiate");
    let source_lid = fixture.source("Lid");
    set_light(&mut fixture.authoring, source_lid, 2.0, 5.0);

    let prefab = fixture.prefab();
    let other = fixture
        .level
        .update_prefab_instances(Uuid::new_v4(), &prefab)
        .expect("instances should update");
    assert_eq!(other.instances, 0);
    let update = fixture.update();

    assert_eq!(update.instances, 1);
    let unlinked_lid = fixture.level.children(unlinked).next().unwrap();
    assert_eq!(
        light(&fixture.level, unlinked_lid),
        Some((10.0, 1.0)),
        "an unlinked copy keeps its values"
    );
}
