use eframe::wgpu;
use glm::{vec2, vec3};
use glob::glob;
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
use uuid::Uuid;

use crate::assets::animation_graph::AnimationGraph;
use crate::assets::error::AssetError;
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture;
use crate::assets::Asset;
use crate::assets::LoadedAssetRef;
use crate::class_registry::ComponentRegistry;
use crate::component::ComponentMesh;
use crate::context::ReadOnlyAssetContext;
use crate::core::{Ref, WeakRef};
use crate::error::BoxedError;
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{AttributeValue, TypeInfo};
use crate::render::{RenderContext, Shader};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    #[serde(skip)]
    pub parent: Option<Uuid>,
    #[serde(skip)]
    pub children: Vec<Uuid>,
    #[serde(skip)]
    pub type_id: Option<(TypeId, Uuid, &'static str)>,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
pub struct AssetMetaData {
    main: AssetMeta,
    inner: Vec<AssetMeta>,
}

#[derive(Default)]
struct AssetData {
    meta: HashMap<Uuid, AssetMeta>,
    names: HashMap<RelativePathBuf, Uuid>,
    extensions: HashMap<String, (TypeId, Uuid, &'static str)>,
    dirty: HashSet<Uuid>,
}

struct AssetConstructors {
    create: AssetConstructor,
    reload: AssetReload,
}

pub struct AssetRegistry {
    render_context: Arc<RenderContext>,
    asset_registry: WeakRef<AssetRegistry>,
    type_registry: Ref<TypeRegistry>,
    component_registry: Ref<ComponentRegistry>,
    asset_paths: Vec<PathBuf>,
    asset_cache: RwLock<AssetCache>,
    asset_data: RwLock<AssetData>,
    asset_constructors: RwLock<HashMap<Uuid, AssetConstructors>>,
    watcher_thread: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    watcher: RecommendedWatcher,
}

impl AssetRegistry {
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
        println!("{:?}", asset_paths);
        let mut watcher = RecommendedWatcher::new(tx, Config::default()).map_err(Box::new)?;
        for path in asset_paths.iter() {
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
    fn register_default_asset_types(&mut self) {
        self.register_asset_type::<Mesh>();
        self.register_asset_type::<Shader>();
        self.register_asset_type::<Texture>();
        self.register_asset_type::<Material>();
        self.register_asset_type::<Prefab>();
        self.register_asset_type::<Scene>();
        self.register_asset_type::<Skybox>();
        self.register_asset_type::<AnimationGraph>();
    }
}

impl AssetRegistry {
    pub fn root_path(&self) -> &PathBuf {
        &self.asset_paths[0]
    }

    pub fn asset_paths(&self) -> &Vec<PathBuf> {
        &self.asset_paths
    }

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

    pub fn load<A: Asset + TypeUuid>(&self, name: &str) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id(name).ok_or(AssetError::NotFound)?;
        self.load_by_id(id)
    }

    pub fn load_by_path<A: Asset + TypeUuid>(&self, path: &Path) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id_from_path(path).ok_or(AssetError::NotFound)?;
        self.load_by_id(id)
    }

    pub fn load_dyn_by_path(&self, path: &Path) -> Result<Ref<dyn Asset>, AssetError> {
        let id = self.asset_id_from_path(path).ok_or(AssetError::NotFound)?;
        self.load_dyn_by_id(id)
    }

    pub fn load_by_id<A: Asset + TypeUuid>(&self, id: Uuid) -> Result<Ref<A>, AssetError> {
        // Load parent asset if any
        let meta = self.asset_meta_from_id(id).ok_or(AssetError::NotFound)?;
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
            .ok_or(AssetError::NotFound)?;
        let asset = self.load_asset_file(id, &path)?;

        // Create ref
        self.asset_cache_mut().insert(id, asset.as_asset());
        Ok(asset)
    }

    pub fn load_dyn_by_id(&self, id: Uuid) -> Result<Ref<dyn Asset>, AssetError> {
        // Load parent asset if any
        let meta = self.asset_meta_from_id(id).ok_or(AssetError::NotFound)?;
        if let Some(parent_id) = meta.parent {
            self.load_dyn_by_id(parent_id)?;
        }

        // Asset already loaded
        if let Some(asset_ref) = self.asset_cache().get(&id) {
            return Ok((*asset_ref).clone());
        }

        // Find constructor & file path
        let (_, type_uuid, _) = meta.type_id.as_ref().ok_or(AssetError::NotFound)?;
        let path = meta.path.as_ref().ok_or(AssetError::NotFound)?;
        let ctors = self.asset_constructors();
        let ctor = ctors.get(type_uuid).ok_or(AssetError::NotFound)?;
        let LoadedAssetRef { asset, sub_assets } = (ctor.create)(self.game_context(), id, path)?;

        // Load from file
        self.load_sub_asset_meta(id, sub_assets);
        self.asset_cache_mut().insert(id, asset.clone());
        Ok(asset)
    }

    pub fn create<A: Asset + TypeUuid>(
        &self,
        name: String,
        value: A,
    ) -> Result<Ref<A>, AssetError> {
        if self.asset_id(name.as_str()).is_some() {
            return Err(AssetError::AlreadyExists);
        }
        let id = utils::uuid_from_str(name.as_str());
        let asset = Ref::from_id_value(id, value);
        let registry = self.type_registry.read();
        let display_name: Option<String> = registry.type_info::<A>().and_then(|info| {
            if let TypeInfo::Struct(info) = info {
                if let Some(AttributeValue::String(str)) = info.attr("name") {
                    return Some(str.to_string());
                }
            }
            None
        });
        let mut data = self.asset_data_mut();
        data.names
            .insert(RelativePathBuf::from(name.as_str()).normalize(), id);
        data.meta.insert(
            id,
            AssetMeta {
                id,
                type_id: Some((TypeId::of::<A>(), A::type_uuid(), A::asset_name())),
                display_name: display_name.unwrap_or(name.clone()),
                name,
                parent: None,
                children: Default::default(),
                path: None,
            },
        );
        self.asset_cache_mut().insert(id, asset.as_asset());
        Ok(asset)
    }

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
                        Err(AssetError::LoadError)
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
                let _ = self.write_meta_file(&meta_path, &meta);
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
        let loaded_asset = A::from_file(&self.game_context(), path)?;
        let LoadedAssetRef { asset, sub_assets } = LoadedAssetRef::new(id, loaded_asset);
        self.load_sub_asset_meta(id, sub_assets);
        Ok(asset)
    }

    fn mark_asset_dirty(&self, id: Uuid) {
        self.asset_data_mut().dirty.insert(id);
    }
}

impl AssetRegistry {
    fn asset_data(&self) -> RwLockReadGuard<AssetData> {
        self.asset_data.read().unwrap()
    }

    fn asset_data_mut(&self) -> RwLockWriteGuard<AssetData> {
        self.asset_data.write().unwrap()
    }

    fn asset_cache(&self) -> RwLockReadGuard<AssetCache> {
        self.asset_cache.read().unwrap()
    }

    fn asset_cache_mut(&self) -> RwLockWriteGuard<AssetCache> {
        self.asset_cache.write().unwrap()
    }

    fn asset_constructors(&self) -> RwLockReadGuard<HashMap<Uuid, AssetConstructors>> {
        self.asset_constructors.read().unwrap()
    }

    fn asset_constructors_mut(&self) -> RwLockWriteGuard<HashMap<Uuid, AssetConstructors>> {
        self.asset_constructors.write().unwrap()
    }

    fn game_context(&self) -> ReadOnlyAssetContext {
        ReadOnlyAssetContext {
            render_context: self.render_context.clone(),
            asset_registry: self.asset_registry.upgrade().unwrap().readonly(),
            type_registry: self.type_registry.readonly(),
            component_registry: self.component_registry.readonly(),
        }
    }
}

impl AssetRegistry {
    fn recv_notify_event(&self, event: Event) {
        let paths_iter = Self::notify_event_paths(&event);
        match event.kind {
            EventKind::Create(_) => {
                for file in paths_iter {
                    let _ =
                        self.build_asset_meta(self.root_path(), file, &file.with_extension("meta"));
                }
            }
            EventKind::Modify(_) => {
                for file in paths_iter {
                    if let Some(id) = self.asset_id_from_path(file) {
                        self.mark_asset_dirty(id);
                    }
                }
            }
            EventKind::Remove(_) => {
                for file in paths_iter {
                    let _ = std::fs::remove_file(file.with_extension("meta"));
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
            .filter_map(|(ext, f)| if ext != "meta" { Some(f) } else { None })
    }

    pub fn asset_id(&self, name: &str) -> Option<Uuid> {
        let path = RelativePathBuf::from(name).normalize();
        self.asset_data().names.get(&path).copied()
    }

    pub fn asset_name(&self, id: Uuid) -> String {
        self.asset_meta_from_id(id)
            .map(|meta| meta.name.clone())
            .unwrap_or_default()
    }

    pub fn asset_name_from_path(&self, path: &Path) -> Option<String> {
        self.asset_id_from_path(path).map(|id| self.asset_name(id))
    }

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

    pub fn asset_meta(&self, name: &str) -> Option<AssetMeta> {
        let id = self.asset_id(name)?;
        self.asset_meta_from_id(id)
    }

    pub fn asset_meta_from_path(&self, path: &Path) -> Option<AssetMeta> {
        self.asset_id_from_path(path)
            .and_then(|id| self.asset_meta_from_id(id))
    }

    pub fn asset_meta_from_id(&self, id: Uuid) -> Option<AssetMeta> {
        self.asset_data().meta.get(&id).cloned()
    }

    pub fn asset_meta_from_ref(&self, reference: &Ref<dyn Asset>) -> Option<AssetMeta> {
        self.asset_meta_from_id(reference.id())
    }

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

    pub fn asset_type_from_ext(&self, ext: &str) -> Option<(TypeId, Uuid, &'static str)> {
        self.asset_data().extensions.get(ext).copied()
    }
}

impl AssetRegistry {
    pub fn build_meta(&self) -> Result<(), BoxedError> {
        for asset_path in &self.asset_paths {
            for path in glob(format!("{}/**/*", asset_path.to_str().unwrap()).as_str())
                .map_err(Box::new)?
                .map(|r| r.unwrap())
                .filter(|p| {
                    let ext = p.extension().map_or("", |ext| ext.to_str().unwrap_or(""));
                    !ext.is_empty() && ext != "meta" && ext != "rs"
                })
            {
                let meta_path = path.with_extension("meta");
                self.build_asset_meta(asset_path, &path, &meta_path)?;
            }
        }
        Ok(())
    }

    fn build_asset_meta(
        &self,
        asset_path: &Path,
        path: &Path,
        meta_path: &Path,
    ) -> Result<(), BoxedError> {
        let mut meta = if meta_path.exists() {
            self.load_meta_file(asset_path, meta_path).main
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
                    type_id: None,
                    name: relative_path.with_extension("").to_string(),
                    display_name,
                    parent: None,
                    children: Default::default(),
                    path: None,
                },
                inner: Default::default(),
            };
            self.write_meta_file(meta_path, &meta).map_err(Box::new)?;
            meta.main
        };
        meta.type_id = self.asset_type_from_ext(path.extension().unwrap().to_str().unwrap());
        meta.path = Some(match path.absolutize().map_err(Box::new)? {
            Cow::Borrowed(p) => p.to_path_buf(),
            Cow::Owned(p) => p,
        });
        let id = meta.id;
        let mut data = self.asset_data_mut();
        data.meta.insert(id, meta);
        data.names
            .insert(Self::relative_asset_path(asset_path, path), id);
        Ok(())
    }

    fn relative_asset_path(asset_path: &Path, path: &Path) -> RelativePathBuf {
        path.relative_to(asset_path)
            .unwrap()
            .normalize()
            .with_extension("")
    }

    fn load_meta_file(&self, asset_path: &Path, meta_path: &Path) -> AssetMetaData {
        let file = File::open(meta_path).unwrap();
        let reader = BufReader::new(file);
        let mut meta: AssetMetaData = serde_json::from_reader(reader).unwrap();
        let mut data = self.asset_data_mut();
        meta.main.children = meta.inner.iter().map(|m| m.id).collect();
        data.meta.insert(meta.main.id, meta.main.clone());
        data.names.insert(
            Self::relative_asset_path(asset_path, meta_path),
            meta.main.id,
        );
        for child in meta.inner.iter_mut() {
            child.parent = Some(meta.main.id);
            let mut path = meta_path.with_extension("");
            path.push(child.name.as_str());
            data.meta.insert(child.id, child.clone());
            data.names
                .insert(Self::relative_asset_path(asset_path, &path), child.id);
        }
        meta
    }

    fn write_meta_file(&self, meta_path: &Path, meta: &AssetMetaData) -> serde_json::Result<()> {
        let file = File::create(meta_path).unwrap();
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, meta)
    }
}

impl AssetRegistry {
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
            if (asset_type.is_none() || meta.type_id.map(|(_, uuid, _)| uuid) == asset_type)
                && meta
                    .display_name
                    .to_lowercase()
                    .contains(search_term.as_str())
            {
                list.push((*meta).clone());
            }
        }
    }

    pub fn reload_assets(&self) {
        let cache = self.asset_cache();
        let reload_ids = self.asset_data_mut().dirty.drain().collect::<Vec<_>>();
        for id in reload_ids {
            let Some(path) = self.asset_meta_from_id(id).and_then(|meta| meta.path) else {
                continue;
            };
            let Some(asset_ref) = cache.get(&id) else {
                continue;
            };
            let Some(meta) = self.asset_meta_from_id(id) else {
                continue;
            };

            let type_uuid = meta.type_id.map(|(_, uuid, _)| uuid).unwrap_or_default();
            let ctors = self.asset_constructors();
            if let Some(ctor) = ctors.get(&type_uuid) {
                let _ = (ctor.reload)(self.game_context(), asset_ref, &path);
            }
        }
    }
}

impl AssetRegistry {
    const SCREEN_SPACE_QUAD: &'static str = "screen_space_quad";
    const BLACK_TEXTURE_2D: &'static str = "black_texture_2d";
    const BLACK_TEXTURE_CUBE: &'static str = "black_texture_cube";
    const DEFAULT_SCENE: &'static str = "default_scene";

    pub fn missing_texture(&self) -> Option<Ref<Texture>> {
        self.load::<Texture>("textures/missing").ok()
    }

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

    pub fn cube(&self) -> Option<Ref<Mesh>> {
        self.load::<Mesh>("meshes/cube").ok()
    }

    pub fn sphere(&self) -> Option<Ref<Mesh>> {
        self.load::<Mesh>("meshes/sphere").ok()
    }

    pub fn cylinder(&self) -> Option<Ref<Mesh>> {
        self.load::<Mesh>("meshes/cylinder").ok()
    }

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

    pub fn new_empty_scene(&self) -> Scene {
        self.game_context().scene()
    }

    pub fn default_scene(&self) -> Option<Ref<Scene>> {
        self.load_or_create(Self::DEFAULT_SCENE, || {
            let mut scene = self.new_empty_scene();
            let game_object = scene.create_game_object(None, None);
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
