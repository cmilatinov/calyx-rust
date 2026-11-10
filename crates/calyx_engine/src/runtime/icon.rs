//! Window icons: the Calyx icon the editor and player show, and the player's
//! choice between a project's own icon and that one.

use std::sync::Arc;

use egui::IconData;
use winit::raw_window_handle::HasWindowHandle;

use crate::error::BoxedError;
use crate::runtime::ProjectRuntimeSettings;

/// The Calyx icon as a 256 px PNG, rendered from `resources/app_icon/calyx.svg`
/// by `resources/app_icon/generate.py`.
pub const CALYX_ICON_PNG: &[u8] = include_bytes!("../../../../resources/app_icon/calyx.png");

/// Width and height of [`CALYX_ICON_PNG`] in pixels.
pub const CALYX_ICON_SIZE: u32 = 256;

/// Decodes an image file's bytes, in any format the `image` crate reads, to
/// the RGBA pixels a window icon takes.
pub fn decode_icon(bytes: &[u8]) -> Result<IconData, BoxedError> {
    let image = image::load_from_memory(bytes)?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok(IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Returns the Calyx icon, decoded.
pub fn calyx_icon() -> Arc<IconData> {
    Arc::new(decode_icon(CALYX_ICON_PNG).expect("the embedded Calyx icon is a valid PNG"))
}

/// Returns the icon the player's window shows: the project's
/// `[runtime.window] icon`, resolved against the project root, or the Calyx
/// icon when the project sets none or its file cannot be read or decoded.
/// A project icon that fails is logged as a warning.
pub fn player_icon(settings: &ProjectRuntimeSettings) -> Arc<IconData> {
    let Some(icon) = &settings.window().icon else {
        return calyx_icon();
    };
    let path = settings.project_path(icon);
    let loaded = std::fs::read(&path)
        .map_err(BoxedError::from)
        .and_then(|bytes| decode_icon(&bytes));
    match loaded {
        Ok(icon) => Arc::new(icon),
        Err(error) => {
            log::warn!(
                "Failed to load the project's window icon '{}': {error}; using the Calyx icon",
                path.display()
            );
            calyx_icon()
        }
    }
}

/// Sets the window's taskbar and title bar icons to `icon` at once, on
/// Windows; elsewhere the viewport's icon is all there is, and this does
/// nothing.
///
/// eframe sets the taskbar-size icon (`ICON_BIG`) only once the window is the
/// active one, after its taskbar button exists, and the taskbar keeps the
/// generic icon it took then: a window that opens without focus never shows
/// its icon there. Setting the window's own icons before it shows was not
/// enough for the player either, so the window class carries them too, which
/// the taskbar reads without asking the window. Call this from the app's
/// creation, before eframe first shows the window.
pub fn set_window_icons(window: &impl HasWindowHandle, icon: &IconData) {
    #[cfg(windows)]
    windows_icons::set(window, icon);
    #[cfg(not(windows))]
    let _ = (window, icon);
}

#[cfg(windows)]
mod windows_icons {
    use egui::IconData;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIcon, GetSystemMetrics, SendMessageW, SetClassLongPtrW, GCLP_HICON, GCLP_HICONSM,
        HICON, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON, WM_SETICON,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub(super) fn set(window: &impl HasWindowHandle, icon: &IconData) {
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get();
        // The big icon first: Windows scales it for the small one until that
        // is set.
        for (kind, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
            // SAFETY: a getter without side effects.
            let size = unsafe { GetSystemMetrics(metric) }.max(16) as u32;
            let Some(hicon) = create_icon(icon, size) else {
                continue;
            };
            let class_icon = if kind == ICON_BIG {
                GCLP_HICON
            } else {
                GCLP_HICONSM
            };
            // SAFETY: `hwnd` is the live window the creation context names,
            // and the window and its class take the icon handle.
            unsafe {
                SendMessageW(hwnd, WM_SETICON, kind as usize, hicon);
                SetClassLongPtrW(hwnd, class_icon, hicon);
            }
        }
    }

    /// Creates a `size` px icon from `icon`, scaled with a Lanczos filter.
    fn create_icon(icon: &IconData, size: u32) -> Option<HICON> {
        let image = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba.clone())?;
        let scaled =
            image::imageops::resize(&image, size, size, image::imageops::FilterType::Lanczos3);
        let mut bgra = scaled.into_raw();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        // The colour image carries the alpha, so the mask hides nothing.
        let and_mask = vec![0u8; (size * size) as usize];
        // SAFETY: both buffers hold at least the rows CreateIcon reads for a
        // `size` x `size` icon at 32 bits per pixel and 1 bit per mask pixel.
        let hicon = unsafe {
            CreateIcon(
                0,
                size as i32,
                size as i32,
                1,
                32,
                and_mask.as_ptr(),
                bgra.as_ptr(),
            )
        };
        (hicon != 0).then_some(hicon)
    }
}

#[cfg(test)]
mod tests {
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
        let ico = include_bytes!("../../../../resources/app_icon/calyx.ico");
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
}
