use std::any::{Any, TypeId};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, RwLock};

use reflect::{impl_reflect_value, reflect_trait};

use crate::assets::error::AssetError;
use crate::assets::mesh::Mesh;
use crate::core::Ref;

pub trait Asset: Any + Send + Sync {
    fn get_file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &[""]
    }
    fn from_file(path: &Path) -> Result<Self, AssetError>
    where
        Self: Sized;
}

#[reflect_trait]
pub trait AssetRef {
    fn asset_type_id(&self) -> TypeId;
    fn as_asset(&self) -> Ref<dyn Asset>;
}

#[reflect_trait]
pub trait AssetOptionRef {
    fn asset_type_id(&self) -> TypeId;
    fn as_asset_option(&self) -> Option<Ref<dyn Asset>>;
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

impl<T: Asset> AssetOptionRef for Option<Ref<T>> {
    fn asset_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn as_asset_option(&self) -> Option<Ref<dyn Asset>> {
        self.clone().map(|r| {
            Ref::from_arc(unsafe {
                Arc::from_raw(Arc::into_raw(r.deref().clone()) as *const RwLock<dyn Asset>)
            })
        })
    }
}

impl Ref<dyn Asset> {
    pub fn try_downcast<A: Asset>(&self) -> Option<Ref<A>> {
        if self.read().unwrap().deref().type_id() == TypeId::of::<A>() {
            Some(Ref::from_arc(unsafe {
                Arc::from_raw(Arc::into_raw(self.deref().clone()) as *const RwLock<A>)
            }))
        } else {
            None
        }
    }
}

impl_reflect_value!(Ref<Mesh>(AssetRef));
