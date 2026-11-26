use super::*;

#[test]
fn selected_skybox_ref_returns_none_without_selection() {
    let assets = AssetRenderState::default();

    assert!(SkyboxRenderer::selected_skybox_ref(None, &assets).is_none());
}

#[test]
fn selected_skybox_ref_returns_none_for_missing_asset() {
    let assets = AssetRenderState::default();
    let skybox_id = Uuid::new_v4();

    assert!(SkyboxRenderer::selected_skybox_ref(Some(skybox_id), &assets).is_none());
}

#[test]
fn selected_skybox_maps_are_withheld_until_prepared() {
    let test_assets = crate::test_support::TestAssets::engine();
    let context = test_assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let mut renderer = SkyboxRenderer::new(&context).expect("skybox shaders should load");
    let source = Texture::solid_color_2d(render_context.clone(), "sky", [64, 128, 255, 255]);
    let skybox = Ref::new(Skybox::new(&render_context, source));
    let skybox_id = Uuid::new_v4();
    let mut assets = AssetRenderState::default();
    assets.skyboxes.insert(skybox_id, skybox.clone());
    renderer.set_skybox(Some(skybox_id));

    assert!(
        skybox.read().environment_maps().is_none(),
        "a skybox must not light or draw anything before its maps are computed"
    );

    let mut encoder =
        render_context
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("skybox_prepare_test"),
            });
    renderer.prepare(&render_context, &mut encoder, &assets);
    render_context.queue().submit([encoder.finish()]);

    let skybox = skybox.read();
    let (irradiance, _, _) = skybox
        .environment_maps()
        .expect("prepare should make the skybox's maps available");
    assert!(
        irradiance.descriptor.size.width <= 64,
        "the irradiance map should stay small enough to compute within a frame, got {}",
        irradiance.descriptor.size.width
    );
}
