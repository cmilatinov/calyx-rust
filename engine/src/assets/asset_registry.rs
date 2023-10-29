use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::thread;
use std::thread::JoinHandle;

use glob::glob;
use notify::event::CreateKind;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use path_absolutize::Absolutize;
use relative_path::{PathExt, RelativePathBuf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::assets::error::AssetError;
use crate::assets::{Asset, AssetRef};
use crate::core::Ref;
use crate::utils::singleton_with_init;

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetMeta {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub path: Option<PathBuf>,
    dirty: bool,
}

#[derive(Default)]
pub struct AssetRegistry {
    asset_paths: Vec<PathBuf>,
    asset_cache: HashMap<Uuid, Ref<dyn Asset>>,
    asset_meta: HashMap<Uuid, AssetMeta>,
    asset_names: HashMap<RelativePathBuf, Uuid>,
    watcher_thread: Option<JoinHandle<()>>,
}

singleton_with_init!(AssetRegistry);

impl AssetRegistry {
    pub fn root_path(&self) -> &PathBuf {
        &self.asset_paths[0]
    }

    pub fn load<A: Asset>(&mut self, name: &str) -> Result<Ref<A>, AssetError> {
        let id = self.asset_id(name).ok_or(AssetError::NotFound)?;

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

    pub fn create<A: Asset>(&mut self, name: &str, value: A) -> Result<Ref<A>, AssetError> {
        if self.asset_id(name).is_some() {
            return Err(AssetError::AssetAlreadyExists);
        }
        let id = Uuid::new_v4();
        let asset = Ref::new(value);
        self.asset_names
            .insert(RelativePathBuf::from(name).normalize(), id);
        self.asset_cache.insert(id, asset.as_asset());
        self.asset_meta.insert(
            id,
            AssetMeta {
                id,
                name: name.to_string(),
                display_name: name.to_string(),
                path: None,
                dirty: false,
            },
        );
        Ok(asset)
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
                    | EventKind::Remove(_) => {
                        println!("Rebuilding");
                    }
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
                        name: name.to_string(),
                        display_name: name.to_string(),
                        path: None,
                        dirty: false,
                    };
                    let file = File::create(meta_path.as_path()).unwrap();
                    let writer = BufWriter::new(file);
                    serde_json::to_writer(writer, &meta).unwrap();
                    meta
                };
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
