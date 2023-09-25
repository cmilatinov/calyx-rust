use reflect::{impl_reflect_value, reflect_trait};
use std::any::{Any, TypeId};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::assets::error::AssetError;
use crate::assets::mesh::Mesh;
use crate::core::Ref;

pub trait Asset: Any + Send + Sync {
    fn get_file_extensions(&self) -> &'static [&'static str] {
        &[""]
    }

    fn load(&mut self, path: &Path) -> Result<(), AssetError>;
}

#[reflect_trait]
pub trait AssetRef {
    fn asset_type_id(&self) -> TypeId;
    fn as_asset(&self) -> Ref<dyn Asset>;
}

impl<T: Asset> AssetRef for Ref<T> {
    fn asset_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
    fn as_asset(&self) -> Ref<dyn Asset> {
        Ref::from_arc(unsafe {
            Arc::from_raw(Arc::into_raw(self.deref().clone()) as *const RwLock<dyn Asset>)
        })
    }
}

impl_reflect_value!(Ref<Mesh>(AssetRef));
