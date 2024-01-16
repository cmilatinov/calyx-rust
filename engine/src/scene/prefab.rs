use crate as engine;
use crate::scene::{Scene, SceneData};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Read};
use std::path::{Path};
use engine_derive::TypeUuid;
use crate::assets::Asset;
use crate::assets::error::AssetError;

#[derive(Serialize, Deserialize, TypeUuid)]
#[uuid="960f1d60-3ad4-4f1d-92d3-cceb0e0623d7"]
#[serde(from = "PrefabShadow")]
pub struct Prefab {
    #[serde(skip_serializing, skip_deserializing)]
    pub scene: Scene,
    pub data: SceneData
}

#[derive(Deserialize)]
pub struct PrefabShadow {
    pub data: SceneData
}

impl From<PrefabShadow> for Prefab {
    fn from(value: PrefabShadow) -> Self {
        Self {
            data: value.data.clone(),
            scene: value.data.into(),
        }
    }
}

impl Asset for Prefab {
    fn get_file_extensions() -> &'static [&'static str] where
        Self: Sized,
    {
        &["cxprefab"]
    }

    fn from_file(path: &Path) -> Result<Self, AssetError> where Self: Sized {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|_| AssetError::LoadError)?;

        let reader = BufReader::new(file);
        serde_json::from_reader(reader).map_err(|_| AssetError::LoadError)
    }
}
