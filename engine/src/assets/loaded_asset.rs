use uuid::Uuid;

use crate::core::Ref;

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
