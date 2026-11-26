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
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::assets::animation_graph::AnimationGraph;
use crate::assets::error::{AssetError, AssetErrorKind};
use crate::assets::import;
use crate::assets::material::{Material, MaterialTexture};
use crate::assets::mesh::Mesh;
use crate::assets::post_process_profile::PostProcessProfile;
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

mod meta_sync;

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
    /// Files the watcher reported without a `.meta`, with when it last
    /// reported each; see [`meta_sync`].
    pending: HashMap<PathBuf, Instant>,
    /// Ids assets had before their `.meta` changed on disk to name another,
    /// with the id each moved to; see [`meta_sync`].
    aliases: HashMap<Uuid, Uuid>,
}

/// A write lock on the registry's metadata or cache that moves the
/// registry's [`AssetRegistry::change_epoch`] on when it is released after a
/// write, and its [`AssetRegistry::meta_epoch`] too when it was taken to
/// register or forget asset files. Reading through it changes nothing.
struct ChangeGuard<'a, T> {
    guard: RwLockWriteGuard<'a, T>,
    epoch: &'a AtomicU64,
    meta_epoch: Option<&'a AtomicU64>,
    changed: bool,
}

impl<T> Deref for ChangeGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for ChangeGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.changed = true;
        &mut self.guard
    }
}

impl<T> Drop for ChangeGuard<'_, T> {
    fn drop(&mut self) {
        if self.changed {
            self.epoch.fetch_add(1, Ordering::AcqRel);
            if let Some(meta_epoch) = self.meta_epoch {
                meta_epoch.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
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

thread_local! {
    /// The ids this thread put into a registry's cache since the innermost
    /// live [`AssetLoadRecord`] started, or `None` when none is live.
    static LOAD_RECORD: RefCell<Option<Vec<Uuid>>> = const { RefCell::new(None) };
}

/// Records which assets the current thread puts into the registry's cache
/// while it lives: each asset a load reads from its file, and each sub-asset
/// such a load creates. An asset that was already cached is not recorded, so
/// [`AssetRegistry::evict_unheld`] given what this records only drops what
/// the recorded work loaded itself.
///
/// Loads on other threads are not recorded, background loads included.
/// Records nest: an inner record's ids are also an outer record's.
#[must_use = "a record stops recording when it is dropped"]
pub struct AssetLoadRecord {
    /// What the enclosing record had recorded when this one started.
    outer: Option<Option<Vec<Uuid>>>,
}

impl AssetLoadRecord {
    /// Starts recording the current thread's cache insertions.
    pub fn start() -> Self {
        let outer = LOAD_RECORD.with(|record| record.replace(Some(Vec::new())));
        Self { outer: Some(outer) }
    }

    /// Stops recording and returns the ids recorded, in insertion order.
    pub fn finish(mut self) -> Vec<Uuid> {
        self.stop()
    }

    fn stop(&mut self) -> Vec<Uuid> {
        let Some(outer) = self.outer.take() else {
            return Vec::new();
        };
        let ids = LOAD_RECORD
            .with(|record| record.replace(outer))
            .unwrap_or_default();
        LOAD_RECORD.with(|record| {
            if let Some(outer) = record.borrow_mut().as_mut() {
                outer.extend_from_slice(&ids);
            }
        });
        ids
    }

    /// Records that this thread cached `id`, when a record is live.
    fn cached(id: Uuid) {
        LOAD_RECORD.with(|record| {
            if let Some(ids) = record.borrow_mut().as_mut() {
                ids.push(id);
            }
        });
    }
}

impl Drop for AssetLoadRecord {
    fn drop(&mut self) {
        self.stop();
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
    /// Counts the writes to the asset metadata and cache, so a caller can
    /// keep what it resolved through the registry until the next one; see
    /// [`Self::change_epoch`].
    change_epoch: AtomicU64,
    /// Counts the writes that register, forget, rename or move asset files;
    /// see [`Self::meta_epoch`].
    meta_epoch: AtomicU64,
    /// Counts the watcher's reports of files and folders created, removed
    /// or renamed under the asset roots; see [`Self::file_change_count`].
    file_changes: AtomicU64,
    /// See [`Self::source_change_count`].
    source_changes: AtomicU64,
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
                change_epoch: Default::default(),
                meta_epoch: Default::default(),
                file_changes: Default::default(),
                source_changes: Default::default(),
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
        registry_ref.write().watcher_thread = Some(std::thread::spawn(move || loop {
            match watcher_rx.recv_timeout(meta_sync::SETTLE_POLL) {
                Ok(Ok(event)) => watcher_registry_ref.read().recv_notify_event(event),
                Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            watcher_registry_ref
                .read()
                .register_settled_files(Instant::now());
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
            change_epoch: Default::default(),
            meta_epoch: Default::default(),
            file_changes: Default::default(),
            source_changes: Default::default(),
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
                change_epoch: Default::default(),
                meta_epoch: Default::default(),
                file_changes: Default::default(),
                source_changes: Default::default(),
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
        self.register_asset_type::<PostProcessProfile>();
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

    /// How many assets the cache holds, of every type.
    pub fn loaded_count(&self) -> usize {
        self.asset_cache().len()
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
        let id = self.resolve_asset_id(id);
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
            report_missing_asset(id);
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
        self.load_asset(self.resolve_asset_id(id), LoadMode::Load, None)
    }

    fn load_typed<A: Asset + TypeUuid>(
        &self,
        id: Uuid,
        mode: LoadMode,
    ) -> Result<Ref<A>, AssetError> {
        let id = self.resolve_asset_id(id);
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
        let Some(meta) = meta else {
            report_missing_asset(id);
            return Err(AssetError::NotFound.with_source(format!("asset id {id}")));
        };
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
                AssetLoadRecord::cached(id);
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

    /// Drops from the cache the assets of `loaded` that nothing outside the
    /// cache holds, and returns how many it dropped. `loaded` is what an
    /// [`AssetLoadRecord`] recorded, so an asset cached before that work
    /// started is never dropped.
    ///
    /// An asset goes with its sub-assets: only a recorded asset with no
    /// parent is considered, together with its sub-assets, and a sub-asset
    /// recorded under a parent cached before is kept, since it could not be
    /// loaded again while its parent stays cached. Shaders are kept: they
    /// are shared by every material that uses them, and compiling them is
    /// the slow part of loading them.
    ///
    /// An asset that something else holds stays cached, and so does every
    /// asset it holds. The assets are taken out of the cache and their strong
    /// references dropped under the cache's lock, and those that are still
    /// alive afterwards are put back, so the check never misses a reference
    /// one evicted asset holds to another. When a parent stays alive but one
    /// of its sub-assets does not, the whole family leaves the cache, so the
    /// next load reads it again in full; its holders keep their copy.
    pub fn evict_unheld(&self, loaded: &[Uuid]) -> usize {
        let families = {
            let data = self.asset_data();
            let mut seen = HashSet::new();
            loaded
                .iter()
                .filter(|id| seen.insert(**id))
                .filter_map(|id| data.meta.get(id))
                .filter(|meta| meta.parent.is_none() && meta.type_uuid != Shader::type_uuid())
                .map(|meta| {
                    let mut members = vec![meta.id];
                    members.extend(meta.children.iter().copied());
                    (meta.id, members)
                })
                .collect::<Vec<_>>()
        };
        if families.is_empty() {
            return 0;
        }

        let mut cache = self.asset_cache_mut();
        let taken = families
            .into_iter()
            .map(|(root, members)| {
                let members = members
                    .into_iter()
                    .filter_map(|id| cache.remove(&id).map(|asset| (id, asset.downgrade())))
                    .collect::<Vec<_>>();
                (root, members)
            })
            .collect::<Vec<_>>();

        let mut evicted = 0;
        for (root, members) in taken {
            let alive = members
                .into_iter()
                .map(|(id, asset)| (id, asset.upgrade()))
                .collect::<Vec<_>>();
            let root_alive = alive
                .iter()
                .any(|(id, asset)| *id == root && asset.is_some());
            let all_alive = alive.iter().all(|(_, asset)| asset.is_some());
            if root_alive && !all_alive {
                evicted += alive.len();
                continue;
            }
            for (id, asset) in alive {
                match asset {
                    Some(asset) => {
                        cache.insert(id, asset);
                    }
                    None => evicted += 1,
                }
            }
        }
        evicted
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
        AssetLoadRecord::cached(id);
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
                    let LoadedAsset {
                        mut asset,
                        sub_assets,
                    } = A::from_file(&assets, path)?;
                    let asset_ref = match existing {
                        Some(asset_ref) => {
                            asset.accept_reload(&asset_ref.read())?;
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
        self.mark_dependents_dirty(path);
    }

    /// Marks dirty every asset whose load reads the file at `path`, or a
    /// file beneath it when it is a folder, such as a shader including it or
    /// a model reading it as an image. A deleted folder arrives as one event
    /// for the folder alone.
    fn mark_dependents_dirty(&self, path: &Path) {
        let dependency_path = Self::dependency_path(path);
        let dependent_ids: HashSet<Uuid> = self
            .asset_data()
            .dependencies
            .iter()
            .filter(|(dependency, _)| dependency.starts_with(&dependency_path))
            .flat_map(|(_, dependents)| dependents.iter().copied())
            .collect();
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

    /// A write lock on the metadata for bookkeeping that leaves the asset
    /// files' registrations alone: dirty marks, reload errors, dependencies,
    /// waiting files, and the sub-assets and in-memory assets a load
    /// records. It moves only [`Self::change_epoch`].
    fn asset_data_mut(&self) -> ChangeGuard<'_, AssetData> {
        ChangeGuard {
            guard: self.asset_data.write().unwrap(),
            epoch: &self.change_epoch,
            meta_epoch: None,
            changed: false,
        }
    }

    /// A write lock on the metadata to register, forget, rename or move
    /// asset files. It moves [`Self::meta_epoch`] as well as
    /// [`Self::change_epoch`].
    fn asset_meta_mut(&self) -> ChangeGuard<'_, AssetData> {
        ChangeGuard {
            guard: self.asset_data.write().unwrap(),
            epoch: &self.change_epoch,
            meta_epoch: Some(&self.meta_epoch),
            changed: false,
        }
    }

    fn asset_cache(&self) -> RwLockReadGuard<'_, AssetCache> {
        self.asset_cache.read().unwrap()
    }

    fn asset_cache_mut(&self) -> ChangeGuard<'_, AssetCache> {
        ChangeGuard {
            guard: self.asset_cache.write().unwrap(),
            epoch: &self.change_epoch,
            meta_epoch: None,
            changed: false,
        }
    }

    /// A counter that moves on whenever an asset is registered, forgotten,
    /// loaded, reloaded, renamed or evicted: any write to the metadata or the
    /// cache. While it holds still, resolving an asset id gives the same
    /// reference, or the same failure, as last time, so callers that resolve
    /// many references every frame can keep their results until it moves.
    pub fn change_epoch(&self) -> u64 {
        self.change_epoch.load(Ordering::Acquire)
    }

    /// A counter that moves on whenever an asset file is registered,
    /// forgotten, renamed or moved, or its `.meta` gives it another id: a
    /// write to which files are assets and which ids they have. Loading,
    /// reloading or evicting an asset leaves it alone, and so do the
    /// sub-assets a load records, so a listing of the asset folders that
    /// resolves each file to its asset stays current while it and
    /// [`Self::file_change_count`] hold still.
    pub fn meta_epoch(&self) -> u64 {
        self.meta_epoch.load(Ordering::Acquire)
    }

    /// A counter that moves on whenever the watcher reports a file or folder
    /// created, removed or renamed under the asset roots, whether or not it
    /// is an asset. While it holds still, a listing of the asset folders is
    /// current.
    pub fn file_change_count(&self) -> u64 {
        self.file_changes.load(Ordering::Acquire)
    }

    /// A counter that moves on whenever the watcher reports a file or folder
    /// created, written, removed or renamed under the asset roots, whether
    /// or not the asset is loaded. While it holds still, no asset's source
    /// changed on disk, so what was read from the sources, such as a
    /// thumbnail's source version, is current.
    pub fn source_change_count(&self) -> u64 {
        self.source_changes.load(Ordering::Acquire)
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

    /// The render context and registries asset constructors take, for code that builds
    /// GPU assets at run time, such as a texture gameplay draws into.
    pub fn asset_context(&self) -> ReadOnlyAssetContext {
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
        if matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            self.source_changes.fetch_add(1, Ordering::AcqRel);
        }
        if matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        ) {
            self.file_changes.fetch_add(1, Ordering::AcqRel);
        }
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
                    self.touch_pending_file(file);
                    self.mark_path_dirty(file);
                }
            }
            EventKind::Remove(_) => {
                // A deleted folder arrives as one extensionless path that
                // `notify_event_paths` drops, so forget by path prefix here.
                for removed in event.paths.iter().filter(|path| !path.exists()) {
                    self.forget_pending_under(removed);
                    self.forget_assets_under(removed);
                    self.mark_dependents_dirty(removed);
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
                    // Only an asset file has a `.meta`. One beside a deleted
                    // file of another type belongs to a same-stem asset.
                    if !self.is_asset_file(file) {
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
        // A `.meta` written or changed by another tool names the asset's id.
        if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
            for meta_path in event
                .paths
                .iter()
                .filter(|path| path.extension().is_some_and(|ext| ext == "meta") && path.is_file())
            {
                self.sync_meta_file(meta_path);
            }
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
        // A file an asset already read, but found missing, is new to it too.
        self.mark_dependents_dirty(file);
        let Some(asset_path) = self.owning_asset_root(file) else {
            log::warn!(
                "Ignoring created file outside every asset root: {}",
                file.display()
            );
            return;
        };
        if !self.is_asset_file(file) {
            log::trace!("Ignoring created file of no asset type: {}", file.display());
            return;
        }
        // A tool writing a file and its `.meta` may write the `.meta` second;
        // wait for it rather than hand out a generated id.
        let meta_path = file.with_extension("meta");
        if !meta_path.exists() && !self.is_registered_at(file) {
            self.defer_created_file(file);
            return;
        }
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
        // The writer registered it, so it no longer waits for a `.meta`.
        self.asset_data_mut().pending.remove(&path);
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
        Self::canonical_through_ancestor(path, |path| dunce::canonicalize(path))
    }

    /// `path` passed through `canonicalize` when it exists. Otherwise its
    /// nearest existing ancestor is, and the missing rest is appended to it,
    /// so the path matches the canonical path it gets once created.
    fn canonical_through_ancestor(
        path: &Path,
        canonicalize: impl Fn(&Path) -> std::io::Result<PathBuf>,
    ) -> Option<PathBuf> {
        if let Ok(path) = canonicalize(path) {
            return Some(path);
        }
        let absolute = path.absolutize().ok()?.into_owned();
        let mut existing = absolute.as_path();
        let mut rest = Vec::new();
        while !existing.exists() {
            rest.push(existing.file_name()?.to_os_string());
            existing = existing.parent()?;
        }
        let mut normalized = canonicalize(existing).ok()?;
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
        let mut ids: HashSet<Uuid> = HashSet::new();
        for meta in self.asset_data().meta.values() {
            if meta.path.as_deref().is_some_and(|p| p.starts_with(path)) {
                ids.insert(meta.id);
                ids.extend(meta.children.iter().copied());
            }
        }
        if ids.is_empty() {
            return;
        }
        let freed = self.drop_asset_ids(&ids);
        self.register_sources_named(&freed);
        log::info!("Forgot deleted assets under {}", path.display());
    }

    /// Drops the in-memory metadata, names, dirty marks, reload errors,
    /// generations, dependency entries and cached values of `ids`, and
    /// returns the names they held. Holders keep their own refs.
    fn drop_asset_ids(&self, ids: &HashSet<Uuid>) -> HashSet<RelativePathBuf> {
        let mut data = self.asset_meta_mut();
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
        freed
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
                    if !self.is_asset_file(&candidate) || !candidate.is_file() {
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

    /// Whether the file at `path` is an asset: its extension names a
    /// registered asset type. Only an asset file gets a `.meta`.
    fn is_asset_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| self.asset_type_uuid_from_ext(ext).is_some())
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
                // Only a file of a registered asset type gets a `.meta`.
                if !self.is_asset_file(&path) {
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
        // Resolved before a new `.meta` is written, so the file records the
        // real type, and a file of no registered type gets none.
        let Some(type_uuid) = self.asset_type_uuid_from_ext(ext) else {
            return Err(format!(
                "{} has no registered asset type for its `.{ext}` extension",
                path.display()
            )
            .into());
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
                    type_uuid,
                    name: relative_path.with_extension("").to_string(),
                    display_name,
                    parent: None,
                    children: Default::default(),
                    path: None,
                },
                inner: Default::default(),
            };
            // A `.meta` another tool wrote since the check above wins.
            if Self::write_new_meta_file(meta_path, &meta)? {
                meta.main
            } else {
                self.load_meta_file(asset_path, meta_path)?.main
            }
        };
        meta.type_uuid = type_uuid;
        meta.path = Some(match path.absolutize().map_err(Box::new)? {
            Cow::Borrowed(p) => p.to_path_buf(),
            Cow::Owned(p) => p,
        });
        let id = meta.id;
        let source = meta.path.clone().unwrap_or_default();
        let mut data = self.asset_meta_mut();
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
            // A model reads files besides its own, such as a glTF's images.
            let dependencies = import::dependencies(path);
            let mut data = self.asset_data_mut();
            for dependency in dependencies {
                data.dependencies
                    .entry(Self::dependency_path(&dependency))
                    .or_default()
                    .insert(id);
            }
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

    /// The key `path` has in the dependencies: its canonical path, in the
    /// [`std::fs::canonicalize`] form that shader includes are keyed by. A
    /// file that does not exist yet goes through its nearest existing
    /// ancestor, so it keys the same once it and its folders are created.
    fn dependency_path(path: &Path) -> PathBuf {
        Self::canonical_through_ancestor(path, |path| std::fs::canonicalize(path))
            .unwrap_or_else(|| path.to_path_buf())
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
        let mut meta = Self::read_meta_file(meta_path)?;
        // Child names may carry the stored main name as a legacy prefix, so
        // strip it before the main name is rebased onto the file's location
        // under its owning root (an older scan may have used an outer root).
        for child in meta.inner.iter_mut() {
            child.name = Self::local_sub_asset_name(&meta.main.name, &child.name);
        }
        meta.main.name = name_path.to_string();
        let mut data = self.asset_meta_mut();
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
    ///
    /// A `.meta` naming another asset id is left as it is: another tool
    /// wrote it, and the watcher re-reads it and re-keys the asset.
    fn write_meta_file(&self, meta_path: &Path, meta: &AssetMetaData) -> Result<(), BoxedError> {
        if let Some(on_disk) = Self::meta_names_another_id(meta_path, meta.main.id) {
            log::info!(
                "Keeping {}: it names asset id {}, not {}",
                meta_path.display(),
                on_disk,
                meta.main.id
            );
            return Ok(());
        }
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
                if error.kind == AssetErrorKind::Incomplete {
                    // The next write of the source marks it dirty again.
                    log::info!(
                        "Skipping hot reload of asset {} ({}) from {}: {}",
                        meta.name,
                        id,
                        path.display(),
                        error
                    );
                    continue;
                }
                log::error!(
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
                    ..Default::default()
                },
            );
            scene
        })
    }
}

/// Logs an error the first time something asks for an asset id no asset
/// root registers, such as a scene's mesh whose file is missing, and returns
/// whether it did. Components resolve their asset references every frame, so
/// each id is reported once. The nil id is an unset reference, not a missing
/// asset.
fn report_missing_asset(id: Uuid) -> bool {
    static REPORTED: std::sync::OnceLock<Mutex<HashSet<Uuid>>> = std::sync::OnceLock::new();
    if id.is_nil() {
        return false;
    }
    let first = REPORTED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(id);
    if first {
        log::error!(
            "No asset with id {id} is registered: something refers to an asset whose file is missing, so it is skipped"
        );
    }
    first
}

#[cfg(test)]
mod tests;
