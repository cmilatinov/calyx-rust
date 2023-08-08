use assets_manager::{AssetCache, Compound, Error, Handle};
use assets_manager::source::FileSystem;
use crate::{singleton};
use crate::utils::Init;

pub struct AssetRegistry {
    asset_cache: AssetCache<FileSystem>
}

singleton!(AssetRegistry);

impl AssetRegistry {
    pub fn load<A: Compound>(&self, id: &str) -> Result<Handle<A>, Error> {
        self.asset_cache.load::<A>(id)
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self {
            asset_cache: AssetCache::new("assets").unwrap()
        }
    }
}

impl Init<AssetRegistry> for AssetRegistry {}