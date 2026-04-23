use std::any::{Any, TypeId};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, RwLock};

use serde::de::DeserializeSeed;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use engine_derive::reflect_trait;
use engine_derive::{impl_extern_type_uuid, impl_reflect_value};

use crate as engine;
use crate::assets::animation_graph::AnimationGraph;
use crate::assets::error::AssetError;
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture;
use crate::assets::LoadedAsset;
use crate::context::{ReadOnlyAssetContext, ReadOnlyRegistryContext};
use crate::core::Ref;
use crate::render::Shader;
use crate::scene::Prefab;
use crate::utils::{ContextSeed, TypeUuid};

use super::animation::Animation;
use super::skybox::Skybox;

pub type AssetId = Uuid;

pub trait Asset: Any + Send + Sync {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        std::any::type_name::<Self>()
    }
    fn file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &[]
    }
    fn from_file(
        assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized;
    fn to_file(&self, _path: &Path) -> Result<(), std::io::Error> {
        Ok(())
    }
}

impl<T: Asset + TypeUuid> Ref<T> {
    pub fn as_asset(&self) -> Ref<dyn Asset> {
        let inner =
            unsafe { Arc::from_raw(Arc::into_raw(self.inner.clone()) as *const RwLock<dyn Asset>) };
        Ref { id: self.id, inner }
    }
}

pub struct AssetRef<T: Asset + TypeUuid> {
    id: Uuid,
    inner: Option<Ref<T>>,
}

impl<T: Asset + TypeUuid> From<Option<Ref<T>>> for AssetRef<T> {
    fn from(value: Option<Ref<T>>) -> Self {
        Self {
            id: value.clone().map(|r| r.id()).unwrap_or_default(),
            inner: value,
        }
    }
}

impl<T: Asset + TypeUuid> Default for AssetRef<T> {
    fn default() -> Self {
        Self {
            id: Uuid::nil(),
            inner: None,
        }
    }
}

impl<T: Asset + TypeUuid> Clone for AssetRef<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            inner: None,
        }
    }
}

impl<T: Asset + TypeUuid> Serialize for AssetRef<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Uuid::serialize(&self.id, serializer)
    }
}

impl<'de, T: Asset + TypeUuid> Deserialize<'de> for AssetRef<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = Uuid::deserialize(deserializer)?;
        Ok(Self { id, inner: None })
    }
}

impl<T: Asset + TypeUuid> AssetRef<T> {
    pub fn from_id(id: Uuid) -> AssetRef<T> {
        Self { id, inner: None }
    }

    pub fn get_ref(&self, context: &ReadOnlyRegistryContext) -> Option<Ref<T>> {
        context.assets.read().load_by_id(self.id).ok()
    }
}

#[reflect_trait]
pub trait AssetAccess: Any + Send + Sync {
    fn asset_type_uuid(&self) -> Uuid;
    fn clear_cache(&mut self);
    fn id(&self) -> Uuid;
    fn id_mut(&mut self) -> &mut Uuid;
    fn get_asset_ref(&mut self, context: &ReadOnlyAssetContext) -> Option<Ref<dyn Asset>>;
    fn set_asset_ref(&mut self, context: &ReadOnlyAssetContext, asset_id: Option<Uuid>);
}

impl<T: Asset + TypeUuid> AssetAccess for AssetRef<T> {
    fn asset_type_uuid(&self) -> Uuid {
        T::type_uuid()
    }

    fn clear_cache(&mut self) {
        self.inner.take();
    }

    fn id(&self) -> Uuid {
        self.id
    }

    fn id_mut(&mut self) -> &mut Uuid {
        &mut self.id
    }

    fn get_asset_ref(&mut self, context: &ReadOnlyAssetContext) -> Option<Ref<dyn Asset>> {
        let asset_ref = context.registries.assets.read().load_by_id(self.id).ok();
        self.inner = asset_ref.clone();
        asset_ref.map(|r| r.as_asset())
    }

    fn set_asset_ref(&mut self, context: &ReadOnlyAssetContext, asset_id: Option<Uuid>) {
        self.clear_cache();
        self.id = asset_id.unwrap_or_default();
        self.inner = context
            .registries
            .assets
            .read()
            .load_by_id::<T>(self.id)
            .ok();
    }
}

impl Ref<dyn Asset> {
    pub fn try_downcast<A: Asset>(&self) -> Option<Ref<A>> {
        if self.read().deref().type_id() == TypeId::of::<A>() {
            let inner =
                unsafe { Arc::from_raw(Arc::into_raw(self.inner.clone()) as *const RwLock<A>) };
            Some(Ref {
                id: self.id(),
                inner,
            })
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
        self.id().serialize(serializer)
    }
}

impl<'de, T: Asset + TypeUuid> DeserializeSeed<'de>
    for ContextSeed<'de, ReadOnlyAssetContext, Ref<T>>
{
    type Value = Ref<T>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = Uuid::deserialize(deserializer)?;
        self.context
            .registries
            .assets
            .read()
            .load_by_id::<T>(id)
            .map_err(|err| serde::de::Error::custom(format!("{:?}", err)))
    }
}

impl_extern_type_uuid!(AssetRef<Mesh>, "082319aa-393d-4630-a9fb-470ed6d030b8");
impl_extern_type_uuid!(AssetRef<Shader>, "c8689617-519a-4e5e-8b47-50af43c4bb68");
impl_extern_type_uuid!(AssetRef<Texture>, "a3007cc8-f56c-4310-8a57-17d9d8580e56");
impl_extern_type_uuid!(AssetRef<Material>, "eae48303-d3b8-46d3-9647-adad3765a2a2");
impl_extern_type_uuid!(AssetRef<Skybox>, "7ed82d25-fa2a-4705-afa8-7865749b5839");
impl_extern_type_uuid!(AssetRef<Animation>, "8ea7e7bd-2481-4096-a682-9bf5793e0bfa");
impl_extern_type_uuid!(
    AssetRef<AnimationGraph>,
    "3c20b700-e0a1-4a01-bd53-246c1d1a292c"
);
impl_extern_type_uuid!(AssetRef<Prefab>, "6c255f88-b471-421f-8e61-c04a95a918f3");

impl_reflect_value!(AssetRef<Mesh>(AssetAccess));
impl_reflect_value!(AssetRef<Shader>(AssetAccess));
impl_reflect_value!(AssetRef<Texture>(AssetAccess));
impl_reflect_value!(AssetRef<Material>(AssetAccess));
impl_reflect_value!(AssetRef<Skybox>(AssetAccess));
impl_reflect_value!(AssetRef<Animation>(AssetAccess));
impl_reflect_value!(AssetRef<AnimationGraph>(AssetAccess));
impl_reflect_value!(AssetRef<Prefab>(AssetAccess));
