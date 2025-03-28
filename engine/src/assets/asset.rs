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
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::Shader;
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
    pub fn get_ref(&self, assets: &ReadOnlyAssetContext) -> Option<Ref<T>> {
        assets.asset_registry.read().load_by_id(self.id).ok()
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
        let asset_ref = context.asset_registry.read().load_by_id(self.id).ok();
        self.inner = asset_ref.clone();
        asset_ref.map(|r| r.as_asset())
    }

    fn set_asset_ref(&mut self, context: &ReadOnlyAssetContext, asset_id: Option<Uuid>) {
        self.clear_cache();
        self.id = asset_id.unwrap_or_default();
        self.inner = context.asset_registry.read().load_by_id::<T>(self.id).ok();
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
            .asset_registry
            .read()
            .load_by_id::<T>(id)
            .map_err(|err| serde::de::Error::custom(format!("{:?}", err)))
    }
}

impl_extern_type_uuid!(Ref<Mesh>, "fe0cede5-078e-453e-a680-1ff55bb582fc");
impl_extern_type_uuid!(Ref<Shader>, "d07ea11e-60d8-4e51-a4d5-7099b50c0a12");
impl_extern_type_uuid!(Ref<Texture>, "731cd634-75a5-4550-9df8-0cc59cfbbd06");
impl_extern_type_uuid!(Ref<Material>, "86d2370f-aecd-462f-a1f3-9b8068627cd8");
impl_extern_type_uuid!(Ref<Skybox>, "913ec3d7-7078-4e65-a890-d540608eeb6b");
impl_extern_type_uuid!(Ref<Animation>, "8163d22f-e417-475f-bbda-04c9f4389961");
impl_extern_type_uuid!(Ref<AnimationGraph>, "6f0df1ef-9aad-4f61-8731-d60ac608d9fd");

impl_extern_type_uuid!(Option<Ref<Mesh>>, "ccee7bcc-744a-4eee-b1c2-af08dd4f481b");
impl_extern_type_uuid!(Option<Ref<Shader>>, "6f9f1e5a-8f39-4595-98cf-410777321105");
impl_extern_type_uuid!(Option<Ref<Texture>>, "b1c1260e-4c2e-4eff-8d6a-ae53308a6cb0");
impl_extern_type_uuid!(
    Option<Ref<Material>>,
    "8c13be0d-18ee-4add-99a6-368d8adf0440"
);
impl_extern_type_uuid!(Option<Ref<Skybox>>, "ee52ede4-a2ae-42d3-b872-c3c6516d53ef");
impl_extern_type_uuid!(
    Option<Ref<Animation>>,
    "0583bce5-72b1-4157-8361-aa4255e097a6"
);
impl_extern_type_uuid!(
    Option<Ref<AnimationGraph>>,
    "520c2d38-85af-454c-b789-5c0661bcae2f"
);

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

impl_reflect_value!(AssetRef<Mesh>(AssetAccess));
impl_reflect_value!(AssetRef<Shader>(AssetAccess));
impl_reflect_value!(AssetRef<Texture>(AssetAccess));
impl_reflect_value!(AssetRef<Material>(AssetAccess));
impl_reflect_value!(AssetRef<Skybox>(AssetAccess));
impl_reflect_value!(AssetRef<Animation>(AssetAccess));
impl_reflect_value!(AssetRef<AnimationGraph>(AssetAccess));
