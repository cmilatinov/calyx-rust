use super::composite_hud;
use image::{Rgba, RgbaImage};

#[test]
fn the_hud_covers_the_game_view_by_its_alpha() {
    let mut view = RgbaImage::from_pixel(3, 1, Rgba([200, 100, 50, 255]));
    let mut hud = RgbaImage::new(3, 1);
    // Clear, opaque white, and half-covering premultiplied black.
    hud.put_pixel(1, 0, Rgba([255, 255, 255, 255]));
    hud.put_pixel(2, 0, Rgba([0, 0, 0, 128]));
    composite_hud(&mut view, &hud).unwrap();
    assert_eq!(view.get_pixel(0, 0), &Rgba([200, 100, 50, 255]));
    assert_eq!(view.get_pixel(1, 0), &Rgba([255, 255, 255, 255]));
    assert_eq!(view.get_pixel(2, 0), &Rgba([100, 50, 25, 255]));
}

#[test]
fn a_hud_of_another_size_is_refused() {
    let mut view = RgbaImage::new(4, 4);
    assert!(composite_hud(&mut view, &RgbaImage::new(2, 2)).is_err());
}
