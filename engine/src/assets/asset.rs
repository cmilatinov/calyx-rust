use std::any::Any;
use crate::assets::error::AssetError;
use crate::core::refs::Ref;

pub trait Asset: Any + Send + Sync {
    fn get_extensions(&self) -> &'static [&'static str] {
        &[""]
    }

    fn load(&mut self, path: &str) -> Result<(), AssetError>;
}