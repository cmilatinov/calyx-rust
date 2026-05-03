use crate::assets::error::AssetError;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::utils::ContextSeed;
use serde::de::{DeserializeOwned, DeserializeSeed};
use std::io::BufReader;
use std::path::Path;
use uuid::Uuid;

/// Asset value loaded from disk together with any discovered sub-assets.
pub struct LoadedAsset<T> {
    /// Loaded asset value.
    pub asset: T,
    /// UUIDs of sub-assets generated or referenced during load.
    pub sub_assets: Vec<Uuid>,
}

impl<T> LoadedAsset<T> {
    /// Wraps an already constructed asset value.
    pub fn new(asset: T) -> LoadedAsset<T> {
        Self {
            asset,
            sub_assets: Default::default(),
        }
    }
}

impl<'de, T> LoadedAsset<T>
where
    ContextSeed<'de, ReadOnlyAssetContext, T>: DeserializeSeed<'de, Value = T>,
{
    /// Loads a JSON asset that requires a [`ReadOnlyAssetContext`] during
    /// deserialization.
    pub fn from_json_file_ctx(
        game: &'de ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<T>, AssetError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|err| {
                AssetError::LoadError
                    .with_path(path)
                    .with_type(std::any::type_name::<T>())
                    .with_source(err)
            })?;
        let reader = BufReader::new(file);
        let seed = ContextSeed::<ReadOnlyAssetContext, T>::new(game);
        let mut deserializer = serde_json::Deserializer::from_reader(reader);
        let asset: T = seed.deserialize(&mut deserializer).map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(std::any::type_name::<T>())
                .with_source(err)
        })?;
        Ok(LoadedAsset::new(asset))
    }
}

impl<T> LoadedAsset<T>
where
    T: DeserializeOwned,
{
    /// Loads a plain JSON asset that does not require context-aware
    /// deserialization.
    pub fn from_json_file(path: &Path) -> Result<LoadedAsset<T>, AssetError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|err| {
                AssetError::LoadError
                    .with_path(path)
                    .with_type(std::any::type_name::<T>())
                    .with_source(err)
            })?;
        let reader = BufReader::new(file);
        Ok(LoadedAsset::new(serde_json::from_reader(reader).map_err(
            |err| {
                AssetError::LoadError
                    .with_path(path)
                    .with_type(std::any::type_name::<T>())
                    .with_source(err)
            },
        )?))
    }
}

/// Shared reference returned by the asset registry after assigning a stable
/// asset UUID.
pub struct LoadedAssetRef<T: ?Sized> {
    /// Shared asset reference.
    pub asset: Ref<T>,
    /// UUIDs of sub-assets generated or referenced during load.
    pub sub_assets: Vec<Uuid>,
}

impl<T> LoadedAssetRef<T> {
    /// Attaches `id` to a freshly loaded asset value.
    pub fn new(id: Uuid, LoadedAsset { asset, sub_assets }: LoadedAsset<T>) -> Self {
        Self {
            asset: Ref::from_id_value(id, asset),
            sub_assets,
        }
    }
}
