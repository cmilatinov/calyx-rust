use egui::{pos2, Color32, Image, ImageSource, Rect, Ui};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

const ATLAS_URI: &str = "bytes://calyx/phosphor_regular.png";
const ATLAS_BYTES: &[u8] = include_bytes!("../../resources/icons/phosphor_regular.png");
const ATLAS_MANIFEST: &str = include_str!("../../resources/icons/phosphor_regular.json");

#[derive(Clone, Copy, Debug)]
pub struct AtlasIcon {
    name: &'static str,
}

#[derive(Debug, Deserialize)]
struct AtlasManifest {
    icons: HashMap<String, IconRect>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct IconRect {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
}

static MANIFEST: LazyLock<AtlasManifest> = LazyLock::new(|| {
    serde_json::from_str(ATLAS_MANIFEST).expect("embedded icon atlas manifest is valid")
});

impl AtlasIcon {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub fn as_image(&self) -> Image<'static> {
        Image::new(ImageSource::Bytes {
            uri: ATLAS_URI.into(),
            bytes: ATLAS_BYTES.into(),
        })
        .uv(self.uv())
        .fit_to_original_size(0.5)
    }

    pub fn paint_at(&self, ui: &mut Ui, rect: Rect, tint: Color32) {
        self.as_image()
            .fit_to_exact_size(rect.size())
            .tint(tint)
            .paint_at(ui, rect);
    }

    fn uv(&self) -> Rect {
        let icon = MANIFEST
            .icons
            .get(self.name)
            .unwrap_or_else(|| panic!("icon '{}' is missing from the embedded atlas", self.name));
        Rect::from_min_max(pos2(icon.u0, icon.v0), pos2(icon.u1, icon.v1))
    }
}

pub const GAME_OBJECT: AtlasIcon = AtlasIcon::new("cube-fill");
pub const OBJECT_TREE: AtlasIcon = AtlasIcon::new("tree-structure-fill");
pub const WALKING: AtlasIcon = AtlasIcon::new("person-simple-walk-fill");
pub const GAMEPAD: AtlasIcon = AtlasIcon::new("game-controller-fill");
pub const VIEWPORT_3D: AtlasIcon = AtlasIcon::new("cube-focus-fill");
