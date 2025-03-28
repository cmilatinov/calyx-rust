use crate::assets::error::AssetError;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::utils::ContextSeed;
use serde::de::{DeserializeOwned, DeserializeSeed};
use std::io::BufReader;
use std::path::Path;
use uuid::Uuid;

pub struct LoadedAsset<T> {
    pub asset: T,
    pub sub_assets: Vec<Uuid>,
}

impl<T> LoadedAsset<T> {
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
    pub fn from_json_file_ctx(
        game: &'de ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<T>, AssetError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|_| AssetError::LoadError)?;
        let reader = BufReader::new(file);
        let seed = ContextSeed::<ReadOnlyAssetContext, T>::new(game);
        let mut deserializer = serde_json::Deserializer::from_reader(reader);
        let asset: T = seed
            .deserialize(&mut deserializer)
            .map_err(|_| AssetError::LoadError)?;
        Ok(LoadedAsset::new(asset))
    }
}

impl<T> LoadedAsset<T>
where
    T: DeserializeOwned,
{
    pub fn from_json_file(path: &Path) -> Result<LoadedAsset<T>, AssetError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|_| AssetError::LoadError)?;
        let reader = BufReader::new(file);
        Ok(LoadedAsset::new(
            serde_json::from_reader(reader).map_err(|_| AssetError::LoadError)?,
        ))
    }
}

pub struct LoadedAssetRef<T: ?Sized> {
    pub asset: Ref<T>,
    pub sub_assets: Vec<Uuid>,
}

impl<T> LoadedAssetRef<T> {
    pub fn new(id: Uuid, LoadedAsset { asset, sub_assets }: LoadedAsset<T>) -> Self {
        Self {
            asset: Ref::from_id_value(id, asset),
            sub_assets,
        }
    }
}
