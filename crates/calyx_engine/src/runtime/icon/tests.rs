use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{calyx_icon, decode_icon, player_icon, CALYX_ICON_PNG, CALYX_ICON_SIZE};
use crate::runtime::ProjectRuntimeSettings;

fn temp_directory() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("calyx-window-icon-{unique}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn settings_with_icon(root: &PathBuf, icon: Option<&str>) -> ProjectRuntimeSettings {
    let mut manifest = String::from("name = \"Icon project\"\n");
    if let Some(icon) = icon {
        manifest.push_str(&format!("[runtime.window]\nicon = {icon:?}\n"));
    }
    ProjectRuntimeSettings::from_manifest(root, &manifest).unwrap()
}

#[test]
fn calyx_icon_decodes_to_rgba_at_its_size() {
    let icon = decode_icon(CALYX_ICON_PNG).unwrap();

    assert_eq!(
        (icon.width, icon.height),
        (CALYX_ICON_SIZE, CALYX_ICON_SIZE)
    );
    assert_eq!(
        icon.rgba.len(),
        (CALYX_ICON_SIZE * CALYX_ICON_SIZE * 4) as usize
    );
    let corner_alpha = icon.rgba[3];
    let centre = ((CALYX_ICON_SIZE / 2 * CALYX_ICON_SIZE + CALYX_ICON_SIZE / 2) * 4) as usize;
    assert_eq!(corner_alpha, 0, "the rounded tile leaves the corners clear");
    assert_eq!(icon.rgba[centre + 3], 255, "the tile is opaque");
}

#[test]
fn calyx_ico_holds_every_windows_size() {
    let ico = include_bytes!("../../../../../resources/app_icon/calyx.ico");
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let sizes: Vec<u32> = (0..count)
        .map(|index| {
            let width = ico[6 + index * 16];
            if width == 0 {
                256
            } else {
                width as u32
            }
        })
        .collect();

    for size in [16, 24, 32, 48, 64, 256] {
        assert!(sizes.contains(&size), "{size} px missing from {sizes:?}");
    }
}

#[test]
fn player_uses_the_calyx_icon_when_the_project_sets_none() {
    let settings = settings_with_icon(&PathBuf::from("root"), None);

    assert_eq!(settings.window().icon, None);
    assert_eq!(player_icon(&settings), calyx_icon());
}

#[test]
fn player_uses_the_calyx_icon_when_the_project_icon_is_missing() {
    let root = temp_directory();
    let settings = settings_with_icon(&root, Some("assets/missing.png"));

    assert_eq!(player_icon(&settings), calyx_icon());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn player_uses_the_calyx_icon_when_the_project_icon_is_not_an_image() {
    let root = temp_directory();
    fs::write(root.join("icon.png"), b"not an image").unwrap();
    let settings = settings_with_icon(&root, Some("icon.png"));

    assert_eq!(player_icon(&settings), calyx_icon());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn player_uses_the_project_icon_it_names() {
    let root = temp_directory();
    let mut image = image::RgbaImage::new(32, 16);
    image.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
    image.save(root.join("game_icon.png")).unwrap();
    let settings = settings_with_icon(&root, Some("game_icon.png"));

    let icon = player_icon(&settings);

    assert_eq!((icon.width, icon.height), (32, 16));
    assert_eq!(&icon.rgba[..4], &[10, 20, 30, 255]);
    fs::remove_dir_all(root).unwrap();
}
