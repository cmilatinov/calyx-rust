use std::{fs, thread};
use std::any::TypeId;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc};
use std::thread::JoinHandle;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

use crate::assets::{Asset, AssetRef};
use crate::assets::error::AssetError;
use crate::core::Ref;
use utils::{Init, singleton};

pub struct AssetRegistry {
    asset_paths: Vec<String>,
    asset_cache: HashMap<String, Ref<dyn Asset>>,
    watcher_thread: Option<JoinHandle<()>>
}

singleton!(AssetRegistry);

impl AssetRegistry {
    pub fn root_path(&self) -> &str {
        self.asset_paths[0].as_str()
    }

    pub fn filename<A: Asset>(&self, id: &str, instance: &A) -> String {
        let extensions = instance.get_file_extensions();
        for path in self.asset_paths.iter() {
            for ext in extensions.iter() {
                let full_path = format!("{}/{}.{}", path, id, ext);
                let meta = fs::metadata(full_path.as_str());
                if let Ok(_) = meta {
                    return full_path;
                }
            }
        }
        String::from(id)
    }

    pub fn add_assets_path(&mut self, path: String) {
        self.asset_paths.push(path);
    }

    pub fn load<A: Asset + Default>(&mut self, id: &str) -> Result<Ref<A>, AssetError> {
        // Asset already loaded
        if let Some(asset) = self.asset_cache.get(id) {
            if asset.read().unwrap().deref().type_id() == TypeId::of::<A>() {
                return Ok(Ref::from_arc(unsafe {
                    Arc::from_raw(Arc::into_raw(asset.deref().clone()) as *const RwLock<A>)
                }));
            }
        };

        // Load from file
        let mut instance = A::default();
        let path = self.filename::<A>(id, &instance);
        instance.load(path.as_str())?;

        // Create ref
        let asset = Ref::new(instance);
        self.asset_cache.insert(String::from(id), asset.as_asset());
        Ok(asset)
    }

    pub fn create<A: Asset>(&mut self, id: &str, value: A) -> Result<Ref<A>, AssetError> {
        if self.asset_cache.contains_key(id) {
            return Err(AssetError::AssetAlreadyExists);
        }
        let asset = Ref::new(value);
        self.asset_cache.insert(String::from(id), asset.as_asset());
        Ok(asset)
    }

    pub fn assets(&self) -> &Vec<String> {
        &self.asset_paths
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self {
            asset_paths: Vec::new(),
            asset_cache: HashMap::new(),
            watcher_thread: None
        }
    }
}

impl Init for AssetRegistry {
    fn initialize(instance: &mut Self) {
        let watcher_thread = thread::spawn(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = RecommendedWatcher::new(tx, Config::default()).unwrap();
            watcher.watch(Path::new("assets"), RecursiveMode::Recursive).unwrap();
            for res in rx {
                match res {
                    Ok(event) => println!("FS Event: {:?}", event),
                    _ => {}
                }
            }
        });
        instance.asset_paths = vec!["assets".to_string()];
        instance.asset_cache = HashMap::new();
        instance.watcher_thread = Some(watcher_thread);
    }
}
