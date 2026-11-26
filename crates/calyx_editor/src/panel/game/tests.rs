use super::{aspect_label, cursor_grabbed_after_input, game_image, hud_input};
use crate::EditorApp;
use calyx_engine::input::{Input, InputState};
use calyx_engine::math::Transform;
use calyx_engine::render::Camera;
use egui::{pos2, vec2, Event, Pos2, Rect, Response, TextureId, Vec2};

#[test]
fn project_window_dimensions_show_their_aspect_instead_of_none() {
    assert_eq!(aspect_label(Some((1920, 1080))), "16:9");
    assert_eq!(aspect_label(Some((1600, 900))), "16:9");
    assert_eq!(aspect_label(Some((4, 3))), "4:3");
    assert_eq!(aspect_label(Some((21, 9))), "21:9");
    assert_eq!(aspect_label(Some((1000, 700))), "10:7");
    assert_eq!(aspect_label(None), "None");
}

fn image_frame(
    ctx: &egui::Context,
    size: Vec2,
    aspect: Option<(u32, u32)>,
    pixels_per_point: f32,
    pointer: Option<Pos2>,
) -> Response {
    let mut raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(43.0, 57.0), size)),
        events: pointer.into_iter().map(Event::PointerMoved).collect(),
        ..Default::default()
    };
    let viewport = raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
    viewport.native_pixels_per_point = Some(pixels_per_point);
    // Window dimensions need not match the area egui gives its panels.
    viewport.inner_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0)));
    let mut response = None;
    let output = ctx.run(raw, |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let image = game_image(ui, Some(TextureId::User(99)), aspect);
                let mut state = InputState::default();
                let input = Input::from_ctx(ctx, Some(&image), &mut state, true);
                assert_eq!(input.surface_rect(), Some(image.rect));
                if let Some(pointer) = pointer {
                    assert_eq!(input.pointer_pos(), Some(pointer));
                    let hud_pointer = hud_input(ui).pointer_position.unwrap();
                    assert_eq!((hud_pointer.x, hud_pointer.y), (pointer.x, pointer.y));
                    assert_pointer_projects_to_image(ctx, &input, image.rect);
                }
                response = Some(image);
            });
    });
    let response = response.unwrap();
    let painted = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Mesh(mesh) if mesh.texture_id == TextureId::User(99) => {
            Some(mesh.calc_bounds())
        }
        _ => None,
    });
    assert_eq!(
        painted,
        Some(response.rect),
        "input and HUD must occupy the painted image"
    );
    response
}

fn assert_pointer_projects_to_image(ctx: &egui::Context, input: &Input, rect: Rect) {
    let local = input.pointer_pos().unwrap() - rect.min;
    let transform = Transform::default();
    let input_camera = Camera::new(rect.aspect_ratio(), 64.0_f32.to_radians(), 0.1, 100.0);
    let ray = input_camera
        .screen_ray(
            &transform,
            nalgebra_glm::vec2(local.x, local.y),
            nalgebra_glm::vec2(rect.width(), rect.height()),
        )
        .unwrap();
    let (width, height) = EditorApp::game_physical_size(ctx, rect.size());
    assert!((width as f32 - rect.width() * ctx.pixels_per_point()).abs() < 0.001);
    assert!((height as f32 - rect.height() * ctx.pixels_per_point()).abs() < 0.001);
    let render_camera = Camera::new(
        width as f32 / height as f32,
        64.0_f32.to_radians(),
        0.1,
        100.0,
    );
    let world = ray.origin + ray.direction * 10.0;
    let clip = render_camera.projection * world.push(1.0);
    let pixel = vec2(
        (clip.x / clip.w + 1.0) * 0.5 * width as f32,
        (1.0 - clip.y / clip.w) * 0.5 * height as f32,
    );
    let screen = rect.min + pixel / ctx.pixels_per_point();
    assert!((screen - input.pointer_pos().unwrap()).length() < 0.01);
}

#[test]
fn game_image_input_matches_rendering_after_resize_aspect_and_dpi_changes() {
    let ctx = egui::Context::default();
    for pixels_per_point in [1.0, 1.25, 1.5, 2.0] {
        for (size, aspect) in [
            (vec2(1650.25, 746.5), Some((1920, 1080))),
            (vec2(800.5, 1100.25), Some((16, 9))),
            (vec2(1300.0, 700.0), None),
            (vec2(1100.0, 700.0), Some((4, 3))),
            (vec2(1100.0, 700.0), Some((21, 9))),
        ] {
            let image = image_frame(&ctx, size, aspect, pixels_per_point, None);
            // Corners, edge midpoints and centre all go through gameplay's
            // response and camera projection, then back through the texture.
            for y in [0.0, 0.5, 1.0] {
                for x in [0.0, 0.5, 1.0] {
                    let pointer = image.rect.min + image.rect.size() * vec2(x, y);
                    image_frame(&ctx, size, aspect, pixels_per_point, Some(pointer));
                }
            }
        }
    }
}

#[test]
fn letterbox_bars_are_outside_the_game_input_surface() {
    let ctx = egui::Context::default();
    let size = vec2(1200.0, 600.0);
    let aspect = Some((4, 3));
    let image = image_frame(&ctx, size, aspect, 1.0, None);
    assert_eq!(image.rect.size(), vec2(800.0, 600.0));
    let bar = image.rect.left_center() - vec2(50.0, 0.0);
    // Interaction is decided against the previous frame's widgets.
    image_frame(&ctx, size, aspect, 1.0, Some(bar));
    assert!(!image_frame(&ctx, size, aspect, 1.0, Some(bar)).hovered());
    image_frame(&ctx, size, aspect, 1.0, Some(image.rect.center()));
    assert!(image_frame(&ctx, size, aspect, 1.0, Some(image.rect.center())).hovered());
}

#[test]
fn clicking_the_game_viewport_grabs_input() {
    assert!(cursor_grabbed_after_input(false, true, true, false));
}

#[test]
fn clicking_outside_the_game_viewport_releases_input() {
    assert!(!cursor_grabbed_after_input(true, false, true, false));
}

#[test]
fn escape_releases_game_viewport_input() {
    assert!(!cursor_grabbed_after_input(true, false, false, true));
}
