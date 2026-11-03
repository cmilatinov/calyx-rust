//! Per-pack settings: everything about importing one Asset Store pack that
//! the general importer cannot know.

pub mod rpg_poly;

use uuid::Uuid;

use crate::package::UnityPackage;
use crate::scene::{Converted, SceneWriter};

/// Bakes a pack's fog into a texture: the fog `[r, g, b, fraction]` for
/// the texture with the given GUID, if any.
pub type TextureFog = fn(&UnityPackage, &str) -> Result<Option<[f64; 4]>, String>;

/// One Asset Store pack the importer knows.
pub struct Pack {
    /// The name `--pack` selects it by.
    pub id: &'static str,
    /// The pack's store title.
    pub title: &'static str,
    /// Where the Unity editor's Package Manager saves the pack, under
    /// `%APPDATA%\Unity\Asset Store-5.x`.
    pub asset_store_path: &'static [&'static str],
    /// The output folder, relative to the repository root.
    pub root: &'static str,
    /// The asset-name prefix of everything generated, its folder under the
    /// project's assets.
    pub asset_prefix: &'static str,
    /// Asset and object ids derive from this namespace.
    pub namespace: Uuid,
    /// The Unity scene converted to the demo scene.
    pub demo_scene: &'static str,
    /// The generated scene's file name, without extension.
    pub scene_name: &'static str,
    /// The material GUID a model file's own renderer uses.
    pub default_material: &'static str,
    /// The fog `[r, g, b, fraction]` to bake into a texture, by its GUID.
    pub texture_fog: TextureFog,
    /// Adds the pack's lighting, post-processing and camera to the converted
    /// scene, and returns extra text for the summary.
    pub finish_scene: fn(&mut SceneWriter, &Converted) -> Result<String, String>,
}

/// Every pack the importer knows.
pub const PACKS: &[&Pack] = &[&rpg_poly::PACK];

/// The pack `--pack` names.
pub fn find(id: &str) -> Option<&'static Pack> {
    PACKS.iter().copied().find(|pack| pack.id == id)
}
