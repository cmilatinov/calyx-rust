use std::any::{Any, TypeId};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use engine_derive::impl_extern_type_uuid;
use engine_derive::{impl_reflect_value, reflect_trait};

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture2D;
use crate::assets::AssetRegistry;
use crate::assets::LoadedAsset;
use crate::core::Ref;
use crate::render::Shader;
use crate::utils::TypeUuid;

pub trait Asset: Any + Send + Sync {
    fn get_file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &[]
    }
    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized;
}

#[reflect_trait]
pub trait AssetRef {
    fn asset_type_uuid(&self) -> Uuid;
    fn as_asset(&self) -> Ref<dyn Asset>;
}

#[reflect_trait]
pub trait AssetOptionRef {
    fn asset_type_uuid(&self) -> Uuid;
    fn as_asset_option(&self) -> Option<Ref<dyn Asset>>;
    fn set(&mut self, asset_ref: Option<Ref<dyn Asset>>);
}

impl<T: Asset + TypeUuid> AssetRef for Ref<T> {
    fn asset_type_uuid(&self) -> Uuid {
        T::type_uuid()
    }
    fn as_asset(&self) -> Ref<dyn Asset> {
        Ref::from(unsafe {
            Arc::from_raw(Arc::into_raw(self.deref().clone()) as *const RwLock<dyn Asset>)
        })
    }
}

impl<T: Asset + TypeUuid> AssetOptionRef for Option<Ref<T>> {
    fn asset_type_uuid(&self) -> Uuid {
        T::type_uuid()
    }
    fn as_asset_option(&self) -> Option<Ref<dyn Asset>> {
        (*self).clone().map(|r| r.as_asset()).into()
    }
    fn set(&mut self, asset_ref: Option<Ref<dyn Asset>>) {
        *self = asset_ref.and_then(|r| r.try_downcast::<T>()).into()
    }
}

impl Ref<dyn Asset> {
    pub fn try_downcast<A: Asset>(&self) -> Option<Ref<A>> {
        if self.read().deref().type_id() == TypeId::of::<A>() {
            Some(Ref::from(unsafe {
                Arc::from_raw(Arc::into_raw(self.deref().clone()) as *const RwLock<A>)
            }))
        } else {
            None
        }
    }
}

impl<T: Asset + TypeUuid> Serialize for Ref<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let id = AssetRegistry::get()
            .asset_id_from_ref(&self.as_asset())
            .unwrap_or_default();
        id.serialize(serializer)
    }
}

impl<'de, T: Asset + TypeUuid> Deserialize<'de> for Ref<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = Uuid::deserialize(deserializer)?;
        Ok(AssetRegistry::get().load_by_id(id).unwrap())
    }
}

impl_extern_type_uuid!(Ref<Mesh>, "fe0cede5-078e-453e-a680-1ff55bb582fc");
impl_extern_type_uuid!(Ref<Shader>, "d07ea11e-60d8-4e51-a4d5-7099b50c0a12");
impl_extern_type_uuid!(Ref<Texture2D>, "731cd634-75a5-4550-9df8-0cc59cfbbd06");
impl_extern_type_uuid!(Ref<Material>, "86d2370f-aecd-462f-a1f3-9b8068627cd8");

impl_reflect_value!(Ref<Mesh>(AssetRef));
impl_reflect_value!(Ref<Shader>(AssetRef));
impl_reflect_value!(Ref<Texture2D>(AssetRef));
impl_reflect_value!(Ref<Material>(AssetRef));

impl_extern_type_uuid!(Option<Ref<Mesh>>, "ccee7bcc-744a-4eee-b1c2-af08dd4f481b");
impl_extern_type_uuid!(Option<Ref<Shader>>, "6f9f1e5a-8f39-4595-98cf-410777321105");
impl_extern_type_uuid!(
    Option<Ref<Texture2D>>,
    "b1c1260e-4c2e-4eff-8d6a-ae53308a6cb0"
);
impl_extern_type_uuid!(
    Option<Ref<Material>>,
    "8c13be0d-18ee-4add-99a6-368d8adf0440"
);

impl_reflect_value!(Option<Ref<Mesh>>(AssetOptionRef));
impl_reflect_value!(Option<Ref<Shader>>(AssetOptionRef));
impl_reflect_value!(Option<Ref<Texture2D>>(AssetOptionRef));
impl_reflect_value!(Option<Ref<Material>>(AssetOptionRef));
