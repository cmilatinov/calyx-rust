use std::any::TypeId;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use glob::glob;
use notify::event::CreateKind;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use path_absolutize::Absolutize;
use relative_path::{PathExt, RelativePathBuf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use reflect::{AttributeValue, TypeInfo, TypeUuid};

use crate::assets::error::AssetError;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture2D;
use crate::assets::{Asset, AssetRef};
use crate::core::Ref;
use crate::render::Shader;
use crate::type_registry::TypeRegistry;
use crate::utils::{singleton, Init};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    pub id: Uuid,
    #[serde(skip_serializing, skip_deserializing)]
    pub type_id: Option<TypeId>,
    pub name: String,
    pub display_name: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing, skip_deserializing)]
    dirty: bool,
}

#[derive(Default)]
pub struct AssetRegistry {
    asset_paths: Vec<PathBuf>,
    asset_cache: HashMap<Uuid, Ref<dyn Asset>>,
    asset_meta: HashMap<Uuid, AssetMeta>,
    asset_names: HashMap<RelativePathBuf, Uuid>,
    asset_extensions: HashMap<String, TypeId>,
    asset_ctors:
        HashMap<TypeId, Box<dyn Fn(&Path) -> Result<Ref<dyn Asset>, AssetError> + Send + Sync>>,
    watcher_thread: Option<JoinHandle<()>>,
}

singleton!(AssetRegistry);

impl Init for AssetRegistry {
    fn initialize(&mut self) {
        self.register_asset_type::<Mesh>();
        self.register_asset_type::<Shader>();
        self.register_asset_type::<Texture2D>();
    }
}

impl AssetRegistry {
    pub fn root_path(&self) -> &PathBuf {
        &self.asset_paths[0]
    }

    pub fn load<A: Asset>(&mut self, name: &str) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id(name).ok_or(AssetError::NotFound)?;
        self.load_by_id(id)
    }

    pub fn load_by_id<A: Asset>(&mut self, id: Uuid) -> Result<Ref<A>, AssetError> {
        // Asset already loaded
        if let Some(asset_ref) = self
            .asset_cache
            .get(&id)
            .and_then(|a| a.try_downcast::<A>())
        {
            return Ok(asset_ref);
        }

        // Load from file
        let path = self
            .asset_path(id, A::get_file_extensions())
            .ok_or(AssetError::NotFound)?;
        let instance = A::from_file(path)?;

        // Create ref
        let asset = Ref::new(instance);
        self.asset_cache.insert(id, asset.as_asset());
        Ok(asset)
    }

    pub fn load_dyn_by_id(&mut self, id: Uuid) -> Result<Ref<dyn Asset>, AssetError> {
        // Asset already loaded
        if let Some(asset_ref) = self.asset_cache.get(&id) {
            return Ok(Ref::from_arc((*asset_ref).clone()));
        }

        // Find constructor & file path
        let meta = self.asset_meta_from_id(id).ok_or(AssetError::NotFound)?;
        let type_id = meta.type_id.as_ref().ok_or(AssetError::NotFound)?;
        let ctor = self.asset_ctors.get(type_id).ok_or(AssetError::NotFound)?;
        let path = meta.path.as_ref().ok_or(AssetError::NotFound)?;

        // Load from file
        let asset = ctor(path.as_path())?;
        self.asset_cache.insert(id, Ref::from(&asset));
        Ok(asset)
    }

    pub fn create<A: Asset + TypeUuid>(
        &mut self,
        name: &str,
        value: A,
    ) -> Result<Ref<A>, AssetError> {
        if self.asset_id(name).is_some() {
            return Err(AssetError::AlreadyExists);
        }
        let id = Uuid::new_v4();
        let asset = Ref::new(value);
        let registry = TypeRegistry::get();
        let display_name = registry.type_info_by_uuid(A::type_uuid()).and_then(|info| {
            if let TypeInfo::Struct(info) = info {
                if let Some(AttributeValue::String(str)) = info.attr("name") {
                    return Some(str.to_string());
                }
            }
            None
        });
        self.asset_names
            .insert(RelativePathBuf::from(name).normalize(), id);
        self.asset_cache.insert(id, asset.as_asset());
        self.asset_meta.insert(
            id,
            AssetMeta {
                id,
                type_id: Some(TypeId::of::<A>()),
                name: name.to_string(),
                display_name: display_name.unwrap_or(name.to_string()),
                path: None,
                dirty: false,
            },
        );
        Ok(asset)
    }

    pub fn register_asset_type<A: Asset>(&mut self) {
        let type_id = TypeId::of::<A>();
        for ext in A::get_file_extensions() {
            self.asset_extensions.insert(String::from(*ext), type_id);
        }
        self.asset_ctors.insert(
            type_id,
            Box::new(|path| {
                let asset = A::from_file(path)?;
                Ok(Ref::new(asset).as_asset())
            }),
        );
    }
}

impl AssetRegistry {
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
                    EventKind::Create(CreateKind::File)
                    | EventKind::Modify(_)
                    | EventKind::Remove(_) => {}
                    _ => {}
                }
            }
        });
        self.asset_cache = HashMap::new();
        self.watcher_thread = Some(watcher_thread);
        self.build_asset_meta();
    }
}

impl AssetRegistry {
    pub fn asset_id(&self, name: &str) -> Option<Uuid> {
        let path = RelativePathBuf::from(name).normalize();
        self.asset_names.get(&path).copied()
    }

    pub fn asset_meta(&self, name: &str) -> Option<&AssetMeta> {
        let id = self.asset_id(name)?;
        self.asset_meta.get(&id)
    }

    pub fn asset_meta_from_id(&self, id: Uuid) -> Option<&AssetMeta> {
        self.asset_meta.get(&id)
    }

    pub fn asset_path(&self, id: Uuid, extensions: &[&str]) -> Option<&Path> {
        let meta = self.asset_meta.get(&id)?;
        let path = meta.path.as_ref()?;
        let ext = path
            .extension()
            .map_or("", |ext| ext.to_str().unwrap_or(""));
        if extensions.contains(&ext) {
            Some(path)
        } else {
            None
        }
    }

    pub fn asset_type_from_ext(&self, ext: &str) -> Option<TypeId> {
        self.asset_extensions.get(ext).copied()
    }

    pub fn asset_id_from_ref(&self, reference: &Ref<dyn Asset>) -> Option<Uuid> {
        for (id, asset_ref) in self.asset_cache.iter() {
            if Arc::ptr_eq(asset_ref, reference) {
                return Some(*id);
            }
        }
        None
    }
}

impl AssetRegistry {
    pub fn build_asset_meta(&mut self) {
        for asset_path in self.asset_paths.iter() {
            for path in glob(format!("{}/**/*", asset_path.to_str().unwrap()).as_str())
                .unwrap()
                .map(|r| r.unwrap())
                .filter(|p| {
                    let ext = p.extension().map_or("", |ext| ext.to_str().unwrap_or(""));
                    !ext.is_empty() && ext != "meta" && ext != "rs"
                })
            {
                let meta_path = path.with_extension("meta");
                let mut meta = if meta_path.exists() {
                    let file = File::open(meta_path.as_path()).unwrap();
                    let reader = BufReader::new(file);
                    serde_json::from_reader(reader).unwrap()
                } else {
                    let name = path.file_stem().unwrap().to_str().unwrap();
                    let meta = AssetMeta {
                        id: Uuid::new_v4(),
                        type_id: None,
                        name: name.to_string(),
                        display_name: name.to_string(),
                        path: None,
                        dirty: false,
                    };
                    let file = File::create(meta_path.as_path()).unwrap();
                    let writer = BufWriter::new(file);
                    serde_json::to_writer_pretty(writer, &meta).unwrap();
                    meta
                };
                meta.type_id =
                    self.asset_type_from_ext(path.extension().unwrap().to_str().unwrap());
                meta.path = Some(match path.absolutize().unwrap() {
                    Cow::Borrowed(p) => p.to_path_buf(),
                    Cow::Owned(p) => p,
                });
                let id = meta.id;
                self.asset_meta.insert(id, meta);
                let relative_path = path.relative_to(asset_path).unwrap().with_extension("");
                self.asset_names.insert(relative_path, id);
            }
        }
    }
}

impl AssetRegistry {
    pub fn search_assets(
        &self,
        search_term: &str,
        asset_type: Option<TypeId>,
        list: &mut Vec<AssetMeta>,
    ) {
        list.clear();
        let search_term = search_term.to_lowercase();
        // TODO: Case insensitive and word by word filter
        for (_, meta) in self.asset_meta.iter() {
            if (asset_type.is_none() || meta.type_id == asset_type)
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
