use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::thread::JoinHandle;

use glob::glob;
use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use path_absolutize::Absolutize;
use relative_path::{PathExt, RelativePathBuf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::assets::error::AssetError;
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture2D;
use crate::assets::LoadedAssetRef;
use crate::assets::{Asset, AssetRef};
use crate::core::Ref;
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{AttributeValue, TypeInfo};
use crate::render::Shader;
use crate::scene::Prefab;
use crate::utils;
use crate::utils::{singleton, Init, TypeUuid};

type AssetConstructor =
    Box<dyn Fn(&Path) -> Result<LoadedAssetRef<dyn Asset>, AssetError> + Send + Sync>;
type AssetCache = HashMap<Uuid, Ref<dyn Asset>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub parent: Option<Uuid>,
    #[serde(skip_serializing, skip_deserializing)]
    pub children: Vec<Uuid>,
    #[serde(skip_serializing, skip_deserializing)]
    pub type_uuid: Option<Uuid>,
    #[serde(skip_serializing, skip_deserializing)]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing, skip_deserializing)]
    dirty: bool,
}

#[derive(Serialize, Deserialize)]
pub struct AssetMetaData {
    main: AssetMeta,
    sub: Vec<AssetMeta>,
}

#[derive(Default)]
struct AssetData {
    meta: HashMap<Uuid, AssetMeta>,
    names: HashMap<RelativePathBuf, Uuid>,
    extensions: HashMap<String, Uuid>,
}

#[derive(Default)]
pub struct AssetRegistry {
    asset_paths: Vec<PathBuf>,
    asset_cache: RwLock<AssetCache>,
    asset_data: RwLock<AssetData>,
    asset_constructors: RwLock<HashMap<Uuid, AssetConstructor>>,
    watcher_thread: Option<JoinHandle<()>>,
}

singleton!(AssetRegistry);

impl Init for AssetRegistry {
    fn initialize(&mut self) {
        self.register_asset_type::<Mesh>();
        self.register_asset_type::<Shader>();
        self.register_asset_type::<Texture2D>();
        self.register_asset_type::<Material>();
        self.register_asset_type::<Prefab>();
    }
}

impl AssetRegistry {
    pub fn root_path(&self) -> &PathBuf {
        &self.asset_paths[0]
    }

    pub fn load<A: Asset + TypeUuid>(&self, name: &str) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id(name).ok_or(AssetError::NotFound)?;
        self.load_by_id(id)
    }

    pub fn load_by_path<A: Asset + TypeUuid>(&self, path: PathBuf) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id_from_path(&path).ok_or(AssetError::NotFound)?;
        self.load_by_id(id)
    }

    pub fn load_by_id<A: Asset + TypeUuid>(&self, id: Uuid) -> Result<Ref<A>, AssetError> {
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
            .asset_path(id, A::get_file_extensions())
            .ok_or(AssetError::NotFound)?;
        let asset = self.load_asset_file(id, &path)?;

        // Create ref
        self.asset_cache_mut().insert(id, asset.as_asset());
        Ok(asset)
    }

    pub fn load_dyn_by_id(&self, id: Uuid) -> Result<Ref<dyn Asset>, AssetError> {
        // Asset already loaded
        if let Some(asset_ref) = self.asset_cache().get(&id) {
            return Ok(Ref::from(asset_ref));
        }

        // Find constructor & file path
        let meta = self.asset_meta_from_id(id).ok_or(AssetError::NotFound)?;
        let type_uuid = meta.type_uuid.as_ref().ok_or(AssetError::NotFound)?;
        let path = meta.path.as_ref().ok_or(AssetError::NotFound)?;
        let ctors = self.asset_constructors();
        let ctor = ctors.get(type_uuid).ok_or(AssetError::NotFound)?;
        let LoadedAssetRef { asset, sub_assets } = ctor(path.as_path())?;

        // Load from file
        self.load_sub_asset_meta(id, sub_assets);
        self.asset_cache_mut().insert(id, Ref::from(&asset));
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
        let asset = Ref::new(value);
        let registry = TypeRegistry::get();
        let display_name = registry.type_info::<A>().and_then(|info| {
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
                type_uuid: Some(A::type_uuid()),
                display_name: display_name.unwrap_or(name.clone()),
                name,
                parent: None,
                children: Default::default(),
                path: None,
                dirty: false,
            },
        );
        self.asset_cache_mut().insert(id, asset.as_asset());
        Ok(asset)
    }

    pub fn register_asset_type<A: Asset + TypeUuid>(&self) {
        let type_uuid = A::type_uuid();
        let mut data = self.asset_data_mut();
        for ext in A::get_file_extensions() {
            data.extensions.insert(String::from(*ext), type_uuid);
        }
        self.asset_constructors_mut().insert(
            type_uuid,
            Box::new(|path| {
                let LoadedAssetRef { asset, sub_assets } = A::from_file(path)?.into();
                Ok(LoadedAssetRef {
                    asset: asset.as_asset(),
                    sub_assets,
                })
            }),
        );
    }

    fn load_sub_asset_meta(&self, id: Uuid, sub_assets: Vec<Uuid>) {
        let mut data = self.asset_data_mut();
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
        let LoadedAssetRef { asset, sub_assets } = A::from_file(path)?.into();
        self.load_sub_asset_meta(id, sub_assets);
        Ok(asset)
    }
}

impl AssetRegistry {
    fn asset_data(&self) -> RwLockReadGuard<AssetData> {
        self.asset_data.read().unwrap()
    }

    fn asset_data_mut(&self) -> RwLockWriteGuard<AssetData> {
        self.asset_data.write().unwrap()
    }

    pub fn set_root_path(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.asset_paths = vec![path.clone(), "assets".into()];

        let watcher_thread = thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = RecommendedWatcher::new(tx, Config::default()).unwrap();
            watcher
                .watch(path.as_path(), RecursiveMode::Recursive)
                .unwrap();
            for event in rx.into_iter().flatten() {
                match event.kind {
                    EventKind::Create(CreateKind::File) => {
                        for file in &event.paths {
                            AssetRegistry::get_mut().build_asset_meta(
                                path.as_path(),
                                file.as_path(),
                                file.with_extension("meta").as_path(),
                            )
                        }
                    }
                    EventKind::Modify(ModifyKind::Data(_)) => {}
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                        println!("{:?}", event.paths);
                    }
                    EventKind::Remove(RemoveKind::File) => {
                        for file in &event.paths {
                            if let Some(ext) = file.extension().and_then(|ext| ext.to_str()) {
                                if ext != "meta" {
                                    let _ = std::fs::remove_file(file.with_extension("meta"));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        self.asset_cache = RwLock::new(HashMap::new());
        self.watcher_thread = Some(watcher_thread);
        self.build_meta();
    }
}

impl AssetRegistry {
    fn asset_cache(&self) -> RwLockReadGuard<AssetCache> {
        self.asset_cache.read().unwrap()
    }

    fn asset_cache_mut(&self) -> RwLockWriteGuard<AssetCache> {
        self.asset_cache.write().unwrap()
    }

    fn asset_constructors(&self) -> RwLockReadGuard<HashMap<Uuid, AssetConstructor>> {
        self.asset_constructors.read().unwrap()
    }

    fn asset_constructors_mut(&self) -> RwLockWriteGuard<HashMap<Uuid, AssetConstructor>> {
        self.asset_constructors.write().unwrap()
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

    pub fn asset_id_from_path(&self, path: &Path) -> Option<Uuid> {
        path.relative_to(self.root_path()).ok().and_then(|p| {
            let p = p.with_extension("");
            self.asset_data().names.get(&p).copied()
        })
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
        self.asset_id_from_ref(reference)
            .and_then(|id| self.asset_meta_from_id(id))
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

    pub fn asset_type_from_ext(&self, ext: &str) -> Option<Uuid> {
        self.asset_data().extensions.get(ext).copied()
    }

    pub fn asset_id_from_ref(&self, reference: &Ref<dyn Asset>) -> Option<Uuid> {
        for (id, asset_ref) in self.asset_cache().iter() {
            if Arc::ptr_eq(asset_ref, reference) {
                return Some(*id);
            }
        }
        None
    }

    pub fn asset_id_from_ref_t<A: Asset + TypeUuid>(&self, reference: &Ref<A>) -> Option<Uuid> {
        self.asset_id_from_ref(&reference.as_asset())
    }
}

impl AssetRegistry {
    pub fn build_meta(&self) {
        for asset_path in &self.asset_paths {
            for path in glob(format!("{}/**/*", asset_path.to_str().unwrap()).as_str())
                .unwrap()
                .map(|r| r.unwrap())
                .filter(|p| {
                    let ext = p.extension().map_or("", |ext| ext.to_str().unwrap_or(""));
                    !ext.is_empty() && ext != "meta" && ext != "rs"
                })
            {
                let meta_path = path.with_extension("meta");
                self.build_asset_meta(asset_path.as_path(), path.as_path(), meta_path.as_path());
            }
        }
    }

    fn build_asset_meta(&self, asset_path: &Path, path: &Path, meta_path: &Path) {
        let mut meta = if meta_path.exists() {
            let file = File::open(meta_path).unwrap();
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap()
        } else {
            let display_name = path
                .file_stem()
                .and_then(|f| f.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let relative_path = path.relative_to(asset_path).unwrap();
            let meta = AssetMeta {
                id: utils::uuid_from_str(relative_path.as_str()),
                type_uuid: None,
                name: relative_path.with_extension("").to_string(),
                display_name,
                parent: None,
                children: Default::default(),
                path: None,
                dirty: false,
            };
            let file = File::create(meta_path).unwrap();
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, &meta).unwrap();
            meta
        };
        meta.type_uuid = self.asset_type_from_ext(path.extension().unwrap().to_str().unwrap());
        meta.path = Some(match path.absolutize().unwrap() {
            Cow::Borrowed(p) => p.to_path_buf(),
            Cow::Owned(p) => p,
        });
        let id = meta.id;
        let mut data = self.asset_data_mut();
        data.meta.insert(id, meta);
        let relative_path = path.relative_to(asset_path).unwrap().with_extension("");
        data.names.insert(relative_path, id);
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
            if (asset_type.is_none() || meta.type_uuid == asset_type)
                && meta
                    .display_name
                    .to_lowercase()
                    .contains(search_term.as_str())
            {
                list.push((*meta).clone());
            }
        }
    }
}
