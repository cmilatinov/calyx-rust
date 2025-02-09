use crate::assets::error::AssetError;
use crate::core::Ref;
use serde::de::DeserializeOwned;
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

impl<T: DeserializeOwned> LoadedAsset<T> {
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

impl<T> From<LoadedAsset<T>> for LoadedAssetRef<T> {
    fn from(LoadedAsset { asset, sub_assets }: LoadedAsset<T>) -> Self {
        Self {
            asset: Ref::new(asset),
            sub_assets,
        }
    }
}
