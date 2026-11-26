use super::*;
use egui_wgpu::wgpu;

fn run_frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    mut draw: impl FnMut(&mut Ui) -> Response,
) -> egui::Rect {
    let mut rect = egui::Rect::NOTHING;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| rect = draw(ui).rect);
    });
    rect
}

fn click_events(pos: egui::Pos2) -> Vec<egui::Event> {
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![egui::Event::PointerMoved(pos), button(true), button(false)]
}

#[test]
fn asset_select_thumbnails_need_an_installed_handle_and_a_supported_type() {
    use calyx_engine::render::Shader;

    let ctx = egui::Context::default();
    let service = crate::widgets::ThumbnailService::default();
    let mut enabled = Vec::new();
    let mut check = |ctx: &egui::Context| {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                enabled.push((
                    Widgets::asset_select_thumbnails(ui, Some(Texture::type_uuid())).is_some(),
                    Widgets::asset_select_thumbnails(ui, None).is_some(),
                    Widgets::asset_select_thumbnails(ui, Some(Shader::type_uuid())).is_some(),
                ));
            });
        });
    };

    check(&ctx);
    service.handle().install(&ctx);
    check(&ctx);

    assert_eq!(enabled, vec![(false, false, false), (true, true, false)]);
}

#[test]
fn asset_select_requests_selected_and_listed_thumbnails() {
    use crate::widgets::{ThumbnailPriority, ThumbnailService, ThumbnailStatus};

    let root = std::env::temp_dir().join(format!("calyx-asset-select-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("failed to create asset root");
    let selected_path = root.join("selected.png");
    let listed_path = root.join("listed.png");
    std::fs::write(&selected_path, b"a").expect("failed to write texture source");
    std::fs::write(&listed_path, b"b").expect("failed to write texture source");

    let context = calyx_engine::test_support::test_asset_context_with_assets(vec![root.clone()]);
    let registry = context.registries.assets.read();
    let key = |path: &std::path::Path| {
        ThumbnailRequest::from_asset_path(&registry, path)
            .expect("texture should support thumbnails")
            .key()
    };
    let (selected_key, listed_key) = (key(&selected_path), key(&listed_path));
    let mut value = selected_key.asset_id;

    let service = ThumbnailService::default();
    let ctx = egui::Context::default();
    service.handle().install(&ctx);
    let mut draw = |ui: &mut Ui| {
        Widgets::asset_select(
            ui,
            &registry,
            "texture_select",
            Some(Texture::type_uuid()),
            &mut value,
        )
    };

    let combo = run_frame(&ctx, Vec::new(), &mut draw);
    assert_eq!(
        service.status(selected_key),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::Normal
        }
    );
    assert_eq!(service.status(listed_key), ThumbnailStatus::Missing);

    run_frame(&ctx, click_events(combo.center()), &mut draw);
    run_frame(&ctx, Vec::new(), &mut draw);

    let high = ThumbnailStatus::Queued {
        priority: ThumbnailPriority::High,
    };
    assert_eq!(service.status(listed_key), high);
    assert_eq!(service.status(selected_key), high);

    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn search_select_focuses_search_field_when_opened() {
    let context = egui::Context::default();
    let mut state = SearchSelectState::default();
    state.open();
    let mut search_id = None;

    let _ = context.run(Default::default(), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            let selector_id = ui.make_persistent_id("selector");
            search_id = Some(selector_id.with("search"));
            Widgets::search_select_contents(ui, "selector", &mut state, |_, _| {});
        });
    });

    assert!(context.memory(|memory| memory.has_focus(search_id.unwrap())));
    assert!(!state.should_request_focus);
}

#[test]
fn texture_2d_filter_accepts_loaded_2d_texture() {
    let context = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
    let render_context = context.render_context.clone();
    let texture = context
        .registries
        .assets
        .read()
        .create(
            "test_texture_2d".into(),
            Texture::solid_color_2d(render_context, "test_texture_2d", [255, 255, 255, 255]),
        )
        .unwrap();
    let registry = context.registries.assets.read();
    let meta = registry.asset_meta_from_id(texture.id()).unwrap();

    assert!(Widgets::asset_is_texture_2d(&registry, &meta));
}

#[test]
fn texture_2d_filter_rejects_loaded_cube_texture() {
    let context = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
    let render_context = context.render_context.clone();
    let texture = context
        .registries
        .assets
        .read()
        .create(
            "test_texture_cube".into(),
            Texture::new(
                render_context,
                &wgpu::TextureDescriptor {
                    label: Some("test_texture_cube"),
                    size: wgpu::Extent3d {
                        width: 16,
                        height: 16,
                        depth_or_array_layers: 6,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                Some(wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::Cube),
                    ..Default::default()
                }),
                false,
            ),
        )
        .unwrap();
    let registry = context.registries.assets.read();
    let meta = registry.asset_meta_from_id(texture.id()).unwrap();

    assert!(!Widgets::asset_is_texture_2d(&registry, &meta));
}

#[test]
fn texture_2d_filter_rejects_loaded_multisampled_texture() {
    let context = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
    let render_context = context.render_context.clone();
    let texture = context
        .registries
        .assets
        .read()
        .create(
            "test_texture_msaa".into(),
            Texture::new(
                render_context,
                &wgpu::TextureDescriptor {
                    label: Some("test_texture_msaa"),
                    size: wgpu::Extent3d {
                        width: 16,
                        height: 16,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 4,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
                None,
                Some(wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    ..Default::default()
                }),
                false,
            ),
        )
        .unwrap();
    let registry = context.registries.assets.read();
    let meta = registry.asset_meta_from_id(texture.id()).unwrap();

    assert!(!Widgets::asset_is_texture_2d(&registry, &meta));
}
