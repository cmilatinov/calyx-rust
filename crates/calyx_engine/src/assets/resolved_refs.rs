//! Asset references resolved once and reused while the registry holds still.

use std::any::{Any, TypeId};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::assets::{Asset, AssetAccess, AssetId, AssetRef};
use crate::context::ReadOnlyRegistryContext;
use crate::core::Ref;
use crate::utils::TypeUuid;

/// What a renderer resolved its asset references to, kept from one pass to
/// the next while the registry's [`change_epoch`] holds still.
///
/// Resolving an [`AssetRef`] through the registry takes its locks and clones
/// the asset's metadata, which adds up when every mesh of a large scene does
/// it for its mesh, material and shader on every frame. A pass that resolves
/// through this cache pays that once per asset until the registry changes.
///
/// A pass runs between [`Self::begin`] and [`Self::end`]. `begin` forgets
/// everything once the registry has changed since the last pass, so a
/// reference never outlives a load, reload, rename or deletion that could
/// make it resolve differently, failures included. `end` drops whatever the
/// pass did not resolve, so the cache holds no asset the renderer stopped
/// drawing.
///
/// [`change_epoch`]: crate::assets::AssetRegistry::change_epoch
#[derive(Default)]
pub(crate) struct ResolvedRefs {
    epoch: Option<u64>,
    pass: u64,
    entries: HashMap<(TypeId, AssetId), Resolved>,
}

struct Resolved {
    /// The `Option<Ref<T>>` the registry resolved the id to.
    asset: Box<dyn Any + Send + Sync>,
    /// The last pass that resolved it.
    pass: u64,
}

impl ResolvedRefs {
    /// Starts a pass, forgetting every reference when the registry changed
    /// since the last one.
    pub fn begin(&mut self, registries: &ReadOnlyRegistryContext) {
        let epoch = registries.assets.read().change_epoch();
        if self.epoch != Some(epoch) {
            self.entries.clear();
            self.epoch = Some(epoch);
        }
        self.pass += 1;
    }

    /// Resolves `asset` as [`AssetRef::get_ref`] does, through the registry
    /// only the first time this id is resolved since it last changed.
    pub fn resolve<T: Asset + TypeUuid>(
        &mut self,
        registries: &ReadOnlyRegistryContext,
        asset: &AssetRef<T>,
    ) -> Option<Ref<T>> {
        self.resolve_id(registries, asset.id())
    }

    /// Resolves the asset `id` of type `T`, as [`Self::resolve`] does.
    pub fn resolve_id<T: Asset + TypeUuid>(
        &mut self,
        registries: &ReadOnlyRegistryContext,
        id: AssetId,
    ) -> Option<Ref<T>> {
        let pass = self.pass;
        match self.entries.entry((TypeId::of::<T>(), id)) {
            Entry::Occupied(mut entry) => {
                let entry = entry.get_mut();
                entry.pass = pass;
                entry
                    .asset
                    .downcast_ref::<Option<Ref<T>>>()
                    .cloned()
                    .flatten()
            }
            Entry::Vacant(entry) => {
                let asset = AssetRef::<T>::from_id(id).get_ref(registries);
                entry.insert(Resolved {
                    asset: Box::new(asset.clone()),
                    pass,
                });
                asset
            }
        }
    }

    /// Ends a pass, dropping every reference it did not resolve.
    pub fn end(&mut self) {
        let pass = self.pass;
        self.entries.retain(|_, entry| entry.pass == pass);
    }

    /// How many references the cache holds.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
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
}
