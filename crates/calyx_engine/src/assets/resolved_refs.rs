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
mod tests;
