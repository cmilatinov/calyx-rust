use re_ui::Icon;

macro_rules! icon_from_path {
    ($path:literal) => {
        Icon::new($path, include_bytes!($path))
    };
}

pub const FOLDER: Icon = icon_from_path!("../../resources/icons/mdi--folder.png");
pub const GAME_OBJECT: Icon = icon_from_path!("../../resources/icons/heroicons--cube.png");
pub const OBJECT_TREE: Icon = icon_from_path!("../../resources/icons/mdi--file-tree.png");
pub const WALKING: Icon = icon_from_path!("../../resources/icons/healthicons--walking.png");
pub const GAMEPAD: Icon = icon_from_path!("../../resources/icons/mdi--gamepad-variant.png");
pub const VIEWPORT_3D: Icon = icon_from_path!("../../resources/icons/iconamoon--3d.png");
