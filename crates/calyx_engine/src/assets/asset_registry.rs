use eframe::wgpu;
use glob::glob;
use nalgebra_glm::vec3;
use notify::event::ModifyKind;
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
use std::sync::{Arc, Condvar, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::{JoinHandle, ThreadId};
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
use crate::render::{AssetShaderLoader, RenderContext, Shader};
use crate::scene::{Prefab, Scene};
use crate::utils;
use crate::utils::TypeUuid;

use super::skybox::Skybox;
use super::LoadedAsset;

/// Loads an asset file. Given the cached reference for the id, it writes the
/// new value into that reference; otherwise it returns a new reference.
type AssetLoader = Arc<
    dyn Fn(
            ReadOnlyAssetContext,
            Uuid,
            &Path,
            Option<&Ref<dyn Asset>>,
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
    /// Registration generation and source path of each on-disk asset id. A
    /// file deleted and recreated at the same path, or an id moving to
    /// another source, keeps its id but gets a new generation.
    generations: HashMap<Uuid, (u64, PathBuf)>,
    next_generation: u64,
}

/// Loader registered for one asset type. Cloned out of the registry before it
/// runs, so no registry lock is held while it loads other assets.
#[derive(Clone)]
struct AssetConstructor {
    load: AssetLoader,
    extensions: &'static [&'static str],
    asset_name: &'static str,
}

/// How [`AssetRegistry::load_asset`] treats an asset that is already cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadMode {
    /// Return the cached reference when there is one.
    Load,
    /// Read the file again and write it into the cached reference.
    Reload,
}

/// Per-id loading slots: at most one thread loads a given asset id at a time.
#[derive(Default)]
struct LoadSlots {
    state: Mutex<LoadSlotState>,
    released: Condvar,
}

/// Which thread loads each id, and which id each blocked thread waits for.
#[derive(Default)]
struct LoadSlotState {
    owners: HashMap<Uuid, ThreadId>,
    waiting: HashMap<ThreadId, Uuid>,
}

impl LoadSlotState {
    /// Whether `owner`, directly or through the threads it waits on, waits
    /// for `thread`; `thread` waiting on `owner` would then never wake.
    fn waits_for(&self, mut owner: ThreadId, thread: ThreadId) -> bool {
        for _ in 0..=self.owners.len() {
            if owner == thread {
                return true;
            }
            match self.waiting.get(&owner).and_then(|id| self.owners.get(id)) {
                Some(next) => owner = *next,
                None => return false,
            }
        }
        false
    }
}

impl LoadSlots {
    /// Waits until no other thread is loading `id`, then claims it. Fails
    /// instead of deadlocking when waiting would close a cycle: this thread
    /// already holds the slot, or the owner waits, through any chain of
    /// threads, on a slot this thread holds.
    fn acquire(&self, id: Uuid) -> Result<LoadSlotGuard<'_>, AssetError> {
        let current = std::thread::current().id();
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        loop {
            let Some(owner) = state.owners.get(&id).copied() else {
                state.waiting.remove(&current);
                state.owners.insert(id, current);
                return Ok(LoadSlotGuard { slots: self, id });
            };
            if state.waits_for(owner, current) {
                state.waiting.remove(&current);
                return Err(AssetError::LoadError.with_source(format!(
                    "asset {id} requested while a load it depends on is in progress"
                )));
            }
            state.waiting.insert(current, id);
            state = self
                .released
                .wait(state)
                .unwrap_or_else(|err| err.into_inner());
        }
    }
}

struct LoadSlotGuard<'a> {
    slots: &'a LoadSlots,
    id: Uuid,
}

impl Drop for LoadSlotGuard<'_> {
    fn drop(&mut self) {
        self.slots
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .owners
            .remove(&self.id);
        self.slots.released.notify_all();
    }
}

/// Result of polling [`AssetRegistry::load_by_id_in_background`].
pub enum BackgroundLoad<A: ?Sized> {
    /// The asset is loaded and cached.
    Loaded(Ref<A>),
    /// A background thread is still loading the asset.
    Loading,
    /// The last background load failed. It is retried after a change to the
    /// asset's source file.
    Failed(AssetError),
}

impl<A: ?Sized> std::fmt::Debug for BackgroundLoad<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loaded(asset) => f.debug_tuple("Loaded").field(&asset.id()).finish(),
            Self::Loading => f.write_str("Loading"),
            Self::Failed(err) => f.debug_tuple("Failed").field(err).finish(),
        }
    }
}

/// Background load of one asset id that has not produced a cached asset.
#[derive(Debug, Clone)]
enum BackgroundLoadState {
    /// A background thread is loading the asset. `source_changed` records
    /// that its source changed after the load started, so a failure of the
    /// stale load is not kept and a success of it is reloaded.
    Loading { source_changed: bool },
    /// The background load failed with this error.
    Failed(AssetError),
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
    asset_constructors: RwLock<HashMap<Uuid, AssetConstructor>>,
    loading: LoadSlots,
    background_loads: Mutex<HashMap<Uuid, BackgroundLoadState>>,
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
                loading: Default::default(),
                background_loads: Default::default(),
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
            loading: Default::default(),
            background_loads: Default::default(),
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

        Ref::new_cyclic(|weak| {
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
                loading: Default::default(),
                background_loads: Default::default(),
                watcher_thread: None,
                watcher,
            };
            r.register_default_asset_types();
            r.build_meta().expect("failed to build asset metadata");
            r
        })
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

    /// Returns every cached asset of type `A`, without loading any from disk.
    pub fn loaded_assets<A: Asset + TypeUuid>(&self) -> Vec<Ref<A>> {
        self.asset_cache()
            .values()
            .filter_map(|asset| asset.try_downcast::<A>())
            .collect()
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
        self.load_typed(id, LoadMode::Load)
    }

    /// Reloads an asset by UUID as a typed handle. A cached asset is updated
    /// in place, so every holder of its reference sees the new value.
    pub fn reload_by_id<A: Asset + TypeUuid>(&self, id: Uuid) -> Result<Ref<A>, AssetError> {
        self.load_typed(id, LoadMode::Reload)
    }

    /// Starts loading an asset by UUID on a background thread, or reports the
    /// progress of the load already started, without waiting for it. Polling
    /// again while the load runs starts no second load. A failed load is
    /// reported until the asset's source file changes, then retried.
    pub fn load_by_id_in_background<A: Asset + TypeUuid>(&self, id: Uuid) -> BackgroundLoad<A> {
        let mut loads = self
            .background_loads
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        // Checked under the lock: a finished load caches its asset before it
        // clears its entry, so the asset is cached or the entry is present.
        if let Some(asset) = self.cached_asset(id) {
            return match asset.try_downcast::<A>() {
                Some(asset) => BackgroundLoad::Loaded(asset),
                None => BackgroundLoad::Failed(
                    AssetError::TypeMismatch
                        .with_type(A::asset_name())
                        .with_source(format!("asset id {id}")),
                ),
            };
        }
        if !self.asset_data().meta.contains_key(&id) {
            return BackgroundLoad::Failed(
                AssetError::NotFound
                    .with_type(A::asset_name())
                    .with_source(format!("asset id {id}")),
            );
        }
        match loads.get(&id) {
            Some(BackgroundLoadState::Loading { .. }) => return BackgroundLoad::Loading,
            Some(BackgroundLoadState::Failed(err)) => return BackgroundLoad::Failed(err.clone()),
            None => {}
        }
        let registry = self
            .asset_registry
            .upgrade()
            .expect("asset registry is always owned by its Ref");
        loads.insert(
            id,
            BackgroundLoadState::Loading {
                source_changed: false,
            },
        );
        drop(loads);
        let spawned = std::thread::Builder::new()
            .name(format!("asset-load-{id}"))
            .spawn(move || {
                let registry = registry.read();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    registry.load_by_id::<A>(id).map(|_| ())
                }))
                .unwrap_or_else(|_| {
                    Err(AssetError::LoadError
                        .with_type(A::asset_name())
                        .with_source(format!("background load of asset {id} panicked")))
                });
                registry.finish_background_load(id, result);
            });
        if let Err(err) = spawned {
            let err = AssetError::LoadError
                .with_type(A::asset_name())
                .with_source(format!("failed to start background load: {err}"));
            self.finish_background_load(id, Err(err.clone()));
            return BackgroundLoad::Failed(err);
        }
        BackgroundLoad::Loading
    }

    /// Records the outcome of a background load started by
    /// [`Self::load_by_id_in_background`].
    fn finish_background_load(&self, id: Uuid, result: Result<(), AssetError>) {
        if let Err(err) = &result {
            log::warn!(
                "Background load of asset {} ({id}) failed: {err}",
                self.asset_name(id)
            );
        }
        let mut loads = self
            .background_loads
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let source_changed = matches!(
            loads.get(&id),
            Some(BackgroundLoadState::Loading {
                source_changed: true
            })
        );
        let succeeded = result.is_ok();
        match result {
            Err(err) if !source_changed => loads.insert(id, BackgroundLoadState::Failed(err)),
            _ => loads.remove(&id),
        };
        drop(loads);
        // The hot reload skipped this asset while it was uncached, so a load
        // that read the source before it changed would stay stale: mark it
        // dirty again to reload the new source.
        if succeeded && source_changed {
            self.mark_asset_dirty(id);
        }
    }

    /// Forgets the failed background load of the asset at `path`, so the next
    /// poll of it retries. A load still in flight is marked so its failure is
    /// not kept either.
    fn clear_failed_background_load(&self, path: &Path) {
        let Some(id) = self.asset_id_from_path(path) else {
            return;
        };
        let mut loads = self
            .background_loads
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        match loads.get_mut(&id) {
            Some(BackgroundLoadState::Failed(_)) => {
                loads.remove(&id);
            }
            Some(BackgroundLoadState::Loading { source_changed }) => *source_changed = true,
            None => {}
        }
    }

    /// Loads an asset by UUID as a type-erased handle.
    pub fn load_dyn_by_id(&self, id: Uuid) -> Result<Ref<dyn Asset>, AssetError> {
        self.load_asset(id, LoadMode::Load, None)
    }

    fn load_typed<A: Asset + TypeUuid>(
        &self,
        id: Uuid,
        mode: LoadMode,
    ) -> Result<Ref<A>, AssetError> {
        self.load_asset(id, mode, Some(A::type_uuid()))?
            .try_downcast::<A>()
            .ok_or_else(|| {
                AssetError::TypeMismatch
                    .with_type(A::asset_name())
                    .with_source(format!("asset id {id}"))
            })
    }

    /// The one load routine behind every load and reload. Loads the parent
    /// asset first, claims the id's loading slot so concurrent loads of one id
    /// share a single reference, and runs the type's loader with no registry
    /// lock held. `asset_type` selects the loader; `None` uses the type in the
    /// asset's metadata.
    fn load_asset(
        &self,
        id: Uuid,
        mode: LoadMode,
        asset_type: Option<Uuid>,
    ) -> Result<Ref<dyn Asset>, AssetError> {
        let (meta, generation) = {
            let data = self.asset_data();
            (
                data.meta.get(&id).cloned(),
                data.generations.get(&id).map(|(generation, _)| *generation),
            )
        };
        let meta =
            meta.ok_or_else(|| AssetError::NotFound.with_source(format!("asset id {id}")))?;
        if let Some(parent_id) = meta.parent {
            self.load_asset(parent_id, LoadMode::Load, None)?;
        }
        if mode == LoadMode::Load {
            if let Some(asset) = self.cached_asset(id) {
                return Ok(asset);
            }
        }

        let _slot = self.loading.acquire(id)?;
        let existing = self.cached_asset(id);
        if mode == LoadMode::Load {
            if let Some(asset) = existing {
                return Ok(asset);
            }
        }

        let type_uuid = asset_type.unwrap_or(meta.type_uuid);
        let ctor = self
            .asset_constructors()
            .get(&type_uuid)
            .cloned()
            .ok_or_else(|| {
                AssetError::NotFound.with_source(format!("constructor for type {type_uuid}"))
            })?;
        let path = meta
            .path
            .as_ref()
            .filter(|path| {
                let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
                ctor.extensions.contains(&ext)
            })
            .ok_or_else(|| {
                AssetError::NotFound
                    .with_type(ctor.asset_name)
                    .with_source(format!("path for asset `{}`", meta.name))
            })?;

        log::trace!(
            "{} asset {} ({}) from {}",
            if mode == LoadMode::Load {
                "Loading"
            } else {
                "Reloading"
            },
            id,
            ctor.asset_name,
            path.display()
        );
        let LoadedAssetRef { asset, sub_assets } =
            (ctor.load)(self.asset_context(), id, path, existing.as_ref())?;
        let sub_asset_count = sub_assets.len();
        {
            // Check, record sub-assets and cache under the metadata lock, so
            // a deletion that forgot this source while it loaded cannot be
            // undone here; the forgetting side drops metadata before it
            // evicts the cache. The generation catches a delete and recreate
            // at the same path.
            let mut data = self.asset_data_mut();
            let registered = data
                .meta
                .get(&id)
                .is_some_and(|current| current.path == meta.path)
                && data.generations.get(&id).map(|(generation, _)| *generation) == generation;
            let mut cache = self.asset_cache_mut();
            if !registered {
                Self::discard_stale_load_locked(&mut data, &mut cache, id, &sub_assets);
                log::info!("Discarding asset {id}: its source was removed while it loaded");
                return Err(AssetError::NotFound
                    .with_path(path)
                    .with_source(format!("asset {id} source was removed while it loaded")));
            }
            self.load_sub_asset_meta_locked(&mut data, id, sub_assets);
            if existing.is_none() {
                cache.insert(id, asset.clone());
            }
        }
        match mode {
            LoadMode::Load => log::trace!("Loaded asset {} from {}", id, path.display()),
            LoadMode::Reload => {
                self.update_asset_dependencies(id, path);
                self.clear_reload_error(id);
                log::info!(
                    "Reloaded asset {} ({}) from {} with {} sub-assets",
                    meta.name,
                    id,
                    path.display(),
                    sub_asset_count
                );
            }
        }
        Ok(asset)
    }

    fn cached_asset(&self, id: Uuid) -> Option<Ref<dyn Asset>> {
        self.asset_cache().get(&id).cloned()
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
        log::info!("Created in-memory asset {asset_name} ({id})");
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
            AssetConstructor {
                load: Arc::new(|assets, id, path, existing| {
                    let existing = existing
                        .map(|asset_ref| {
                            asset_ref.try_downcast::<A>().ok_or_else(|| {
                                AssetError::TypeMismatch
                                    .with_path(path)
                                    .with_type(A::asset_name())
                            })
                        })
                        .transpose()?;
                    let LoadedAsset { asset, sub_assets } = A::from_file(&assets, path)?;
                    let asset_ref = match existing {
                        Some(asset_ref) => {
                            *asset_ref.write() = asset;
                            asset_ref
                        }
                        None => Ref::from_id_value(id, asset),
                    };
                    Ok(LoadedAssetRef {
                        asset: asset_ref.as_asset(),
                        sub_assets,
                    })
                }),
                extensions: A::file_extensions(),
                asset_name: A::asset_name(),
            },
        );
    }

    /// Drops what a load whose source was forgotten while it ran left
    /// behind: the asset and its sub-assets in the cache, and the metadata
    /// and names of sub-assets the load created. Sub-assets the asset
    /// registered now under this id lists as its own are kept.
    fn discard_stale_load_locked(
        data: &mut AssetData,
        cache: &mut AssetCache,
        id: Uuid,
        sub_assets: &[Uuid],
    ) {
        cache.remove(&id);
        let kept: HashSet<Uuid> = data
            .meta
            .get(&id)
            .map(|meta| meta.children.iter().copied().collect())
            .unwrap_or_default();
        for sub_asset_id in sub_assets {
            cache.remove(sub_asset_id);
            if !kept.contains(sub_asset_id) {
                data.meta.remove(sub_asset_id);
                data.names.retain(|_, name_id| name_id != sub_asset_id);
            }
        }
    }

    fn load_sub_asset_meta_locked(&self, data: &mut AssetData, id: Uuid, sub_assets: Vec<Uuid>) {
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

    fn asset_constructors(&self) -> RwLockReadGuard<'_, HashMap<Uuid, AssetConstructor>> {
        self.asset_constructors.read().unwrap()
    }

    fn asset_constructors_mut(&self) -> RwLockWriteGuard<'_, HashMap<Uuid, AssetConstructor>> {
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
    pub(crate) fn recv_notify_event(&self, event: Event) {
        log::trace!("Received asset notification: {:?}", event.kind);
        let paths_iter = Self::notify_event_paths(&event);
        match event.kind {
            EventKind::Create(_) => {
                for file in paths_iter {
                    self.register_created_file(file);
                }
            }
            // A file written to a temporary file and renamed onto its path,
            // as scenes and prefabs are saved, arrives as a rename. Onto a
            // path no asset holds yet, it is a new file.
            EventKind::Modify(ModifyKind::Name(_)) => {
                for file in paths_iter {
                    if self.asset_id_from_path(file).is_some() {
                        self.mark_path_dirty(file);
                    } else {
                        self.register_created_file(file);
                    }
                }
            }
            EventKind::Modify(_) => {
                for file in paths_iter {
                    self.mark_path_dirty(file);
                }
            }
            EventKind::Remove(_) => {
                // A deleted folder arrives as one extensionless path that
                // `notify_event_paths` drops, so forget by path prefix here.
                for removed in event.paths.iter().filter(|path| !path.exists()) {
                    self.forget_assets_under(removed);
                }
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
        // Only events that can change the source retry a failed load; an
        // access event, such as the loader opening its own source, cannot.
        if matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            for file in Self::notify_event_paths(&event) {
                self.clear_failed_background_load(file);
            }
        }
    }

    /// Registers the file the watcher reported as new, warning when it lies
    /// outside every asset root or its metadata cannot be built.
    fn register_created_file(&self, file: &Path) {
        let Some(asset_path) = self.owning_asset_root(file) else {
            log::warn!(
                "Ignoring created file outside every asset root: {}",
                file.display()
            );
            return;
        };
        if let Err(err) = self.build_asset_meta(asset_path, file, &file.with_extension("meta")) {
            log::warn!(
                "Failed to build asset metadata for {}: {}",
                file.display(),
                err
            );
        }
    }

    /// Whether `path` lies inside an asset root, so the registry can hold the
    /// file at it. `path` need not exist; a relative path is taken from the
    /// working directory.
    pub fn is_in_asset_root(&self, path: &Path) -> bool {
        self.rooted_path(path).is_some()
    }

    /// Registers the asset file at `path` right away, without waiting for the
    /// watcher to report it, and returns its id. A registered file keeps its
    /// id. A writer that opens the file it just wrote registers it here
    /// first, since the watcher reports a file on its own thread, later.
    pub fn register_file(&self, path: &Path) -> Result<Uuid, AssetError> {
        let not_found = || AssetError::NotFound.with_path(path);
        let (root, path) = self
            .rooted_path(path)
            .ok_or_else(|| not_found().with_source("the file is outside every asset root"))?;
        if let Some(id) = self.asset_id_from_path(&path) {
            return Ok(id);
        }
        self.build_asset_meta(root, &path, &path.with_extension("meta"))
            .map_err(|err| AssetError::LoadError.with_path(&path).with_source(err))?;
        self.asset_id_from_path(&path).ok_or_else(not_found)
    }

    /// The deepest asset root holding `path`, and `path` spelled from that
    /// root as the registry stores it. Both sides are compared in normalized
    /// form, so a root or file named through an 8.3 short name, a symlink or
    /// another letter case still matches. `None` when no root holds `path`.
    fn rooted_path(&self, path: &Path) -> Option<(&Path, PathBuf)> {
        let path = Self::normalized_path(path)?;
        self.asset_paths
            .iter()
            .filter_map(|root| {
                let normalized_root = Self::normalized_path(root)?;
                let rest = path.strip_prefix(&normalized_root).ok()?;
                Some((normalized_root.components().count(), root, root.join(rest)))
            })
            .max_by_key(|(depth, ..)| *depth)
            .map(|(_, root, path)| (root.as_path(), path))
    }

    /// `path` canonical when it exists, else made absolute against the working
    /// directory below its canonical nearest existing ancestor.
    fn normalized_path(path: &Path) -> Option<PathBuf> {
        if let Ok(path) = dunce::canonicalize(path) {
            return Some(path);
        }
        // The file may not exist yet: canonicalize its nearest existing
        // ancestor, so its root matches the canonical asset roots.
        let absolute = path.absolutize().ok()?.into_owned();
        let mut existing = absolute.as_path();
        let mut rest = Vec::new();
        while !existing.exists() {
            rest.push(existing.file_name()?.to_os_string());
            existing = existing.parent()?;
        }
        let mut normalized = dunce::canonicalize(existing).ok()?;
        normalized.extend(rest.iter().rev());
        Some(normalized)
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

    /// Returns the asset root that contains `path`, preferring the deepest
    /// root when roots nest.
    fn owning_asset_root(&self, path: &Path) -> Option<&Path> {
        self.asset_paths
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .map(PathBuf::as_path)
    }

    /// Drops the in-memory metadata, names, dirty marks, reload errors and
    /// dependency entries of every asset whose source file is `path` or lies below it,
    /// and of their sub-assets. Assets are matched by their stored source
    /// path, not by name, so a same-name asset in another root survives and
    /// is registered again under any name the forgotten asset held.
    fn forget_assets_under(&self, path: &Path) {
        let mut data = self.asset_data_mut();
        let mut ids: HashSet<Uuid> = HashSet::new();
        for meta in data.meta.values() {
            if meta.path.as_deref().is_some_and(|p| p.starts_with(path)) {
                ids.insert(meta.id);
                ids.extend(meta.children.iter().copied());
            }
        }
        if ids.is_empty() {
            return;
        }
        let freed: HashSet<RelativePathBuf> = data
            .names
            .iter()
            .filter(|(_, id)| ids.contains(id))
            .map(|(name, _)| name.clone())
            .collect();
        data.meta.retain(|id, _| !ids.contains(id));
        data.names.retain(|_, id| !ids.contains(id));
        data.dirty.retain(|id, _| !ids.contains(id));
        data.reload_errors.retain(|error| !ids.contains(&error.id));
        data.generations.retain(|id, _| !ids.contains(id));
        for dependents in data.dependencies.values_mut() {
            dependents.retain(|dependent| !ids.contains(dependent));
        }
        data.dependencies
            .retain(|_, dependents| !dependents.is_empty());

        drop(data);
        // Evict cached values too: a recreated file often gets the same
        // path-derived id and must load fresh. Holders keep their own refs.
        self.asset_cache_mut().retain(|id, _| !ids.contains(id));
        self.register_sources_named(&freed);
        log::info!("Forgot deleted assets under {}", path.display());
    }

    /// Registers again, from disk, every asset file in any root whose name
    /// is in `names`. A same-name file in another root can be hidden by the
    /// forgotten asset, sometimes under the same generated id, so only a
    /// rescan of the roots finds it.
    fn register_sources_named(&self, names: &HashSet<RelativePathBuf>) {
        for name in names {
            for root in &self.asset_paths {
                let Some(dir) = name.to_path(root).parent().map(Path::to_path_buf) else {
                    continue;
                };
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for candidate in entries.filter_map(|entry| entry.ok().map(|e| e.path())) {
                    // The same files `build_meta` registers at startup.
                    let ext = candidate
                        .extension()
                        .map_or("", |ext| ext.to_str().unwrap_or(""));
                    if ext.is_empty() || ext == "meta" || ext == "rs" || !candidate.is_file() {
                        continue;
                    }
                    if self.owning_asset_root(&candidate) != Some(root.as_path())
                        || Self::relative_asset_path(root, &candidate).ok().as_ref() != Some(name)
                    {
                        continue;
                    }
                    if let Err(err) =
                        self.build_asset_meta(root, &candidate, &candidate.with_extension("meta"))
                    {
                        log::warn!(
                            "Failed to build asset metadata for {}: {}",
                            candidate.display(),
                            err
                        );
                    }
                }
            }
        }
    }

    /// Resolves an asset UUID from a filesystem path.
    pub fn asset_id_from_path(&self, path: &Path) -> Option<Uuid> {
        let root_path = self.owning_asset_root(path)?;
        let name = Self::relative_asset_path(root_path, path).ok()?;
        self.asset_data().names.get(&name).copied()
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
                        log::warn!("Skipping asset path while building metadata: {err}");
                        continue;
                    }
                };
                let ext = path
                    .extension()
                    .map_or("", |ext| ext.to_str().unwrap_or(""));
                if ext.is_empty() || ext == "meta" || ext == "rs" {
                    continue;
                }
                if self.owning_asset_root(&path) != Some(asset_path.as_path()) {
                    // A deeper root owns this file; its own pass registers it.
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
        // An asset is typed by its extension. Checked before anything is
        // written, so a file without one gets no `.meta`.
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            return Err(format!("{} has no file extension to type it by", path.display()).into());
        };
        let name_path = Self::relative_asset_path(asset_path, path)?;
        // The asset this name resolves to now, captured before the metadata
        // read below replaces it, in case this file takes over its id.
        let previous = {
            let data = self.asset_data();
            data.names
                .get(&name_path)
                .and_then(|id| data.meta.get(id))
                .filter(|meta| meta.parent.is_none())
                .cloned()
        };
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
        meta.type_uuid = self.asset_type_uuid_from_ext(ext).unwrap_or(Uuid::nil());
        meta.path = Some(match path.absolutize().map_err(Box::new)? {
            Cow::Borrowed(p) => p.to_path_buf(),
            Cow::Owned(p) => p,
        });
        let id = meta.id;
        let source = meta.path.clone().unwrap_or_default();
        let mut data = self.asset_data_mut();
        let children = meta.children.clone();
        data.meta.insert(id, meta);
        data.names.insert(name_path.clone(), id);
        // Only a new registration gets a new generation: forgetting an asset
        // drops its generation, so a delete and recreate counts as new, and
        // so does the id moving to another source, while a create event for
        // a still-registered file must not make in-flight loads of it stale.
        let moved = data
            .generations
            .get(&id)
            .is_some_and(|(_, previous)| *previous != source);
        if moved || !data.generations.contains_key(&id) {
            data.next_generation += 1;
            let generation = data.next_generation;
            data.generations.insert(id, (generation, source));
        }
        if moved {
            // The previous source's sub-assets that this source does not
            // also define belong to a file no longer registered here.
            let stale_children: HashSet<Uuid> = previous
                .filter(|previous| previous.id == id)
                .map(|previous| {
                    previous
                        .children
                        .into_iter()
                        .filter(|child| !children.contains(child))
                        .collect()
                })
                .unwrap_or_default();
            data.meta
                .retain(|meta_id, _| !stale_children.contains(meta_id));
            // Names the previous source gave this id and its sub-assets, as
            // when the file moved along with its `.meta`, no longer apply.
            data.names.retain(|name, name_id| {
                let ours = *name_id == id || children.contains(name_id);
                !stale_children.contains(name_id)
                    && (!ours || *name == name_path || name.starts_with(&name_path))
            });
            data.dirty
                .retain(|dirty_id, _| !stale_children.contains(dirty_id));
            // A reload error described the previous source.
            data.reload_errors
                .retain(|error| error.id != id && !stale_children.contains(&error.id));
            // Cached values were read from the previous source.
            let mut cache = self.asset_cache_mut();
            cache.remove(&id);
            for child in children.iter().chain(&stale_children) {
                cache.remove(child);
            }
        }
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

        match AssetShaderLoader::new(self).dependencies(path) {
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

    /// Returns `path` relative to `asset_path` without its extension, or an
    /// error when `path` is not inside `asset_path`.
    fn relative_asset_path(asset_path: &Path, path: &Path) -> Result<RelativePathBuf, BoxedError> {
        let relative = path.strip_prefix(asset_path).map_err(|_| {
            format!(
                "{} is not inside asset root {}",
                path.display(),
                asset_path.display()
            )
        })?;
        Ok(RelativePathBuf::from_path(relative)
            .map_err(Box::new)?
            .normalize()
            .with_extension(""))
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
        let name_path = Self::relative_asset_path(asset_path, meta_path)?;
        let file = File::open(meta_path).map_err(Box::new)?;
        let reader = BufReader::new(file);
        let mut meta: AssetMetaData = serde_json::from_reader(reader).map_err(Box::new)?;
        // Child names may carry the stored main name as a legacy prefix, so
        // strip it before the main name is rebased onto the file's location
        // under its owning root (an older scan may have used an outer root).
        for child in meta.inner.iter_mut() {
            child.name = Self::local_sub_asset_name(&meta.main.name, &child.name);
        }
        meta.main.name = name_path.to_string();
        let mut data = self.asset_data_mut();
        meta.main.children = meta.inner.iter().map(|m| m.id).collect();
        data.meta.insert(meta.main.id, meta.main.clone());
        data.names.insert(name_path, meta.main.id);
        for child in meta.inner.iter_mut() {
            child.parent = Some(meta.main.id);
            let name = Self::sub_asset_name(&meta.main.name, &child.name);
            data.meta.insert(child.id, child.clone());
            data.names
                .insert(RelativePathBuf::from(name.as_str()).normalize(), child.id);
        }
        Ok(meta)
    }

    /// Writes `meta` to `meta_path`, leaving the file untouched when it
    /// already holds the same content in either line-ending style. A changed
    /// file keeps the line endings it had, so a checkout's CRLF files (git's
    /// `core.autocrlf`) stay CRLF and only the changed lines differ.
    fn write_meta_file(&self, meta_path: &Path, meta: &AssetMetaData) -> Result<(), BoxedError> {
        let content = serde_json::to_string_pretty(meta).map_err(Box::new)?;
        let existing = std::fs::read(meta_path).ok();
        let crlf = match existing.as_deref() {
            Some(existing) => {
                if Self::normalize_line_endings(existing) == content.as_bytes() {
                    return Ok(());
                }
                existing.windows(2).any(|pair| pair == b"\r\n")
            }
            None => false,
        };
        let content = if crlf {
            content.replace('\n', "\r\n")
        } else {
            content
        };
        std::fs::write(meta_path, content).map_err(Box::new)?;
        Ok(())
    }

    fn normalize_line_endings(bytes: &[u8]) -> Vec<u8> {
        let mut normalized = Vec::with_capacity(bytes.len());
        let mut iter = bytes.iter().peekable();
        while let Some(&byte) = iter.next() {
            if byte == b'\r' && iter.peek() == Some(&&b'\n') {
                continue;
            }
            normalized.push(byte);
        }
        normalized
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
        for meta in self.asset_data().meta.values() {
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
            let (meta, generation) = {
                let data = self.asset_data();
                (
                    data.meta.get(&id).cloned(),
                    data.generations.get(&id).map(|(generation, _)| *generation),
                )
            };
            let Some(meta) = meta else {
                continue;
            };
            let Some(path) = meta.path else {
                continue;
            };
            if self.cached_asset(id).is_none() {
                continue;
            }
            log::trace!(
                "Hot-reloading asset {} ({}, type {}) from {}",
                meta.name,
                id,
                meta.type_uuid,
                path.display()
            );
            if let Err(error) = self.load_asset(id, LoadMode::Reload, None) {
                log::warn!(
                    "Failed to hot-reload asset {} ({}) from {}: {}",
                    meta.name,
                    id,
                    path.display(),
                    error
                );
                self.set_current_reload_error(generation, AssetReloadError { id, path, error });
            }
        }
    }

    /// Records a reload error only while the source it was read from is
    /// still registered under the same generation, so a failure of a file
    /// deleted, or deleted and recreated, during the reload is dropped.
    fn set_current_reload_error(&self, generation: Option<u64>, reload_error: AssetReloadError) {
        let mut data = self.asset_data_mut();
        let id = reload_error.id;
        let current = data
            .meta
            .get(&id)
            .is_some_and(|meta| meta.path.as_ref() == Some(&reload_error.path))
            && data.generations.get(&id).map(|(generation, _)| *generation) == generation;
        if !current {
            log::info!("Dropping reload error for asset {id}: its source was removed");
            return;
        }
        data.reload_errors.retain(|error| error.id != id);
        data.reload_errors.push(reload_error);
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
}

impl AssetRegistry {
    pub(crate) const BLACK_TEXTURE_2D: &'static str = "black_texture_2d";
    pub(crate) const BLACK_TEXTURE_CUBE: &'static str = "black_texture_cube";
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
            scene.add_component(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::error::AssetErrorKind;
    use crate::test_support::test_registries_with_assets;
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
        let original_meta = std::fs::read(&meta_path).expect("failed to read original metadata");
        let replacement_path = root.join("scene.cxscene.tmp");
        std::fs::write(&replacement_path, b"replacement")
            .expect("failed to stage replacement asset");
        if std::fs::rename(&replacement_path, &asset_path).is_err() {
            let backup_path = root.join("scene.cxscene.backup");
            std::fs::rename(&asset_path, &backup_path).expect("failed to stage original asset");
            std::fs::rename(&replacement_path, &asset_path)
                .expect("failed to install replacement asset");
            std::fs::remove_file(backup_path).expect("failed to remove original asset backup");
        }
        let event = Event::new(EventKind::Remove(notify::event::RemoveKind::File))
            .add_path(asset_path.clone());

        registry.recv_notify_event(event);

        assert_eq!(
            std::fs::read(&asset_path).expect("failed to read replacement asset"),
            b"replacement"
        );
        assert_eq!(
            std::fs::read(&meta_path).expect("failed to read preserved metadata"),
            original_meta
        );
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
    fn create_event_in_second_root_registers_relative_to_that_root() {
        let first_root = temp_asset_root("calyx-create-event-first-root");
        let second_root = temp_asset_root("calyx-create-event-second-root");
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        std::fs::create_dir_all(second_root.join("nested")).expect("failed to create nested dir");
        let asset_path = second_root.join("nested").join("created.cxscene");
        std::fs::write(&asset_path, b"{}").expect("failed to write created asset");
        let event = Event::new(EventKind::Create(notify::event::CreateKind::File))
            .add_path(asset_path.clone());

        registry.recv_notify_event(event);

        let id = registry
            .asset_id("nested/created")
            .expect("created asset is registered under the second root");
        let meta = registry
            .asset_meta_from_id(id)
            .expect("created asset has metadata");
        assert_eq!(meta.name, "nested/created");
        assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
        assert!(second_root.join("nested").join("created.meta").exists());
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn registering_a_file_without_an_extension_is_refused() {
        let root = temp_asset_root("calyx-register-extensionless");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        // A scene saved as `new_level`, with no extension to type it by.
        let path = root.join("new_level");
        std::fs::write(&path, b"{}").expect("failed to write extensionless file");

        let error = registry
            .register_file(&path)
            .expect_err("a file without an extension has no asset type");

        assert!(error.to_string().contains("no file extension"), "{error}");
        assert_eq!(registry.asset_id_from_path(&path), None);
        assert!(!root.join("new_level.meta").exists(), "no .meta is written");
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn a_root_named_in_another_form_owns_the_files_under_it() {
        let root = temp_asset_root("calyx-register-root-form");
        std::fs::create_dir_all(root.join("sub")).expect("failed to create sub directory");
        // The same directory as `root`, named through `sub/..`, as an 8.3
        // short name or a symlink would name it differently on disk.
        let alternate_root = root.join("sub").join("..");
        let registries = test_registries_with_assets(vec![alternate_root]);
        let registry = registries.assets.read();
        let path = root.join("new_level.cxscene");
        std::fs::write(&path, b"{}").expect("failed to write asset file");

        assert!(registry.is_in_asset_root(&path));
        let id = registry
            .register_file(&path)
            .expect("a file under the root registers");

        assert_eq!(registry.asset_id("new_level"), Some(id));
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn rename_onto_a_new_asset_file_registers_it_with_its_meta() {
        let root = temp_asset_root("calyx-rename-event");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        // How scenes and prefabs are saved: a temporary file renamed onto
        // the target, which the watcher reports as a rename, not a create.
        // On Windows the target arrives alone, as the rename's new name.
        let temp_path = root.join("saved.cxscene.0123.tmp");
        let asset_path = root.join("saved.cxscene");
        std::fs::write(&temp_path, b"{}").expect("failed to write temp file");
        std::fs::rename(&temp_path, &asset_path).expect("failed to rename temp file");
        let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::To,
        )))
        .add_path(asset_path.clone());

        registry.recv_notify_event(event);

        let id = registry
            .asset_id_from_path(&asset_path)
            .expect("the renamed-to file is registered");
        assert_eq!(registry.asset_id("saved"), Some(id));
        assert!(root.join("saved.meta").exists(), "its .meta is written");
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn path_lookup_uses_deepest_root_when_roots_nest() {
        let outer_root = temp_asset_root("calyx-nested-roots");
        let inner_root = outer_root.join("inner");
        std::fs::create_dir_all(&inner_root).expect("failed to create inner asset root");
        let registries = test_registries_with_assets(vec![outer_root.clone(), inner_root.clone()]);
        let registry = registries.assets.read();
        let asset_path = inner_root.join("created.cxscene");
        std::fs::write(&asset_path, b"{}").expect("failed to write created asset");
        let event = Event::new(EventKind::Create(notify::event::CreateKind::File))
            .add_path(asset_path.clone());

        registry.recv_notify_event(event);

        let id = registry
            .asset_id("created")
            .expect("created asset is registered under the inner root");
        assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
        drop(registry);
        std::fs::remove_dir_all(outer_root).expect("failed to remove temp asset root");
    }

    #[test]
    fn startup_scan_names_nested_assets_relative_to_deepest_root() {
        let outer_root = temp_asset_root("calyx-nested-startup");
        let inner_root = outer_root.join("inner");
        std::fs::create_dir_all(&inner_root).expect("failed to create inner asset root");
        let asset_path = inner_root.join("existing.cxscene");
        std::fs::write(&asset_path, b"{}").expect("failed to write existing asset");
        let registries = test_registries_with_assets(vec![outer_root.clone(), inner_root.clone()]);
        let registry = registries.assets.read();

        let id = registry
            .asset_id("existing")
            .expect("existing asset is registered under the inner root");
        assert_eq!(
            registry.asset_meta_from_id(id).map(|meta| meta.name),
            Some("existing".into())
        );
        assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
        assert_eq!(registry.asset_id("inner/existing"), None);
        drop(registry);
        std::fs::remove_dir_all(outer_root).expect("failed to remove temp asset root");
    }

    #[test]
    fn remove_event_forgets_only_the_asset_at_the_removed_path() {
        let first_root = temp_asset_root("calyx-same-name-first-root");
        let second_root = temp_asset_root("calyx-same-name-second-root");
        let (first_path, _, first_id) = write_scene_with_meta(&first_root);
        let (_, _, second_id) = write_scene_with_meta(&second_root);
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        assert_eq!(registry.asset_id("scene"), Some(second_id));

        std::fs::remove_file(&first_path).expect("failed to delete first scene");
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(first_path),
        );

        assert!(registry.asset_meta_from_id(first_id).is_none());
        assert!(registry.asset_meta_from_id(second_id).is_some());
        assert_eq!(registry.asset_id("scene"), Some(second_id));
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn remove_event_for_name_owner_restores_same_name_asset_from_other_root() {
        let first_root = temp_asset_root("calyx-same-name-restore-first-root");
        let second_root = temp_asset_root("calyx-same-name-restore-second-root");
        let (_, _, first_id) = write_scene_with_meta(&first_root);
        let (second_path, _, second_id) = write_scene_with_meta(&second_root);
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        assert_eq!(registry.asset_id("scene"), Some(second_id));

        std::fs::remove_file(&second_path).expect("failed to delete second scene");
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(second_path),
        );

        assert!(registry.asset_meta_from_id(second_id).is_none());
        assert_eq!(registry.asset_id("scene"), Some(first_id));
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn remove_event_restores_same_name_asset_sharing_a_generated_id() {
        let first_root = temp_asset_root("calyx-shared-id-first-root");
        let second_root = temp_asset_root("calyx-shared-id-second-root");
        let first_path = first_root.join("scene.cxscene");
        let second_path = second_root.join("scene.cxscene");
        std::fs::write(&first_path, b"{}").expect("failed to write first scene");
        std::fs::write(&second_path, b"{}").expect("failed to write second scene");
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        let id = registry.asset_id("scene").expect("scene is registered");
        assert_eq!(
            registry.asset_meta_from_id(id).and_then(|meta| meta.path),
            Some(second_path.clone())
        );

        std::fs::remove_file(&second_path).expect("failed to delete second scene");
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(second_path),
        );

        let restored = registry
            .asset_id("scene")
            .expect("the first root's scene takes the name back");
        assert_eq!(
            registry
                .asset_meta_from_id(restored)
                .and_then(|meta| meta.path),
            Some(first_path)
        );
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn remove_event_clears_reload_errors_of_deleted_asset() {
        let root = temp_asset_root("calyx-remove-clears-reload-error");
        let (asset_path, _, asset_id) = write_scene_with_meta(&root);
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        let generation = registry
            .asset_data()
            .generations
            .get(&asset_id)
            .map(|(generation, _)| *generation);
        registry.set_current_reload_error(
            generation,
            AssetReloadError {
                id: asset_id,
                path: asset_path.clone(),
                error: AssetError::LoadError.with_source("test reload failure"),
            },
        );
        assert_eq!(registry.reload_errors().len(), 1);

        std::fs::remove_file(&asset_path).expect("failed to delete scene asset");
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
        );

        assert!(registry.reload_errors().is_empty());
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn remove_event_restores_same_name_asset_with_unregistered_extension() {
        let first_root = temp_asset_root("calyx-unregistered-ext-first-root");
        let second_root = temp_asset_root("calyx-unregistered-ext-second-root");
        let first_path = first_root.join("scene.custom");
        let second_path = second_root.join("scene.custom");
        std::fs::write(&first_path, b"").expect("failed to write first file");
        std::fs::write(&second_path, b"").expect("failed to write second file");
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        assert!(registry.asset_id("scene").is_some());

        std::fs::remove_file(&second_path).expect("failed to delete second file");
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(second_path),
        );

        let restored = registry
            .asset_id("scene")
            .expect("the first root's file takes the name back");
        assert_eq!(
            registry
                .asset_meta_from_id(restored)
                .and_then(|meta| meta.path),
            Some(first_path)
        );
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn folder_remove_event_forgets_assets_below_the_folder() {
        let root = temp_asset_root("calyx-folder-remove");
        let folder = root.join("sub");
        std::fs::create_dir_all(&folder).expect("failed to create asset folder");
        let (_, _, asset_id) = write_scene_with_meta(&folder);
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        assert_eq!(registry.asset_id("sub/scene"), Some(asset_id));

        std::fs::remove_dir_all(&folder).expect("failed to delete asset folder");
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::Folder)).add_path(folder),
        );

        assert_eq!(registry.asset_id("sub/scene"), None);
        assert!(registry.asset_meta_from_id(asset_id).is_none());
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn legacy_outer_relative_meta_is_rebased_to_owning_root() {
        let outer_root = temp_asset_root("calyx-legacy-nested-meta");
        let inner_root = outer_root.join("inner");
        std::fs::create_dir_all(&inner_root).expect("failed to create inner asset root");
        let parent_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();
        std::fs::write(inner_root.join("parent.fbx"), b"").expect("failed to write parent asset");
        std::fs::write(
            inner_root.join("parent.meta"),
            serde_json::to_vec_pretty(&json!({
                "main": {
                    "id": parent_id,
                    "name": "inner/parent",
                    "display_name": "parent",
                    "type_uuid": Prefab::type_uuid(),
                },
                "inner": [
                    {
                        "id": child_id,
                        "name": "inner/parent/Body",
                        "display_name": "Body",
                        "type_uuid": Mesh::type_uuid(),
                    }
                ]
            }))
            .expect("failed to encode test meta"),
        )
        .expect("failed to write parent meta");
        let registries = test_registries_with_assets(vec![outer_root.clone(), inner_root.clone()]);
        let registry = registries.assets.read();

        assert_eq!(registry.asset_id("parent"), Some(parent_id));
        assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
        assert_eq!(registry.asset_name(parent_id), "parent");
        assert_eq!(registry.asset_name(child_id), "Body");
        drop(registry);
        std::fs::remove_dir_all(outer_root).expect("failed to remove temp asset root");
    }

    #[test]
    fn remove_event_for_deleted_asset_forgets_metadata_and_names() {
        let first_root = temp_asset_root("calyx-remove-event-first-root");
        let second_root = temp_asset_root("calyx-remove-event-second-root");
        let (asset_path, meta_path, asset_id) = write_scene_with_meta(&second_root);
        let (parent_id, child_id) = write_parent_with_child_meta(&second_root, "Body");
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        assert_eq!(registry.asset_id("scene"), Some(asset_id));
        assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
        std::fs::remove_file(&asset_path).expect("failed to delete scene asset");
        let parent_path = second_root.join("parent.fbx");
        std::fs::remove_file(&parent_path).expect("failed to delete parent asset");
        let event = Event::new(EventKind::Remove(notify::event::RemoveKind::File))
            .add_path(asset_path)
            .add_path(parent_path);

        registry.recv_notify_event(event);

        assert!(!meta_path.exists());
        assert!(!second_root.join("parent.meta").exists());
        assert_eq!(registry.asset_id("scene"), None);
        assert!(registry.asset_meta_from_id(asset_id).is_none());
        assert_eq!(registry.asset_id("parent"), None);
        assert_eq!(registry.asset_id("parent/Body"), None);
        assert!(registry.asset_meta_from_id(parent_id).is_none());
        assert!(registry.asset_meta_from_id(child_id).is_none());
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn relative_asset_path_rejects_path_outside_root() {
        let root = temp_asset_root("calyx-relative-path-root");
        let other = temp_asset_root("calyx-relative-path-other");

        assert!(AssetRegistry::relative_asset_path(&root, &other.join("asset.obj")).is_err());
        assert_eq!(
            AssetRegistry::relative_asset_path(&root, &root.join("dir").join("asset.obj"))
                .expect("path inside root resolves")
                .as_str(),
            "dir/asset"
        );
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
        std::fs::remove_dir_all(other).expect("failed to remove other temp dir");
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

    /// Plain-text asset used to observe what the registry loaded.
    struct TextTestAsset {
        text: String,
    }

    impl TypeUuid for TextTestAsset {
        fn type_uuid() -> Uuid {
            Uuid::from_u128(0x5d1f_0c2a_7b44_4f0e_9a61_3c8e_2b17_d401)
        }
    }

    impl Asset for TextTestAsset {
        fn file_extensions() -> &'static [&'static str] {
            &["cxtesttext"]
        }

        fn from_file(
            _assets: &ReadOnlyAssetContext,
            path: &Path,
        ) -> Result<LoadedAsset<Self>, AssetError> {
            let text = std::fs::read_to_string(path)
                .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
            Ok(LoadedAsset::new(Self { text }))
        }
    }

    static SLOW_TEST_ASSET_LOADS: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    /// Asset whose load is slow enough for two threads to race on it.
    struct SlowTestAsset;

    impl TypeUuid for SlowTestAsset {
        fn type_uuid() -> Uuid {
            Uuid::from_u128(0x9e3b_61d4_02a7_4c55_b8f2_71e0_4d6a_c902)
        }
    }

    impl Asset for SlowTestAsset {
        fn file_extensions() -> &'static [&'static str] {
            &["cxtestslow"]
        }

        fn from_file(
            _assets: &ReadOnlyAssetContext,
            _path: &Path,
        ) -> Result<LoadedAsset<Self>, AssetError> {
            SLOW_TEST_ASSET_LOADS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            Ok(LoadedAsset::new(Self))
        }
    }

    type TestGate = Arc<(std::sync::Barrier, std::sync::Barrier)>;

    /// Start and release barriers for [`GatedTestAsset`] loads, per source
    /// path so tests running in parallel do not share a gate.
    static GATED_TEST_ASSET_GATES: std::sync::OnceLock<Mutex<HashMap<PathBuf, TestGate>>> =
        std::sync::OnceLock::new();

    /// How many times a [`GatedTestAsset`] load started, per source path.
    static GATED_TEST_ASSET_LOADS: std::sync::OnceLock<Mutex<HashMap<PathBuf, usize>>> =
        std::sync::OnceLock::new();

    fn gated_test_asset_loads(path: &Path) -> usize {
        GATED_TEST_ASSET_LOADS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .get(path)
            .copied()
            .unwrap_or_default()
    }

    fn gated_test_asset_gate(path: &Path) -> TestGate {
        GATED_TEST_ASSET_GATES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new((std::sync::Barrier::new(2), std::sync::Barrier::new(2))))
            .clone()
    }

    /// Asset whose load, when its file contains `gate`, signals that it
    /// started and then waits to be released, so a test can act while the
    /// load is in flight. A file containing `fail` makes the load fail once
    /// released. Like an imported prefab, it reads its metadata first and
    /// creates a `Child` sub-asset once released.
    struct GatedTestAsset;

    impl TypeUuid for GatedTestAsset {
        fn type_uuid() -> Uuid {
            Uuid::from_u128(0x2c84_f1e9_5a30_4d7b_a6c1_08f3_9e52_b713)
        }
    }

    impl Asset for GatedTestAsset {
        fn file_extensions() -> &'static [&'static str] {
            &["cxtestgated"]
        }

        fn from_file(
            assets: &ReadOnlyAssetContext,
            path: &Path,
        ) -> Result<LoadedAsset<Self>, AssetError> {
            let registry = assets.registries.assets.read();
            let meta = registry
                .asset_meta_from_path(path)
                .ok_or_else(|| AssetError::NotFound.with_path(path))?;
            let content = std::fs::read_to_string(path)
                .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
            *GATED_TEST_ASSET_LOADS
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .entry(path.to_path_buf())
                .or_default() += 1;
            if content.contains("gate") {
                let gate = gated_test_asset_gate(path);
                gate.0.wait();
                gate.1.wait();
            }
            if content.contains("fail") {
                return Err(AssetError::LoadError
                    .with_path(path)
                    .with_source("test load failure"));
            }
            let child = registry.create_or_update_sub_asset(
                meta.id,
                &meta.name,
                "Child",
                TextTestAsset {
                    text: "child".into(),
                },
            )?;
            Ok(LoadedAsset {
                asset: Self,
                sub_assets: vec![child.id()],
            })
        }
    }

    #[test]
    fn load_in_flight_during_deletion_does_not_cache_the_deleted_asset() {
        let root = temp_asset_root("calyx-delete-during-load");
        let asset_path = root.join("gated.cxtestgated");
        std::fs::write(&asset_path, "gate").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let id = {
            let registry = registries.assets.read();
            registry.register_asset_type::<GatedTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
            registry
                .asset_id("gated")
                .expect("test asset should have metadata")
        };
        let gate = gated_test_asset_gate(&asset_path);
        let assets = registries.assets.clone();
        let loader = std::thread::spawn(move || {
            assets
                .read()
                .load_by_id::<GatedTestAsset>(id)
                .map(|_| ())
                .map_err(|err| err.to_string())
        });

        gate.0.wait();
        std::fs::remove_file(&asset_path).expect("failed to delete test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
        );
        gate.1.wait();
        let result = loader.join().expect("loader thread panicked");

        assert!(result.is_err(), "a load of a deleted source must fail");

        assert!(registries
            .assets
            .read()
            .loaded_by_id::<GatedTestAsset>(id)
            .is_none());
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn discarded_load_rolls_back_the_sub_assets_it_created() {
        let root = temp_asset_root("calyx-discarded-load-sub-assets");
        let asset_path = root.join("gated.cxtestgated");
        std::fs::write(&asset_path, "gate").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let id = {
            let registry = registries.assets.read();
            registry.register_asset_type::<GatedTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
            registry
                .asset_id("gated")
                .expect("test asset should have metadata")
        };
        let gate = gated_test_asset_gate(&asset_path);
        let assets = registries.assets.clone();
        let loader = std::thread::spawn(move || {
            assets
                .read()
                .load_by_id::<GatedTestAsset>(id)
                .map(|_| ())
                .map_err(|err| err.to_string())
        });

        gate.0.wait();
        std::fs::remove_file(&asset_path).expect("failed to delete test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
        );
        gate.1.wait();
        let _ = loader.join().expect("loader thread panicked");

        let registry = registries.assets.read();
        assert_eq!(registry.asset_id("gated/Child"), None);
        assert!(registry
            .asset_data()
            .meta
            .values()
            .all(|meta| meta.parent != Some(id)));
        assert!(!root.join("gated.meta").exists());
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn current_meta_files_keep_their_bytes_across_build_and_load() {
        let root = temp_asset_root("calyx-current-meta-bytes");
        let asset_path = root.join("plain.cxtestgated");
        let meta_path = root.join("plain.meta");
        std::fs::write(&asset_path, "plain").expect("failed to write test asset");
        // A first session creates the metadata, sub-asset included.
        {
            let registries = test_registries_with_assets(vec![root.clone()]);
            let registry = registries.assets.read();
            registry.register_asset_type::<GatedTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
            registry
                .load_by_path::<GatedTestAsset>(&asset_path)
                .expect("test asset should load");
        }
        // A Windows checkout with `core.autocrlf=true` has CRLF line endings.
        let written = std::fs::read_to_string(&meta_path).expect("failed to read test meta");
        assert!(
            written.contains("Child"),
            "the meta should list the sub-asset"
        );
        let checked_out = written.replace("\r\n", "\n").replace('\n', "\r\n");
        std::fs::write(&meta_path, &checked_out).expect("failed to write test meta");

        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .load_by_path::<GatedTestAsset>(&asset_path)
            .expect("test asset should load");

        assert_eq!(
            std::fs::read_to_string(&meta_path).expect("failed to read test meta"),
            checked_out,
            "unchanged metadata must not be rewritten"
        );
        drop(registry);
        drop(registries);

        // A real change keeps the file's line endings, so only changed lines differ.
        let stale = checked_out.replace("\"name\": \"plain\"", "\"name\": \"stale\"");
        std::fs::write(&meta_path, &stale).expect("failed to write test meta");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .load_by_path::<GatedTestAsset>(&asset_path)
            .expect("test asset should load");
        assert_eq!(
            std::fs::read_to_string(&meta_path).expect("failed to read test meta"),
            checked_out,
            "changed metadata should be rewritten with the file's line endings"
        );
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn create_event_for_registered_file_keeps_its_generation() {
        let root = temp_asset_root("calyx-create-keeps-generation");
        let (asset_path, _, asset_id) = write_scene_with_meta(&root);
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        let before = registry
            .asset_data()
            .generations
            .get(&asset_id)
            .map(|(generation, _)| *generation);
        assert!(before.is_some());

        registry.recv_notify_event(
            Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(asset_path),
        );

        assert_eq!(
            registry
                .asset_data()
                .generations
                .get(&asset_id)
                .map(|(generation, _)| *generation),
            before
        );
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn reload_failing_after_deletion_records_no_reload_error() {
        let root = temp_asset_root("calyx-failed-reload-after-delete");
        let asset_path = root.join("gated.cxtestgated");
        std::fs::write(&asset_path, "plain").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let id = {
            let registry = registries.assets.read();
            registry.register_asset_type::<GatedTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
            let id = registry
                .asset_id("gated")
                .expect("test asset should have metadata");
            registry
                .load_by_id::<GatedTestAsset>(id)
                .expect("test asset should load");
            id
        };
        std::fs::write(&asset_path, "gate fail").expect("failed to rewrite test asset");
        registries
            .assets
            .read()
            .asset_data_mut()
            .dirty
            .insert(id, Instant::now() - Duration::from_secs(1));
        let gate = gated_test_asset_gate(&asset_path);
        let assets = registries.assets.clone();
        let reloader = std::thread::spawn(move || assets.read().reload_assets());

        gate.0.wait();
        std::fs::remove_file(&asset_path).expect("failed to delete test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
        );
        gate.1.wait();
        reloader.join().expect("reloader thread panicked");

        assert!(registries.assets.read().reload_errors().is_empty());
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn load_spanning_delete_and_recreate_does_not_cache_the_old_file() {
        let root = temp_asset_root("calyx-recreate-during-load");
        let asset_path = root.join("gated.cxtestgated");
        std::fs::write(&asset_path, "gate").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let id = {
            let registry = registries.assets.read();
            registry.register_asset_type::<GatedTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
            registry
                .asset_id("gated")
                .expect("test asset should have metadata")
        };
        let gate = gated_test_asset_gate(&asset_path);
        let assets = registries.assets.clone();
        let loader = std::thread::spawn(move || {
            assets
                .read()
                .load_by_id::<GatedTestAsset>(id)
                .map(|_| ())
                .map_err(|err| err.to_string())
        });

        gate.0.wait();
        std::fs::remove_file(&asset_path).expect("failed to delete test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File))
                .add_path(asset_path.clone()),
        );
        std::fs::write(&asset_path, "gate").expect("failed to recreate test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(asset_path.clone()),
        );
        assert_eq!(registries.assets.read().asset_id("gated"), Some(id));
        gate.1.wait();
        let _ = loader.join().expect("loader thread panicked");

        assert!(registries
            .assets
            .read()
            .loaded_by_id::<GatedTestAsset>(id)
            .is_none());
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn reload_by_id_updates_existing_reference_in_place() {
        let root = temp_asset_root("calyx-reload-in-place");
        let asset_path = root.join("note.cxtesttext");
        std::fs::write(&asset_path, "first").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        registry.register_asset_type::<TextTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        let id = registry
            .asset_id("note")
            .expect("test asset should have metadata");

        let held = registry
            .load_by_id::<TextTestAsset>(id)
            .expect("test asset should load");
        std::fs::write(&asset_path, "second").expect("failed to rewrite test asset");
        let reloaded = registry
            .reload_by_id::<TextTestAsset>(id)
            .expect("test asset should reload");

        assert_eq!(held.ptr_id(), reloaded.ptr_id());
        assert_eq!(held.read().text, "second");
        let cached = registry
            .loaded_by_id::<TextTestAsset>(id)
            .expect("reloaded asset should stay cached");
        assert_eq!(cached.ptr_id(), held.ptr_id());
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn id_moving_to_another_source_loads_the_new_source() {
        let first_root = temp_asset_root("calyx-id-moves-first-root");
        let second_root = temp_asset_root("calyx-id-moves-second-root");
        std::fs::write(first_root.join("note.cxtesttext"), "first")
            .expect("failed to write first asset");
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        registry.register_asset_type::<TextTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        let id = registry
            .asset_id("note")
            .expect("first asset is registered");
        let held = registry
            .load_by_id::<TextTestAsset>(id)
            .expect("first asset should load");
        assert_eq!(held.read().text, "first");

        let second_path = second_root.join("note.cxtesttext");
        std::fs::write(&second_path, "second").expect("failed to write second asset");
        registry.recv_notify_event(
            Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(second_path),
        );

        assert_eq!(registry.asset_id("note"), Some(id));
        let loaded = registry
            .load_by_id::<TextTestAsset>(id)
            .expect("second asset should load");
        assert_eq!(loaded.read().text, "second");
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn id_moving_to_another_source_drops_the_old_sources_sub_assets() {
        let first_root = temp_asset_root("calyx-id-moves-children-first-root");
        let second_root = temp_asset_root("calyx-id-moves-children-second-root");
        let parent_id = Uuid::new_v4();
        let (body_id, arm_id) = (Uuid::new_v4(), Uuid::new_v4());
        let write_parent = |root: &Path, child_id: Uuid, child_name: &str| {
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
                    "inner": [{
                        "id": child_id,
                        "name": child_name,
                        "display_name": child_name,
                        "type_uuid": Mesh::type_uuid(),
                    }]
                }))
                .expect("failed to encode test meta"),
            )
            .expect("failed to write parent meta");
        };
        write_parent(&first_root, body_id, "Body");
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        assert_eq!(registry.asset_id("parent/Body"), Some(body_id));

        write_parent(&second_root, arm_id, "Arm");
        registry.recv_notify_event(
            Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(second_root.join("parent.fbx")),
        );

        assert_eq!(registry.asset_id("parent/Arm"), Some(arm_id));
        assert_eq!(registry.asset_id("parent/Body"), None);
        assert!(registry.asset_meta_from_id(body_id).is_none());
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn id_moving_to_another_source_clears_its_reload_error() {
        let first_root = temp_asset_root("calyx-id-moves-reload-error-first-root");
        let second_root = temp_asset_root("calyx-id-moves-reload-error-second-root");
        let first_path = first_root.join("note.cxtesttext");
        std::fs::write(&first_path, "first").expect("failed to write first asset");
        let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
        let registry = registries.assets.read();
        registry.register_asset_type::<TextTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        let id = registry
            .asset_id("note")
            .expect("first asset is registered");
        let generation = registry
            .asset_data()
            .generations
            .get(&id)
            .map(|(generation, _)| *generation);
        registry.set_current_reload_error(
            generation,
            AssetReloadError {
                id,
                path: registry
                    .asset_meta_from_id(id)
                    .and_then(|meta| meta.path)
                    .expect("first asset has a source path"),
                error: AssetError::LoadError.with_source("test reload failure"),
            },
        );
        assert_eq!(registry.reload_errors().len(), 1);

        let second_path = second_root.join("note.cxtesttext");
        std::fs::write(&second_path, "second").expect("failed to write second asset");
        registry.recv_notify_event(
            Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(second_path),
        );

        assert_eq!(registry.asset_id("note"), Some(id));
        assert!(registry.reload_errors().is_empty());
        drop(registry);
        std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
        std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
    }

    #[test]
    fn asset_moved_with_its_meta_drops_its_old_names() {
        let root = temp_asset_root("calyx-moved-with-meta");
        let (old_path, old_meta_path, asset_id) = write_scene_with_meta(&root);
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        assert_eq!(registry.asset_id("scene"), Some(asset_id));

        std::fs::create_dir_all(root.join("moved")).expect("failed to create target folder");
        let new_path = root.join("moved").join("scene.cxscene");
        std::fs::rename(&old_path, &new_path).expect("failed to move asset");
        std::fs::rename(&old_meta_path, root.join("moved").join("scene.meta"))
            .expect("failed to move asset metadata");
        registry.recv_notify_event(
            Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(new_path),
        );
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(old_path),
        );

        assert_eq!(registry.asset_id("moved/scene"), Some(asset_id));
        assert_eq!(registry.asset_id("scene"), None);
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn deleted_asset_is_evicted_and_recreated_file_loads_fresh() {
        let root = temp_asset_root("calyx-delete-evicts-cache");
        let asset_path = root.join("note.cxtesttext");
        std::fs::write(&asset_path, "first").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        registry.register_asset_type::<TextTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        let id = registry
            .asset_id("note")
            .expect("test asset should have metadata");
        let held = registry
            .load_by_id::<TextTestAsset>(id)
            .expect("test asset should load");

        std::fs::remove_file(&asset_path).expect("failed to delete test asset");
        registry.recv_notify_event(
            Event::new(EventKind::Remove(notify::event::RemoveKind::File))
                .add_path(asset_path.clone()),
        );

        assert!(registry.loaded_by_id::<TextTestAsset>(id).is_none());
        std::fs::write(&asset_path, "second").expect("failed to recreate test asset");
        registry.recv_notify_event(
            Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(asset_path.clone()),
        );
        let recreated_id = registry
            .asset_id("note")
            .expect("recreated asset should have metadata");
        let recreated = registry
            .load_by_id::<TextTestAsset>(recreated_id)
            .expect("recreated asset should load");
        assert_eq!(recreated.read().text, "second");
        assert_eq!(held.read().text, "first");
        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn concurrent_loads_of_one_id_share_one_reference() {
        let root = temp_asset_root("calyx-concurrent-load");
        std::fs::write(root.join("slow.cxtestslow"), "").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let id = {
            let registry = registries.assets.read();
            registry.register_asset_type::<SlowTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
            registry
                .asset_id("slow")
                .expect("test asset should have metadata")
        };
        SLOW_TEST_ASSET_LOADS.store(0, std::sync::atomic::Ordering::SeqCst);

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let assets = registries.assets.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    assets
                        .read()
                        .load_by_id::<SlowTestAsset>(id)
                        .expect("test asset should load")
                        .ptr_id()
                })
            })
            .collect::<Vec<_>>();
        let ptrs = handles
            .into_iter()
            .map(|handle| handle.join().expect("loader thread panicked"))
            .collect::<Vec<_>>();

        assert_eq!(ptrs[0], ptrs[1]);
        assert_eq!(
            SLOW_TEST_ASSET_LOADS.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    /// Asset whose file names another asset of this type that it loads, slowly
    /// enough for two threads to each hold one slot of a cycle.
    struct CyclicTestAsset;

    impl TypeUuid for CyclicTestAsset {
        fn type_uuid() -> Uuid {
            Uuid::from_u128(0x2c7a_94e1_5f30_4b8d_a1c6_0e9f_37b2_d815)
        }
    }

    impl Asset for CyclicTestAsset {
        fn file_extensions() -> &'static [&'static str] {
            &["cxtestcycle"]
        }

        fn from_file(
            assets: &ReadOnlyAssetContext,
            path: &Path,
        ) -> Result<LoadedAsset<Self>, AssetError> {
            let other = std::fs::read_to_string(path)
                .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
            std::thread::sleep(Duration::from_millis(200));
            assets
                .registries
                .assets
                .read()
                .load::<CyclicTestAsset>(other.trim())?;
            Ok(LoadedAsset::new(Self))
        }
    }

    #[test]
    fn concurrent_mutually_dependent_loads_fail_instead_of_deadlocking() {
        let root = temp_asset_root("calyx-concurrent-cycle");
        std::fs::write(root.join("a.cxtestcycle"), "b").expect("failed to write test asset");
        std::fs::write(root.join("b.cxtestcycle"), "a").expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        {
            let registry = registries.assets.read();
            registry.register_asset_type::<CyclicTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
        }

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        for name in ["a", "b"] {
            let assets = registries.assets.clone();
            let barrier = barrier.clone();
            let done_tx = done_tx.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let result = assets.read().load::<CyclicTestAsset>(name).map(|_| ());
                let _ = done_tx.send(result);
            });
        }
        drop(done_tx);

        for _ in 0..2 {
            let result = done_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("concurrent cyclic loads deadlocked");
            assert!(result.is_err());
        }
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

    /// Registers [`GatedTestAsset`] over a temp root holding `gated.cxtestgated`
    /// with `content`, returning the root, the file path, the registries and
    /// the asset id.
    fn gated_background_fixture(
        prefix: &str,
        content: &str,
    ) -> (PathBuf, PathBuf, ReadOnlyRegistryContext, Uuid) {
        let root = temp_asset_root(prefix);
        let asset_path = root.join("gated.cxtestgated");
        std::fs::write(&asset_path, content).expect("failed to write test asset");
        let registries = test_registries_with_assets(vec![root.clone()]);
        let id = {
            let registry = registries.assets.read();
            registry.register_asset_type::<GatedTestAsset>();
            registry.build_meta().expect("failed to rebuild metadata");
            registry
                .asset_id("gated")
                .expect("test asset should have metadata")
        };
        (root, asset_path, registries, id)
    }

    /// Polls the background load of `id` until it stops reporting `Loading`.
    fn poll_until_settled<A: Asset + TypeUuid>(
        registries: &ReadOnlyRegistryContext,
        id: Uuid,
    ) -> BackgroundLoad<A> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = registries.assets.read().load_by_id_in_background::<A>(id);
            if !matches!(status, BackgroundLoad::Loading) {
                return status;
            }
            assert!(Instant::now() < deadline, "background load never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn background_load_returns_at_once_and_loads_once_on_another_thread() {
        let (root, asset_path, registries, id) =
            gated_background_fixture("calyx-background-load", "gate");
        let gate = gated_test_asset_gate(&asset_path);

        // The load blocks on the gate, so a blocking call would hang here.
        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Loading
        ));
        gate.0.wait();
        for _ in 0..3 {
            assert!(matches!(
                registries
                    .assets
                    .read()
                    .load_by_id_in_background::<GatedTestAsset>(id),
                BackgroundLoad::Loading
            ));
        }
        gate.1.wait();

        let BackgroundLoad::Loaded(loaded) = poll_until_settled::<GatedTestAsset>(&registries, id)
        else {
            panic!("background load should have succeeded");
        };
        let cached = registries
            .assets
            .read()
            .load_by_id::<GatedTestAsset>(id)
            .expect("asset should be cached");
        assert_eq!(loaded.ptr_id(), cached.ptr_id());
        assert_eq!(gated_test_asset_loads(&asset_path), 1);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn failed_background_load_is_kept_until_its_source_changes() {
        let (root, asset_path, registries, id) =
            gated_background_fixture("calyx-background-load-failure", "gate fail");
        let gate = gated_test_asset_gate(&asset_path);

        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Loading
        ));
        gate.0.wait();
        gate.1.wait();
        assert!(matches!(
            poll_until_settled::<GatedTestAsset>(&registries, id),
            BackgroundLoad::Failed(_)
        ));
        // A retry would start another gated load and report `Loading`.
        for _ in 0..3 {
            assert!(matches!(
                registries
                    .assets
                    .read()
                    .load_by_id_in_background::<GatedTestAsset>(id),
                BackgroundLoad::Failed(_)
            ));
        }
        assert_eq!(gated_test_asset_loads(&asset_path), 1);

        std::fs::write(&asset_path, "fixed").expect("failed to rewrite test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                .add_path(asset_path.clone()),
        );
        assert!(matches!(
            poll_until_settled::<GatedTestAsset>(&registries, id),
            BackgroundLoad::Loaded(_)
        ));
        assert_eq!(gated_test_asset_loads(&asset_path), 2);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn source_change_during_a_background_load_retries_its_failure() {
        let (root, asset_path, registries, id) =
            gated_background_fixture("calyx-background-load-change", "gate fail");
        let gate = gated_test_asset_gate(&asset_path);

        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Loading
        ));
        // The load has read the failing source; fix it before it finishes.
        gate.0.wait();
        std::fs::write(&asset_path, "fixed").expect("failed to rewrite test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                .add_path(asset_path.clone()),
        );
        gate.1.wait();

        assert!(matches!(
            poll_until_settled::<GatedTestAsset>(&registries, id),
            BackgroundLoad::Loaded(_)
        ));
        assert_eq!(gated_test_asset_loads(&asset_path), 2);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn read_access_events_keep_a_background_load_failure() {
        let (root, asset_path, registries, id) =
            gated_background_fixture("calyx-background-load-access", "gate fail");
        let gate = gated_test_asset_gate(&asset_path);
        let open_event = || {
            Event::new(EventKind::Access(notify::event::AccessKind::Open(
                notify::event::AccessMode::Any,
            )))
            .add_path(asset_path.clone())
        };

        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Loading
        ));
        // inotify reports the loader opening its own source as an access.
        gate.0.wait();
        registries.assets.read().recv_notify_event(open_event());
        gate.1.wait();
        assert!(matches!(
            poll_until_settled::<GatedTestAsset>(&registries, id),
            BackgroundLoad::Failed(_)
        ));

        registries.assets.read().recv_notify_event(open_event());
        // A retry would start another gated load and report `Loading`.
        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Failed(_)
        ));
        assert_eq!(gated_test_asset_loads(&asset_path), 1);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn source_change_during_a_successful_background_load_reloads_it() {
        let (root, asset_path, registries, id) =
            gated_background_fixture("calyx-background-load-replace", "gate");
        let gate = gated_test_asset_gate(&asset_path);

        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Loading
        ));
        // The load has read the old source; replace it before it finishes
        // and let the debounce consume the dirty mark while it is uncached.
        gate.0.wait();
        std::fs::write(&asset_path, "replaced").expect("failed to rewrite test asset");
        registries.assets.read().recv_notify_event(
            Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                .add_path(asset_path.clone()),
        );
        std::thread::sleep(HOT_RELOAD_DEBOUNCE * 2);
        registries.assets.read().reload_assets();
        gate.1.wait();
        assert!(matches!(
            poll_until_settled::<GatedTestAsset>(&registries, id),
            BackgroundLoad::Loaded(_)
        ));
        assert_eq!(gated_test_asset_loads(&asset_path), 1);

        // The stale load must leave a pending reload of the replacement.
        std::thread::sleep(HOT_RELOAD_DEBOUNCE * 2);
        registries.assets.read().reload_assets();
        assert_eq!(gated_test_asset_loads(&asset_path), 2);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }
}
