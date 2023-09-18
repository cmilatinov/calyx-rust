use std::any::TypeId;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::{thread};

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use notify::event::CreateKind;

use crate::assets::error::AssetError;
use crate::assets::{Asset, AssetRef};
use crate::core::Ref;
use utils::{singleton_with_init};

#[derive(Default)]
pub struct AssetRegistry {
    asset_paths: Vec<PathBuf>,
    asset_cache: HashMap<PathBuf, Ref<dyn Asset>>,
    watcher_thread: Option<JoinHandle<()>>,
}

singleton_with_init!(AssetRegistry);

impl AssetRegistry {
    pub fn root_path(&self) -> &PathBuf {
        &self.asset_paths[0]
    }

    pub fn filename<A: Asset>(&self, id: impl Into<PathBuf> + Clone, instance: &A) -> PathBuf {
        let id: PathBuf = id.into();
        let extensions = instance.get_file_extensions();
        for path in self.asset_paths.iter() {
            for ext in extensions.iter() {
                let mut new_path = path.clone();
                new_path.push(id.clone());
                new_path.set_extension(ext);
                if new_path.exists() { return new_path }
            }
        }
        id
    }

    pub fn add_assets_path(&mut self, path: PathBuf) {
        self.asset_paths.push(path);
    }

    pub fn load<A: Asset + Default>(&mut self, id: impl Into<PathBuf> + Clone) -> Result<Ref<A>, AssetError> {
        let id: PathBuf = id.into();
        // Asset already loaded
        if let Some(asset) = self.asset_cache.get(&id) {
            if asset.read().unwrap().deref().type_id() == TypeId::of::<A>() {
                return Ok(Ref::from_arc(unsafe {
                    Arc::from_raw(Arc::into_raw(asset.deref().clone()) as *const RwLock<A>)
                }));
            }
        };

        // Load from file
        let mut instance = A::default();
        let path = self.filename::<A>(&id, &instance);
        instance.load(path)?;

        // Create ref
        let asset = Ref::new(instance);
        self.asset_cache.insert(id, asset.as_asset());
        Ok(asset)
    }

    pub fn create<A: Asset>(&mut self, id: impl Into<PathBuf>, value: A) -> Result<Ref<A>, AssetError> {
        let id: PathBuf = id.into();

        if self.asset_cache.contains_key(&id) {
            return Err(AssetError::AssetAlreadyExists);
        }
        let asset = Ref::new(value);
        self.asset_cache.insert(id, asset.as_asset());
        Ok(asset)
    }

    pub fn assets(&self) -> &Vec<PathBuf> {
        &self.asset_paths
    }
}

impl AssetRegistry {
    pub fn set_root(&mut self, path: PathBuf) {
        self.asset_paths = vec![path.clone(), "assets".into()];

        let watcher_thread = thread::spawn(move|| {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = RecommendedWatcher::new(tx, Config::default()).unwrap();
            watcher
                .watch(path.as_path(), RecursiveMode::Recursive)
                .unwrap();
            for event in rx.into_iter().flatten() {
                match event.kind {
                    EventKind::Create(CreateKind::File) |
                    EventKind::Modify(_)                |
                    EventKind::Remove(_) => {
                        println!("Rebuilding");
                    }
                    _ => {}
                }
            }
        });
        self.asset_cache = HashMap::new();
        self.watcher_thread = Some(watcher_thread);
    }
}
