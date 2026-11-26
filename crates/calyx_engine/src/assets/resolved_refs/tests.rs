use super::ResolvedRefs;
use crate::assets::animation_graph::AnimationGraph;
use crate::assets::mesh::Mesh;
use crate::assets::AssetRef;
use crate::test_support::TestAssets;
use uuid::Uuid;

#[test]
fn a_pass_reuses_what_an_unchanged_registry_resolved_and_drops_what_it_skipped() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registries = &context.registries;
    let (cube, sphere) = {
        let registry = registries.assets.read();
        (registry.cube().unwrap(), registry.sphere().unwrap())
    };
    let mut refs = ResolvedRefs::default();

    refs.begin(registries);
    let first = refs.resolve(registries, &AssetRef::<Mesh>::from_id(cube.id()));
    let missing = refs.resolve(registries, &AssetRef::<Mesh>::from_id(Uuid::new_v4()));
    refs.resolve(registries, &AssetRef::<Mesh>::from_id(sphere.id()));
    refs.end();
    assert!(first.is_some_and(|mesh| mesh.id() == cube.id()));
    assert!(missing.is_none());
    let epoch = registries.assets.read().change_epoch();

    refs.begin(registries);
    let again = refs.resolve(registries, &AssetRef::<Mesh>::from_id(cube.id()));
    refs.end();
    assert!(again.is_some_and(|mesh| mesh.id() == cube.id()));
    assert_eq!(
        registries.assets.read().change_epoch(),
        epoch,
        "resolving cached assets changes nothing in the registry"
    );
    assert_eq!(
        refs.len(),
        1,
        "the sphere and the missing id were not resolved"
    );
}

#[test]
fn a_registry_change_makes_the_next_pass_resolve_again() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registries = &context.registries;
    let cube = registries.assets.read().cube().unwrap();
    let mut refs = ResolvedRefs::default();
    refs.begin(registries);
    refs.resolve(registries, &AssetRef::<Mesh>::from_id(cube.id()));
    refs.end();

    let created = registries
        .assets
        .read()
        .create(
            "test/resolved_refs_created".to_string(),
            AnimationGraph::default(),
        )
        .unwrap();
    refs.begin(registries);
    assert_eq!(refs.len(), 0, "the registry changed, so nothing is kept");
    let resolved = refs.resolve(
        registries,
        &AssetRef::<AnimationGraph>::from_id(created.id()),
    );
    refs.end();
    assert!(resolved.is_some_and(|mesh| mesh.id() == created.id()));
}
