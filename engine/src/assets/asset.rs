use std::any::{Any, TypeId};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, RwLock};

use reflect::{impl_extern_type_uuid, impl_reflect_value, reflect_trait};

use crate::assets::error::AssetError;
use crate::assets::mesh::Mesh;
use crate::core::{OptionRef, Ref};

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
    fn as_asset_option(&self) -> OptionRef<dyn Asset>;
    fn set(&mut self, asset_ref: OptionRef<dyn Asset>);
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

impl<T: Asset> AssetOptionRef for OptionRef<T> {
    fn asset_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
    fn as_asset_option(&self) -> OptionRef<dyn Asset> {
        self.0.clone().map(|r| r.as_asset()).into()
    }
    fn set(&mut self, asset_ref: OptionRef<dyn Asset>) {
        *self = asset_ref.0.and_then(|r| r.try_downcast::<T>()).into()
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

impl_extern_type_uuid!(Ref<Mesh>, "fe0cede5-078e-453e-a680-1ff55bb582fc");
impl_reflect_value!(Ref<Mesh>(AssetRef));
impl_extern_type_uuid!(OptionRef<Mesh>, "ccee7bcc-744a-4eee-b1c2-af08dd4f481b");
impl_reflect_value!(OptionRef<Mesh>(AssetOptionRef));
