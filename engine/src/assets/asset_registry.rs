use eframe::wgpu;
use glob::glob;
use nalgebra_glm::{vec2, vec3};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use path_absolutize::Absolutize;
use relative_path::{PathExt, RelativePathBuf};
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::assets::animation_graph::AnimationGraph;
use crate::assets::error::AssetError;
use crate::assets::material::{Material, MaterialTexture};
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture;
use crate::assets::Asset;
use crate::assets::LoadedAssetRef;
use crate::class_registry::ComponentRegistry;
use crate::component::ComponentMesh;
use crate::context::{ReadOnlyAssetContext, ReadOnlyRegistryContext};
use crate::core::{ReadOnlyRef, Ref, WeakRef};
use crate::error::BoxedError;
use crate::input::ActionMap;
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{AttributeValue, TypeInfo};
use crate::render::{RenderContext, Shader, ShaderPreprocessor};
use crate::scene::{Prefab, Scene};
use crate::utils;
use crate::utils::TypeUuid;

use super::skybox::Skybox;
use super::LoadedAsset;

type AssetConstructor = Box<
    dyn Fn(ReadOnlyAssetContext, Uuid, &Path) -> Result<LoadedAssetRef<dyn Asset>, AssetError>
        + Send
        + Sync,
>;
type AssetReload = Box<
    dyn Fn(
            ReadOnlyAssetContext,
            &Ref<dyn Asset>,
            &Path,
        ) -> Result<LoadedAssetRef<dyn Asset>, AssetError>
        + Send
        + Sync,
>;
type AssetCache = HashMap<Uuid, Ref<dyn Asset>>;
type ColorTextureCache = HashMap<[u8; 4], Ref<Texture>>;

const HOT_RELOAD_DEBOUNCE: Duration = Duration::from_millis(75);

/// Serialized metadata for one asset entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    /// Stable asset UUID.
    pub id: Uuid,
    /// Canonical asset name relative to an asset root.
    pub name: String,
    /// User-facing display name.
    pub display_name: String,
    /// Asset type UUID resolved from the file extension.
    pub type_uuid: Uuid,
    #[serde(skip)]
    /// Parent asset UUID when this metadata entry describes a sub-asset.
    pub parent: Option<Uuid>,
    #[serde(skip)]
    /// Child asset UUIDs when this entry owns sub-assets.
    pub children: Vec<Uuid>,
    #[serde(skip)]
    /// Absolute source path for the asset file.
    pub path: Option<PathBuf>,
}

/// On-disk metadata bundle for one asset file and its sub-assets.
#[derive(Serialize, Deserialize)]
pub struct AssetMetaData {
    /// Primary asset metadata entry.
    main: AssetMeta,
    /// Metadata for any sub-assets stored under the main asset.
    inner: Vec<AssetMeta>,
}

#[derive(Default)]
struct AssetData {
    meta: HashMap<Uuid, AssetMeta>,
    names: HashMap<RelativePathBuf, Uuid>,
    extensions: HashMap<String, (TypeId, Uuid, &'static str)>,
    dirty: HashMap<Uuid, Instant>,
    dependencies: HashMap<PathBuf, HashSet<Uuid>>,
    reload_errors: Vec<AssetReloadError>,
}

struct AssetConstructors {
    create: AssetConstructor,
    reload: AssetReload,
}

/// Error captured while hot-reloading an already loaded asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetReloadError {
    /// Asset UUID that failed to reload.
    pub id: Uuid,
    /// Source path that triggered the reload failure.
    pub path: PathBuf,
    /// Underlying reload error.
    pub error: AssetError,
}

/// Central asset registry for metadata, loading, caching, and hot reload.
pub struct AssetRegistry {
    render_context: Arc<RenderContext>,
    asset_registry: WeakRef<AssetRegistry>,
    type_registry: Ref<TypeRegistry>,
    component_registry: Ref<ComponentRegistry>,
    asset_paths: Vec<PathBuf>,
    asset_cache: RwLock<AssetCache>,
    color_texture_cache: RwLock<ColorTextureCache>,
    asset_data: RwLock<AssetData>,
    asset_constructors: RwLock<HashMap<Uuid, AssetConstructors>>,
    watcher_thread: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    watcher: RecommendedWatcher,
}

impl AssetRegistry {
    /// Creates a full asset registry rooted at `root_path` and the shared
    /// workspace `assets` directory.
    pub fn new(
        root_path: impl Into<PathBuf>,
        render_context: Arc<RenderContext>,
        type_registry: Ref<TypeRegistry>,
        component_registry: Ref<ComponentRegistry>,
    ) -> Result<Ref<Self>, BoxedError> {
        let (tx, watcher_rx) = std::sync::mpsc::channel();
        let path = dunce::canonicalize(root_path.into()).map_err(Box::new)?;
        let assets_path = std::env::current_dir().map_err(Box::new)?.join("assets");
        let assets_path = dunce::canonicalize(assets_path).map_err(Box::new)?;
        let asset_paths = [path.clone(), assets_path];
        let mut watcher = RecommendedWatcher::new(tx, Config::default()).map_err(Box::new)?;
        for path in asset_paths.iter() {
            log::info!("Watching asset root {}", path.display());
            watcher
                .watch(path, RecursiveMode::Recursive)
                .map_err(Box::new)?;
        }
        let registry_ref = Ref::new_cyclic(|weak| {
            let mut registry: Self = Self {
                render_context,
                asset_registry: weak,
                type_registry,
                component_registry,
                asset_paths: asset_paths.into(),
                asset_cache: Default::default(),
                color_texture_cache: Default::default(),
                asset_data: Default::default(),
                asset_constructors: Default::default(),
                watcher_thread: None,
                watcher,
            };
            registry.register_default_asset_types();
            registry
                .build_meta()
                .expect("failed to build asset metadata");
            registry
        });
        let watcher_registry_ref = registry_ref.clone();
        registry_ref.write().watcher_thread = Some(std::thread::spawn(move || {
            for event in watcher_rx.iter().flatten() {
                watcher_registry_ref.read().recv_notify_event(event);
            }
        }));
        Ok(registry_ref)
    }
}

impl AssetRegistry {
    /// Creates a test registry with one asset root and no file-watcher thread.
    pub fn new_test(
        root_path: impl Into<PathBuf>,
        render_context: Arc<RenderContext>,
        type_registry: Ref<TypeRegistry>,
        component_registry: Ref<ComponentRegistry>,
    ) -> Ref<Self> {
        let path = root_path.into();
        let (tx, _rx) = std::sync::mpsc::channel();
        let watcher =
            RecommendedWatcher::new(tx, Config::default()).expect("failed to create watcher");
        Ref::new_cyclic(|weak| Self {
            render_context,
            asset_registry: weak,
            type_registry,
            component_registry,
            asset_paths: vec![path],
            asset_cache: Default::default(),
            color_texture_cache: Default::default(),
            asset_data: Default::default(),
            asset_constructors: Default::default(),
            watcher_thread: None,
            watcher,
        })
    }
}

impl AssetRegistry {
    /// Creates a test registry backed by multiple asset roots.
    pub fn new_test_with_assets(
        asset_paths: Vec<PathBuf>,
        render_context: Arc<RenderContext>,
        type_registry: Ref<TypeRegistry>,
        component_registry: Ref<ComponentRegistry>,
    ) -> Ref<Self> {
        let (tx, _rx) = std::sync::mpsc::channel();
        let watcher =
            RecommendedWatcher::new(tx, Config::default()).expect("failed to create watcher");
        let registry = Ref::new_cyclic(|weak| {
            let mut r = Self {
                render_context,
                asset_registry: weak,
                type_registry,
                component_registry,
                asset_paths,
                asset_cache: Default::default(),
                color_texture_cache: Default::default(),
                asset_data: Default::default(),
                asset_constructors: Default::default(),
                watcher_thread: None,
                watcher,
            };
            r.register_default_asset_types();
            r.build_meta().expect("failed to build asset metadata");
            r
        });
        registry
    }
}
impl AssetRegistry {
    fn register_default_asset_types(&mut self) {
        self.register_asset_type::<Mesh>();
        self.register_asset_type::<Shader>();
        self.register_asset_type::<Texture>();
        self.register_asset_type::<Material>();
        self.register_asset_type::<Prefab>();
        self.register_asset_type::<Scene>();
        self.register_asset_type::<Skybox>();
        self.register_asset_type::<AnimationGraph>();
        self.register_asset_type::<ActionMap>();
    }
}

impl AssetRegistry {
    /// Returns the primary project asset root.
    pub fn root_path(&self) -> &PathBuf {
        &self.asset_paths[0]
    }

    /// Returns all asset roots searched by the registry.
    pub fn asset_paths(&self) -> &Vec<PathBuf> {
        &self.asset_paths
    }

    /// Writes an asset value to `path` as pretty-printed JSON.
    pub fn write_to_file<A: Asset + Serialize>(
        asset: &A,
        path: &Path,
    ) -> Result<(), std::io::Error> {
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)
            .and_then(|file| {
                let writer = BufWriter::new(file);
                serde_json::to_writer_pretty(writer, asset).map_err(|e| e.into())
            })
    }

    /// Persists a loaded asset back to its source file when possible.
    pub fn persist(&self, id: Uuid) -> bool {
        let Some(AssetMeta {
            path: Some(asset_path),
            ..
        }) = self.asset_meta_from_id(id)
        else {
            return false;
        };
        let asset_cache = self.asset_cache();
        let Some(asset_ref) = asset_cache.get(&id).cloned() else {
            return false;
        };
        let result = asset_ref.read().to_file(&asset_path);
        result.is_ok()
    }

    /// Loads an asset by canonical asset name.
    pub fn load<A: Asset + TypeUuid>(&self, name: &str) -> Result<Ref<A>, AssetError> {
        let id = self
            .asset_id(name)
            .ok_or_else(|| AssetError::NotFound.with_source(format!("asset name `{name}`")))?;
        self.load_by_id(id)
    }

    /// Loads an asset by filesystem path.
    pub fn load_by_path<A: Asset + TypeUuid>(&self, path: &Path) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id_from_path(path).ok_or_else(|| {
            AssetError::NotFound
                .with_path(path)
                .with_type(A::asset_name())
        })?;
        self.load_by_id(id)
    }

    /// Returns a typed cached asset without loading it from disk.
    pub fn loaded_by_id<A: Asset + TypeUuid>(&self, id: Uuid) -> Option<Ref<A>> {
        self.asset_cache()
            .get(&id)
            .and_then(|asset| asset.try_downcast::<A>())
    }

    /// Reloads an asset by filesystem path, replacing the cached value.
    pub fn reload_by_path<A: Asset + TypeUuid>(&self, path: &Path) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id_from_path(path).ok_or_else(|| {
            AssetError::NotFound
                .with_path(path)
                .with_type(A::asset_name())
        })?;
        self.reload_by_id(id)
    }

    /// Loads an asset by filesystem path as a type-erased handle.
    pub fn load_dyn_by_path(&self, path: &Path) -> Result<Ref<dyn Asset>, AssetError> {
        let id = self
            .asset_id_from_path(path)
            .ok_or_else(|| AssetError::NotFound.with_path(path))?;
        self.load_dyn_by_id(id)
    }

    /// Loads an asset by UUID as a typed handle.
    pub fn load_by_id<A: Asset + TypeUuid>(&self, id: Uuid) -> Result<Ref<A>, AssetError> {
        // Load parent asset if any
        let meta = self
            .asset_meta_from_id(id)
            .ok_or_else(|| AssetError::NotFound.with_source(format!("asset id {id}")))?;
        if let Some(parent_id) = meta.parent {
            self.load_dyn_by_id(parent_id)?;
        }

        // Asset already loaded
        if let Some(asset_ref) = self
            .asset_cache()
            .get(&id)
            .and_then(|a| a.try_downcast::<A>())
        {
            return Ok(asset_ref);
        }

        // Load from file
        let path = self
            .asset_path(id, A::file_extensions())
            .ok_or_else(|| AssetError::NotFound.with_type(A::asset_name()))?;
        log::trace!(
            "Loading asset {} ({}) from {}",
            id,
            A::asset_name(),
            path.display()
        );
        let asset = self.load_asset_file(id, &path)?;

        // Create ref
        self.asset_cache_mut().insert(id, asset.as_asset());
        log::trace!("Loaded asset {} from {}", id, path.display());
        Ok(asset)
    }

    /// Reloads an asset by UUID as a typed handle, replacing the cached value.
    pub fn reload_by_id<A: Asset + TypeUuid>(&self, id: Uuid) -> Result<Ref<A>, AssetError> {
        log::trace!("Reloading asset {} ({})", id, A::asset_name());
        let meta = self
            .asset_meta_from_id(id)
            .ok_or_else(|| AssetError::NotFound.with_source(format!("asset id {id}")))?;
        if let Some(parent_id) = meta.parent {
            self.load_dyn_by_id(parent_id)?;
        }

        let path = self
            .asset_path(id, A::file_extensions())
            .ok_or_else(|| AssetError::NotFound.with_type(A::asset_name()))?;
        let asset = self.load_asset_file(id, &path)?;

        self.asset_cache_mut().insert(id, asset.as_asset());
        self.update_asset_dependencies(id, &path);
        self.clear_reload_error(id);
        log::info!("Reloaded asset {} from {}", id, path.display());
        Ok(asset)
    }

    /// Loads an asset by UUID as a type-erased handle.
    pub fn load_dyn_by_id(&self, id: Uuid) -> Result<Ref<dyn Asset>, AssetError> {
        // Load parent asset if any
        let meta = self
            .asset_meta_from_id(id)
            .ok_or_else(|| AssetError::NotFound.with_source(format!("asset id {id}")))?;
        if let Some(parent_id) = meta.parent {
            self.load_dyn_by_id(parent_id)?;
        }

        // Asset already loaded
        if let Some(asset_ref) = self.asset_cache().get(&id) {
            return Ok((*asset_ref).clone());
        }

        // Find constructor & file path
        let path = meta.path.as_ref().ok_or_else(|| {
            AssetError::NotFound.with_source(format!("path for asset `{}`", meta.name))
        })?;
        let ctors = self.asset_constructors();
        let ctor = ctors.get(&meta.type_uuid).ok_or_else(|| {
            AssetError::NotFound.with_source(format!("constructor for type {}", meta.type_uuid))
        })?;
        log::trace!("Loading dynamic asset {} from {}", id, path.display());
        let LoadedAssetRef { asset, sub_assets } = (ctor.create)(self.asset_context(), id, path)?;

        // Load from file
        self.load_sub_asset_meta(id, sub_assets);
        self.asset_cache_mut().insert(id, asset.clone());
        log::trace!("Loaded dynamic asset {} from {}", id, path.display());
        Ok(asset)
    }

    /// Creates a new in-memory asset entry with the provided `name`.
    pub fn create<A: Asset + TypeUuid>(
        &self,
        name: String,
        value: A,
    ) -> Result<Ref<A>, AssetError> {
        if self.asset_id(name.as_str()).is_some() {
            return Err(AssetError::AlreadyExists.with_source(format!("asset name `{name}`")));
        }
        let asset_name = name.clone();
        let id = utils::uuid_from_str(name.as_str());
        let asset = Ref::from_id_value(id, value);
        self.upsert_in_memory_asset_meta::<A>(id, name);
        self.asset_cache_mut().insert(id, asset.as_asset());
        log::info!("Created in-memory asset {} ({})", asset_name, id);
        Ok(asset)
    }

    /// Creates an in-memory asset or updates an existing cached asset with the
    /// same name and type.
    pub fn create_or_update<A: Asset + TypeUuid>(
        &self,
        name: String,
        value: A,
    ) -> Result<Ref<A>, AssetError> {
        let display_name = self.asset_display_name::<A>(&name);
        let path = RelativePathBuf::from(name.as_str()).normalize();
        let mut data = self.asset_data_mut();
        let id = if let Some(id) = data.names.get(&path).copied() {
            if let Some(meta) = data.meta.get(&id) {
                if meta.path.is_some() || meta.parent.is_some() {
                    return Err(
                        AssetError::AlreadyExists.with_source(format!("asset name `{name}`"))
                    );
                }
                if meta.type_uuid != A::type_uuid() {
                    return Err(AssetError::TypeMismatch
                        .with_type(A::asset_name())
                        .with_source(format!("asset name `{name}`")));
                }
            }
            id
        } else {
            utils::uuid_from_str(name.as_str())
        };

        let mut cache = self.asset_cache_mut();
        if let Some(asset_ref) = cache.get(&id).cloned() {
            let Some(asset_ref) = asset_ref.try_downcast::<A>() else {
                return Err(AssetError::TypeMismatch
                    .with_type(A::asset_name())
                    .with_source(format!("asset name `{name}`")));
            };
            {
                let mut asset = asset_ref.write();
                *asset = value;
            }
            Self::upsert_in_memory_asset_meta_locked::<A>(&mut data, id, name, display_name);
            return Ok(asset_ref);
        }

        let asset = Ref::from_id_value(id, value);
        Self::upsert_in_memory_asset_meta_locked::<A>(&mut data, id, name, display_name);
        cache.insert(id, asset.as_asset());
        Ok(asset)
    }

    /// Creates or refreshes an imported sub-asset owned by `parent_id`.
    pub fn create_or_update_sub_asset<A: Asset + TypeUuid>(
        &self,
        parent_id: Uuid,
        parent_name: &str,
        local_name: &str,
        value: A,
    ) -> Result<Ref<A>, AssetError> {
        let canonical_name = Self::sub_asset_name(parent_name, local_name);
        let display_name = self.asset_display_name::<A>(local_name);
        let canonical_path = RelativePathBuf::from(canonical_name.as_str()).normalize();
        let mut data = self.asset_data_mut();
        let id = if let Some(id) = data.names.get(&canonical_path).copied() {
            if let Some(meta) = data.meta.get(&id) {
                if meta.path.is_some()
                    || meta.parent.is_some_and(|existing| existing != parent_id)
                    || meta.name != local_name
                {
                    return Err(AssetError::AlreadyExists
                        .with_source(format!("asset name `{canonical_name}`")));
                }
                if meta.type_uuid != A::type_uuid() {
                    return Err(AssetError::TypeMismatch
                        .with_type(A::asset_name())
                        .with_source(format!("asset name `{canonical_name}`")));
                }
            }
            id
        } else {
            utils::uuid_from_str(canonical_name.as_str())
        };

        let mut cache = self.asset_cache_mut();
        if let Some(asset_ref) = cache.get(&id).cloned() {
            let Some(asset_ref) = asset_ref.try_downcast::<A>() else {
                return Err(AssetError::TypeMismatch
                    .with_type(A::asset_name())
                    .with_source(format!("asset name `{canonical_name}`")));
            };
            {
                let mut asset = asset_ref.write();
                *asset = value;
            }
            Self::upsert_sub_asset_meta_locked::<A>(
                &mut data,
                id,
                parent_id,
                parent_name,
                local_name,
                display_name,
            );
            return Ok(asset_ref);
        }

        let asset = Ref::from_id_value(id, value);
        Self::upsert_sub_asset_meta_locked::<A>(
            &mut data,
            id,
            parent_id,
            parent_name,
            local_name,
            display_name,
        );
        cache.insert(id, asset.as_asset());
        Ok(asset)
    }

    fn upsert_in_memory_asset_meta<A: Asset + TypeUuid>(&self, id: Uuid, name: String) {
        let display_name = self.asset_display_name::<A>(&name);
        let mut data = self.asset_data_mut();
        Self::upsert_in_memory_asset_meta_locked::<A>(&mut data, id, name, display_name);
    }

    fn upsert_in_memory_asset_meta_locked<A: Asset + TypeUuid>(
        data: &mut AssetData,
        id: Uuid,
        name: String,
        display_name: String,
    ) {
        let existing_meta = data.meta.get(&id).cloned();
        data.names
            .insert(RelativePathBuf::from(name.as_str()).normalize(), id);
        data.meta.insert(
            id,
            AssetMeta {
                id,
                type_uuid: A::type_uuid(),
                display_name,
                name,
                parent: existing_meta.as_ref().and_then(|meta| meta.parent),
                children: existing_meta
                    .as_ref()
                    .map(|meta| meta.children.clone())
                    .unwrap_or_default(),
                path: existing_meta.and_then(|meta| meta.path),
            },
        );
    }

    fn upsert_sub_asset_meta_locked<A: Asset + TypeUuid>(
        data: &mut AssetData,
        id: Uuid,
        parent_id: Uuid,
        parent_name: &str,
        local_name: &str,
        display_name: String,
    ) {
        let canonical_name = Self::sub_asset_name(parent_name, local_name);
        data.names.insert(
            RelativePathBuf::from(canonical_name.as_str()).normalize(),
            id,
        );
        data.meta.insert(
            id,
            AssetMeta {
                id,
                type_uuid: A::type_uuid(),
                display_name,
                name: local_name.to_string(),
                parent: Some(parent_id),
                children: Default::default(),
                path: None,
            },
        );
    }

    fn asset_display_name<A: TypeUuid + 'static>(&self, fallback: &str) -> String {
        let registry = self.type_registry.read();
        registry
            .type_info::<A>()
            .and_then(|info| {
                if let TypeInfo::Struct(info) = info {
                    if let Some(AttributeValue::String(str)) = info.attr("name") {
                        return Some(str.to_string());
                    }
                }
                None
            })
            .unwrap_or_else(|| fallback.to_string())
    }

    /// Loads `name` when it exists, otherwise creates it from `create_fn`.
    pub fn load_or_create<A: Asset + TypeUuid, F: FnOnce() -> A>(
        &self,
        name: &str,
        create_fn: F,
    ) -> Option<Ref<A>> {
        if let Ok(asset) = self.load(name) {
            return asset.into();
        }
        let asset = create_fn();
        self.create(name.into(), asset).ok()
    }

    /// Registers a loadable asset type and its file extensions.
    pub fn register_asset_type<A: Asset + TypeUuid>(&self) {
        let type_uuid = A::type_uuid();
        let mut data = self.asset_data_mut();
        for ext in A::file_extensions() {
            data.extensions.insert(
                String::from(*ext),
                (TypeId::of::<A>(), type_uuid, A::asset_name()),
            );
        }
        self.asset_constructors_mut().insert(
            type_uuid,
            AssetConstructors {
                create: Box::new(|game, id, path| {
                    let loaded_asset = A::from_file(&game, path)?;
                    let LoadedAssetRef { asset, sub_assets } =
                        LoadedAssetRef::new(id, loaded_asset);
                    Ok(LoadedAssetRef {
                        asset: asset.as_asset(),
                        sub_assets,
                    })
                }),
                reload: Box::new(|game, asset_ref, path| {
                    if let Some(asset_ref) = asset_ref.try_downcast::<A>() {
                        let LoadedAsset {
                            asset: loaded_asset,
                            sub_assets,
                        } = A::from_file(&game, path)?;
                        {
                            let mut asset = asset_ref.write();
                            *asset = loaded_asset;
                        }
                        Ok(LoadedAssetRef {
                            asset: asset_ref.as_asset(),
                            sub_assets,
                        })
                    } else {
                        Err(AssetError::TypeMismatch
                            .with_path(path)
                            .with_type(A::asset_name()))
                    }
                }),
            },
        );
    }

    fn load_sub_asset_meta(&self, id: Uuid, sub_assets: Vec<Uuid>) {
        let mut data = self.asset_data_mut();
        if let Some(parent_meta) = data.meta.get(&id) {
            if let Some(path) = &parent_meta.path {
                let meta_path = path.with_extension("meta");
                let meta = AssetMetaData {
                    main: parent_meta.clone(),
                    inner: sub_assets
                        .iter()
                        .filter_map(|id| data.meta.get(id).cloned())
                        .collect(),
                };
                if let Err(err) = self.write_meta_file(&meta_path, &meta) {
                    log::warn!(
                        "Failed to write asset metadata for {}: {}",
                        meta_path.display(),
                        err
                    );
                }
            }
        }
        for child_id in &sub_assets {
            if let Some(child_meta) = data.meta.get_mut(child_id) {
                child_meta.parent = Some(id);
            }
        }
        if let Some(parent_meta) = data.meta.get_mut(&id) {
            parent_meta.children = sub_assets;
        }
    }

    fn load_asset_file<A: Asset>(&self, id: Uuid, path: &Path) -> Result<Ref<A>, AssetError> {
        let loaded_asset = A::from_file(&self.asset_context(), path)?;
        let LoadedAssetRef { asset, sub_assets } = LoadedAssetRef::new(id, loaded_asset);
        self.load_sub_asset_meta(id, sub_assets);
        Ok(asset)
    }

    fn mark_asset_dirty(&self, id: Uuid) {
        self.asset_data_mut().dirty.insert(id, Instant::now());
    }

    fn mark_path_dirty(&self, path: &Path) {
        if let Some(id) = self.asset_id_from_path(path) {
            log::trace!(
                "Marking asset {} dirty after change to {}",
                id,
                path.display()
            );
            self.mark_asset_dirty(id);
        }

        let dependency_path = Self::dependency_path(path);
        let dependent_ids = self
            .asset_data()
            .dependencies
            .get(&dependency_path)
            .cloned()
            .unwrap_or_default();
        for id in dependent_ids {
            log::trace!(
                "Marking dependent asset {} dirty after change to {}",
                id,
                path.display()
            );
            self.mark_asset_dirty(id);
        }
    }
}

impl AssetRegistry {
    fn asset_data(&self) -> RwLockReadGuard<'_, AssetData> {
        self.asset_data.read().unwrap()
    }

    fn asset_data_mut(&self) -> RwLockWriteGuard<'_, AssetData> {
        self.asset_data.write().unwrap()
    }

    fn asset_cache(&self) -> RwLockReadGuard<'_, AssetCache> {
        self.asset_cache.read().unwrap()
    }

    fn asset_cache_mut(&self) -> RwLockWriteGuard<'_, AssetCache> {
        self.asset_cache.write().unwrap()
    }

    fn asset_constructors(&self) -> RwLockReadGuard<'_, HashMap<Uuid, AssetConstructors>> {
        self.asset_constructors.read().unwrap()
    }

    fn asset_constructors_mut(&self) -> RwLockWriteGuard<'_, HashMap<Uuid, AssetConstructors>> {
        self.asset_constructors.write().unwrap()
    }

    fn registry_context(&self) -> ReadOnlyRegistryContext {
        ReadOnlyRegistryContext {
            assets: self.asset_registry.upgrade().unwrap().readonly(),
            types: self.type_registry.readonly(),
            components: self.component_registry.readonly(),
        }
    }

    fn asset_context(&self) -> ReadOnlyAssetContext {
        ReadOnlyAssetContext {
            render_context: self.render_context.clone(),
            registries: self.registry_context(),
        }
    }
}

impl AssetRegistry {
    fn recv_notify_event(&self, event: Event) {
        log::trace!("Received asset notification: {:?}", event.kind);
        let paths_iter = Self::notify_event_paths(&event);
        match event.kind {
            EventKind::Create(_) => {
                for file in paths_iter {
                    if let Err(err) =
                        self.build_asset_meta(self.root_path(), file, &file.with_extension("meta"))
                    {
                        log::warn!(
                            "Failed to build asset metadata for {}: {}",
                            file.display(),
                            err
                        );
                    }
                }
            }
            EventKind::Modify(_) => {
                for file in paths_iter {
                    self.mark_path_dirty(file);
                }
            }
            EventKind::Remove(_) => {
                for file in paths_iter {
                    if file.is_file() {
                        log::trace!(
                            "Treating removal notification for existing path as replacement: {}",
                            file.display()
                        );
                        self.mark_path_dirty(file);
                        continue;
                    }
                    let meta_path = file.with_extension("meta");
                    match std::fs::remove_file(&meta_path) {
                        Ok(()) => log::info!("Removed asset metadata {}", meta_path.display()),
                        Err(err) => log::warn!(
                            "Failed to remove asset metadata {}: {}",
                            meta_path.display(),
                            err
                        ),
                    }
                }
            }
            _ => {}
        }
    }

    fn notify_event_paths(event: &Event) -> impl Iterator<Item = &PathBuf> {
        event
            .paths
            .iter()
            .filter(|f| {
                if let EventKind::Remove(_) = event.kind {
                    true
                } else {
                    f.is_file()
                }
            })
            .filter_map(|f| {
                f.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| (ext, f))
            })
            .filter_map(|(ext, f)| {
                if matches!(ext, "meta" | "tmp") {
                    None
                } else {
                    Some(f)
                }
            })
    }

    /// Returns the asset UUID for `name`, if known.
    pub fn asset_id(&self, name: &str) -> Option<Uuid> {
        let path = RelativePathBuf::from(name).normalize();
        self.asset_data().names.get(&path).copied()
    }

    /// Returns the canonical asset name for `id`.
    pub fn asset_name(&self, id: Uuid) -> String {
        self.asset_meta_from_id(id)
            .map(|meta| meta.name.clone())
            .unwrap_or_default()
    }

    /// Resolves a canonical asset name from a filesystem path.
    pub fn asset_name_from_path(&self, path: &Path) -> Option<String> {
        self.asset_id_from_path(path).map(|id| self.asset_name(id))
    }

    /// Resolves an asset UUID from a filesystem path.
    pub fn asset_id_from_path(&self, path: &Path) -> Option<Uuid> {
        for root_path in &self.asset_paths {
            if common_path::common_path(root_path, path)
                .map(|prefix| prefix == *root_path)
                .unwrap_or(false)
            {
                return path.relative_to(root_path).ok().and_then(|p| {
                    let p = p.with_extension("");
                    self.asset_data().names.get(&p).copied()
                });
            }
        }
        None
    }

    /// Returns metadata for the asset named `name`.
    pub fn asset_meta(&self, name: &str) -> Option<AssetMeta> {
        let id = self.asset_id(name)?;
        self.asset_meta_from_id(id)
    }

    /// Returns metadata for the asset at `path`.
    pub fn asset_meta_from_path(&self, path: &Path) -> Option<AssetMeta> {
        self.asset_id_from_path(path)
            .and_then(|id| self.asset_meta_from_id(id))
    }

    /// Returns metadata for the asset UUID `id`.
    pub fn asset_meta_from_id(&self, id: Uuid) -> Option<AssetMeta> {
        self.asset_data().meta.get(&id).cloned()
    }

    /// Returns metadata for a typed asset reference.
    #[inline]
    pub fn asset_meta_from_ref<A: Asset>(&self, reference: &ReadOnlyRef<A>) -> Option<AssetMeta> {
        self.asset_meta_from_id(reference.id())
    }

    #[inline]
    /// Returns metadata for a type-erased asset reference.
    pub fn asset_meta_from_ref_dyn(&self, reference: &ReadOnlyRef<dyn Asset>) -> Option<AssetMeta> {
        self.asset_meta_from_id(reference.id())
    }

    /// Returns the source path for `id` when its extension matches
    /// `extensions`.
    pub fn asset_path(&self, id: Uuid, extensions: &[&str]) -> Option<PathBuf> {
        let meta = self.asset_meta_from_id(id)?;
        let path = meta.path?;
        let ext = path
            .extension()
            .map_or("", |ext| ext.to_str().unwrap_or(""));
        if extensions.contains(&ext) {
            Some(path)
        } else {
            None
        }
    }

    /// Returns the registered asset type UUID for a file extension.
    pub fn asset_type_uuid_from_ext(&self, ext: &str) -> Option<Uuid> {
        self.asset_data()
            .extensions
            .get(ext)
            .map(|(_, uuid, _)| *uuid)
    }
}

impl AssetRegistry {
    /// Rebuilds metadata for every asset file under every asset root.
    pub fn build_meta(&self) -> Result<(), BoxedError> {
        let mut built_count = 0usize;
        for asset_path in &self.asset_paths {
            for path in
                glob(format!("{}/**/*", asset_path.to_str().unwrap()).as_str()).map_err(Box::new)?
            {
                let path = match path {
                    Ok(path) => path,
                    Err(err) => {
                        log::warn!("Skipping asset path while building metadata: {}", err);
                        continue;
                    }
                };
                let ext = path
                    .extension()
                    .map_or("", |ext| ext.to_str().unwrap_or(""));
                if ext.is_empty() || ext == "meta" || ext == "rs" {
                    continue;
                }
                let meta_path = path.with_extension("meta");
                if let Err(err) = self.build_asset_meta(asset_path, &path, &meta_path) {
                    log::warn!(
                        "Failed to build asset metadata for {}: {}",
                        path.display(),
                        err
                    );
                } else {
                    built_count += 1;
                }
            }
        }
        log::info!("Built metadata for {built_count} assets");
        Ok(())
    }

    fn build_asset_meta(
        &self,
        asset_path: &Path,
        path: &Path,
        meta_path: &Path,
    ) -> Result<(), BoxedError> {
        let mut meta = if meta_path.exists() {
            self.load_meta_file(asset_path, meta_path)?.main
        } else {
            let display_name = path
                .file_stem()
                .and_then(|f| f.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let relative_path = path.relative_to(asset_path).map_err(Box::new)?;
            let meta = AssetMetaData {
                main: AssetMeta {
                    id: utils::uuid_from_str(relative_path.as_str()),
                    type_uuid: Uuid::nil(),
                    name: relative_path.with_extension("").to_string(),
                    display_name,
                    parent: None,
                    children: Default::default(),
                    path: None,
                },
                inner: Default::default(),
            };
            self.write_meta_file(meta_path, &meta)?;
            meta.main
        };
        meta.type_uuid = self
            .asset_type_uuid_from_ext(path.extension().unwrap().to_str().unwrap())
            .unwrap_or(Uuid::nil());
        meta.path = Some(match path.absolutize().map_err(Box::new)? {
            Cow::Borrowed(p) => p.to_path_buf(),
            Cow::Owned(p) => p,
        });
        let id = meta.id;
        let mut data = self.asset_data_mut();
        data.meta.insert(id, meta);
        data.names
            .insert(Self::relative_asset_path(asset_path, path), id);
        drop(data);
        self.update_asset_dependencies(id, path);
        log::trace!("Built asset metadata for {}", path.display());
        Ok(())
    }

    fn update_asset_dependencies(&self, id: Uuid, path: &Path) {
        let mut data = self.asset_data_mut();
        for dependents in data.dependencies.values_mut() {
            dependents.remove(&id);
        }
        data.dependencies
            .retain(|_, dependents| !dependents.is_empty());
        drop(data);

        if path.extension().and_then(|ext| ext.to_str()) != Some("wgsl") {
            return;
        }

        match ShaderPreprocessor::shader_dependencies(self, path) {
            Ok(dependencies) => {
                let mut data = self.asset_data_mut();
                for dependency in dependencies {
                    data.dependencies.entry(dependency).or_default().insert(id);
                }
            }
            Err(err) => log::warn!(
                "Failed to scan shader dependencies for {}: {}",
                path.display(),
                err
            ),
        }
    }

    fn dependency_path(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn relative_asset_path(asset_path: &Path, path: &Path) -> RelativePathBuf {
        path.relative_to(asset_path)
            .unwrap()
            .normalize()
            .with_extension("")
    }

    fn sub_asset_name(parent_name: &str, local_name: &str) -> String {
        let mut path = RelativePathBuf::from(parent_name).normalize();
        path.push(local_name);
        path.normalize().to_string()
    }

    fn local_sub_asset_name(parent_name: &str, child_name: &str) -> String {
        let parent_name = RelativePathBuf::from(parent_name).normalize().to_string();
        let child_name = RelativePathBuf::from(child_name).normalize().to_string();
        child_name
            .strip_prefix(parent_name.as_str())
            .and_then(|local| local.strip_prefix('/'))
            .unwrap_or(child_name.as_str())
            .to_string()
    }

    fn load_meta_file(
        &self,
        asset_path: &Path,
        meta_path: &Path,
    ) -> Result<AssetMetaData, BoxedError> {
        let file = File::open(meta_path).map_err(Box::new)?;
        let reader = BufReader::new(file);
        let mut meta: AssetMetaData = serde_json::from_reader(reader).map_err(Box::new)?;
        let mut data = self.asset_data_mut();
        meta.main.children = meta.inner.iter().map(|m| m.id).collect();
        data.meta.insert(meta.main.id, meta.main.clone());
        data.names.insert(
            Self::relative_asset_path(asset_path, meta_path),
            meta.main.id,
        );
        for child in meta.inner.iter_mut() {
            child.name = Self::local_sub_asset_name(&meta.main.name, &child.name);
            child.parent = Some(meta.main.id);
            let name = Self::sub_asset_name(&meta.main.name, &child.name);
            data.meta.insert(child.id, child.clone());
            data.names
                .insert(RelativePathBuf::from(name.as_str()).normalize(), child.id);
        }
        Ok(meta)
    }

    fn write_meta_file(&self, meta_path: &Path, meta: &AssetMetaData) -> Result<(), BoxedError> {
        let file = File::create(meta_path).map_err(Box::new)?;
        let writer = BufWriter::new(file);
        Ok(serde_json::to_writer_pretty(writer, meta).map_err(Box::new)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::error::AssetErrorKind;
    use crate::test_utils::test_registries_with_assets;
    use serde_json::json;

    fn temp_asset_root(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("failed to create temp asset root");
        path
    }

    fn write_parent_with_child_meta(root: &Path, child_name: &str) -> (Uuid, Uuid) {
        let parent_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();
        std::fs::write(root.join("parent.fbx"), b"").expect("failed to write parent asset");
        std::fs::write(
            root.join("parent.meta"),
            serde_json::to_vec_pretty(&json!({
                "main": {
                    "id": parent_id,
                    "name": "parent",
                    "display_name": "parent",
                    "type_uuid": Prefab::type_uuid(),
                },
                "inner": [
                    {
                        "id": child_id,
                        "name": child_name,
                        "display_name": child_name,
                        "type_uuid": Mesh::type_uuid(),
                    }
                ]
            }))
            .expect("failed to encode test meta"),
        )
        .expect("failed to write parent meta");
        (parent_id, child_id)
    }

    fn write_scene_with_meta(root: &Path) -> (PathBuf, PathBuf, Uuid) {
        let asset_id = Uuid::new_v4();
        let asset_path = root.join("scene.cxscene");
        let meta_path = root.join("scene.meta");
        std::fs::write(&asset_path, b"{}").expect("failed to write scene asset");
        std::fs::write(
            &meta_path,
            serde_json::to_vec_pretty(&json!({
                "main": {
                    "id": asset_id,
                    "name": "scene",
                    "display_name": "scene",
                    "type_uuid": Scene::type_uuid(),
                },
                "inner": []
            }))
            .expect("failed to encode scene meta"),
        )
        .expect("failed to write scene meta");
        (asset_path, meta_path, asset_id)
    }

    #[test]
    fn remove_event_for_replaced_asset_preserves_metadata() {
        let root = temp_asset_root("calyx-asset-replacement-event");
        let (asset_path, meta_path, asset_id) = write_scene_with_meta(&root);
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        let event =
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path);

        registry.recv_notify_event(event);

        assert!(meta_path.exists());
        assert!(registry.asset_data().dirty.contains_key(&asset_id));
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn remove_event_for_deleted_asset_removes_metadata() {
        let root = temp_asset_root("calyx-asset-deletion-event");
        let (asset_path, meta_path, _) = write_scene_with_meta(&root);
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        std::fs::remove_file(&asset_path).expect("failed to delete scene asset");
        let event =
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path);

        registry.recv_notify_event(event);

        assert!(!meta_path.exists());
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn create_or_update_rejects_disk_asset_name() {
        let root = temp_asset_root("calyx-disk-create-or-update");
        std::fs::write(root.join("existing.obj"), b"").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();

        let result =
            registry.create_or_update("existing".into(), Mesh::new(&registry.render_context));

        assert!(matches!(
            result,
            Err(AssetError {
                kind: AssetErrorKind::AlreadyExists,
                ..
            })
        ));
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn sub_asset_meta_uses_local_names_and_canonical_lookup() {
        let root = temp_asset_root("calyx-local-subasset-meta");
        let (parent_id, child_id) = write_parent_with_child_meta(&root, "Body");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();

        assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
        assert_eq!(
            registry.asset_meta_from_id(child_id).map(|meta| meta.name),
            Some("Body".into())
        );
        assert_eq!(
            registry
                .asset_meta_from_id(child_id)
                .and_then(|meta| meta.parent),
            Some(parent_id)
        );
        assert_eq!(
            registry
                .asset_meta_from_id(parent_id)
                .map(|meta| meta.children),
            Some(vec![child_id])
        );
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn sub_asset_meta_normalizes_legacy_full_child_names() {
        let root = temp_asset_root("calyx-legacy-subasset-meta");
        let (_, child_id) = write_parent_with_child_meta(&root, "parent/Body");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();

        assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
        assert_eq!(
            registry.asset_meta_from_id(child_id).map(|meta| meta.name),
            Some("Body".into())
        );
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }
}

impl AssetRegistry {
    /// Searches metadata entries by display name and optional asset type.
    pub fn search_assets(
        &self,
        search_term: &str,
        asset_type: Option<Uuid>,
        list: &mut Vec<AssetMeta>,
    ) {
        list.clear();
        let search_term = search_term.to_lowercase();
        // TODO: Case insensitive and word by word filter
        for (_, meta) in self.asset_data().meta.iter() {
            if (asset_type.is_none() || Some(meta.type_uuid) == asset_type)
                && meta
                    .display_name
                    .to_lowercase()
                    .contains(search_term.as_str())
            {
                list.push((*meta).clone());
            }
        }
    }

    /// Applies any pending hot-reload operations whose debounce window has
    /// elapsed.
    pub fn reload_assets(&self) {
        let now = Instant::now();
        let reload_ids = {
            let mut data = self.asset_data_mut();
            let reload_ids = data
                .dirty
                .iter()
                .filter_map(|(id, dirty_at)| {
                    (now.duration_since(*dirty_at) >= HOT_RELOAD_DEBOUNCE).then_some(*id)
                })
                .collect::<Vec<_>>();
            for id in &reload_ids {
                data.dirty.remove(id);
            }
            reload_ids
        };

        for id in reload_ids {
            let Some(path) = self.asset_meta_from_id(id).and_then(|meta| meta.path) else {
                continue;
            };
            let Some(asset_ref) = self.asset_cache().get(&id).cloned() else {
                continue;
            };
            let Some(meta) = self.asset_meta_from_id(id) else {
                continue;
            };

            let ctors = self.asset_constructors();
            if let Some(ctor) = ctors.get(&meta.type_uuid) {
                log::trace!(
                    "Hot-reloading asset {} ({}, type {}) from {}",
                    meta.name,
                    meta.id,
                    meta.type_uuid,
                    path.display()
                );
                match (ctor.reload)(self.asset_context(), &asset_ref, &path) {
                    Ok(loaded) => {
                        let sub_asset_count = loaded.sub_assets.len();
                        self.load_sub_asset_meta(id, loaded.sub_assets);
                        self.update_asset_dependencies(id, &path);
                        self.clear_reload_error(id);
                        log::info!(
                            "Hot-reloaded asset {} ({}) from {} with {} sub-assets",
                            meta.name,
                            meta.id,
                            path.display(),
                            sub_asset_count
                        );
                    }
                    Err(error) => {
                        log::warn!(
                            "Failed to hot-reload asset {} ({}) from {}: {}",
                            meta.name,
                            meta.id,
                            path.display(),
                            error
                        );
                        self.set_reload_error(AssetReloadError { id, path, error });
                    }
                }
            }
        }
    }

    /// Returns the current list of asset hot-reload failures.
    pub fn reload_errors(&self) -> Vec<AssetReloadError> {
        self.asset_data().reload_errors.clone()
    }

    fn clear_reload_error(&self, id: Uuid) {
        self.asset_data_mut()
            .reload_errors
            .retain(|error| error.id != id);
    }

    fn set_reload_error(&self, reload_error: AssetReloadError) {
        let mut data = self.asset_data_mut();
        data.reload_errors
            .retain(|error| error.id != reload_error.id);
        data.reload_errors.push(reload_error);
    }
}

impl AssetRegistry {
    const SCREEN_SPACE_QUAD: &'static str = "screen_space_quad";
    const BLACK_TEXTURE_2D: &'static str = "black_texture_2d";
    const BLACK_TEXTURE_CUBE: &'static str = "black_texture_cube";
    const DEFAULT_SCENE: &'static str = "default_scene";

    /// Returns the built-in "missing texture" asset when available.
    pub fn missing_texture(&self) -> Option<Ref<Texture>> {
        self.load::<Texture>("textures/missing").ok()
    }

    /// Returns the built-in white texture asset when available.
    pub fn white_texture(&self) -> Option<Ref<Texture>> {
        self.load::<Texture>("textures/white").ok()
    }

    /// Returns or creates an in-memory 1x1 texture for a material color.
    pub fn color_texture_2d(&self, color: [f32; 4]) -> Ref<Texture> {
        let rgba = MaterialTexture::color_key(color);
        if let Some(texture) = self.color_texture_cache.read().unwrap().get(&rgba) {
            return texture.clone();
        }

        let name = format!(
            "material_color_texture_{:02x}{:02x}{:02x}{:02x}",
            rgba[0], rgba[1], rgba[2], rgba[3]
        );
        let texture = Ref::from_id_value(
            utils::uuid_from_str(name.as_str()),
            Texture::solid_color_2d(self.render_context.clone(), name.as_str(), rgba),
        );
        let mut cache = self.color_texture_cache.write().unwrap();
        if let Some(texture) = cache.get(&rgba) {
            return texture.clone();
        }
        cache.insert(rgba, texture.clone());
        texture
    }

    /// Returns or creates a 2D black fallback texture.
    pub fn black_texture_2d(&self) -> Option<Ref<Texture>> {
        self.load_or_create(Self::BLACK_TEXTURE_2D, || {
            Texture::new(
                self.render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some(Self::BLACK_TEXTURE_2D),
                    size: wgpu::Extent3d {
                        width: 16,
                        height: 16,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                None,
                false,
            )
        })
    }

    /// Returns or creates a cubemap black fallback texture.
    pub fn black_texture_cube(&self) -> Option<Ref<Texture>> {
        self.load_or_create(Self::BLACK_TEXTURE_CUBE, || {
            Texture::new(
                self.render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some(Self::BLACK_TEXTURE_CUBE),
                    size: wgpu::Extent3d {
                        width: 16,
                        height: 16,
                        depth_or_array_layers: 6,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                Some(wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::Cube),
                    ..Default::default()
                }),
                false,
            )
        })
    }

    /// Returns the built-in cube mesh.
    pub fn cube(&self) -> Option<Ref<Mesh>> {
        self.load::<Mesh>("meshes/cube").ok()
    }

    /// Returns the built-in sphere mesh.
    pub fn sphere(&self) -> Option<Ref<Mesh>> {
        self.load::<Mesh>("meshes/sphere").ok()
    }

    /// Returns the built-in cylinder mesh.
    pub fn cylinder(&self) -> Option<Ref<Mesh>> {
        self.load::<Mesh>("meshes/cylinder").ok()
    }

    /// Returns or creates a fullscreen quad mesh.
    pub fn screen_space_quad(&self) -> Option<Ref<Mesh>> {
        self.load_or_create(Self::SCREEN_SPACE_QUAD, || {
            let mut quad = Mesh {
                indices: vec![0, 1, 2, 1, 0, 3],
                vertices: vec![
                    vec3(-1.0, -1.0, 0.0),
                    vec3(1.0, 1.0, 0.0),
                    vec3(-1.0, 1.0, 0.0),
                    vec3(1.0, -1.0, 0.0),
                ],
                normals: vec![
                    vec3(0.0, 0.0, -1.0),
                    vec3(0.0, 0.0, -1.0),
                    vec3(0.0, 0.0, -1.0),
                    vec3(0.0, 0.0, -1.0),
                ],
                uvs: [
                    vec![
                        vec2(0.0, 0.0),
                        vec2(1.0, 1.0),
                        vec2(0.0, 1.0),
                        vec2(1.0, 0.0),
                    ],
                    vec![],
                    vec![],
                    vec![],
                ],
                ..Mesh::new(&self.render_context)
            };
            quad.mark_dirty();
            quad
        })
    }

    /// Builds a wireframe circle mesh used by gizmo rendering.
    pub fn wire_circle(&self) -> Mesh {
        const RESOLUTION: usize = 72;
        let mut circle = Mesh::new(&self.render_context);
        circle.vertices.resize(RESOLUTION + 1, vec3(0.0, 0.0, 0.0));
        circle.normals.resize(RESOLUTION + 1, vec3(0.0, 0.0, 0.0));
        for i in 0..RESOLUTION {
            let angle = (i as f32) * 360.0 / (RESOLUTION as f32);
            let vertex = vec3(angle.to_radians().cos(), angle.to_radians().sin(), 0.0);
            circle.vertices[i] = vertex;
            circle.normals[i] = vertex;
        }
        circle.vertices[RESOLUTION] = circle.vertices[0];
        circle.normals[RESOLUTION] = circle.normals[0];
        circle.mark_dirty();
        circle
    }

    /// Builds a wireframe cube mesh used by gizmo rendering.
    pub fn wire_cube(&self) -> Mesh {
        let mut cube = Mesh {
            indices: vec![
                0, 1, 1, 2, 2, 3, 3, 0, 4, 5, 5, 6, 6, 7, 7, 4, 0, 4, 1, 5, 2, 6, 3, 7,
            ],
            vertices: vec![
                vec3(-0.5, -0.5, -0.5),
                vec3(-0.5, 0.5, -0.5),
                vec3(0.5, 0.5, -0.5),
                vec3(0.5, -0.5, -0.5),
                vec3(-0.5, -0.5, 0.5),
                vec3(-0.5, 0.5, 0.5),
                vec3(0.5, 0.5, 0.5),
                vec3(0.5, -0.5, 0.5),
            ],
            normals: vec![
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
            ],
            ..Mesh::new(&self.render_context)
        };
        cube.mark_dirty();
        cube
    }

    /// Creates a new empty scene bound to this registry's type contexts.
    pub fn new_empty_scene(&self) -> Scene {
        self.registry_context().scene()
    }

    /// Returns or creates the default scene asset.
    pub fn default_scene(&self) -> Option<Ref<Scene>> {
        self.load_or_create(Self::DEFAULT_SCENE, || {
            let mut scene = self.new_empty_scene();
            let game_object = scene.create(None, None);
            scene.bind_component(
                game_object,
                ComponentMesh {
                    mesh: self.load("meshes/cube").ok().into(),
                    material: self.load("materials/default").ok().into(),
                },
            );
            scene
        })
    }
}
