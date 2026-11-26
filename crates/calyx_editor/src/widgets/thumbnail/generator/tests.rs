use super::*;
use calyx_engine::component::{ComponentBloom, ComponentGlobalPostProcessVolume, ComponentTonemap};
use calyx_engine::test_support::TestAssets;

#[test]
fn a_scene_job_releases_its_resolved_effects_before_assembly_reload() {
    let fixture = TestAssets::engine();
    let assets = fixture.asset_context();
    let context = assets.lock_read();
    let mut scene = context.scene();
    let volume = scene.create(None, None);
    scene.add_component(volume, ComponentGlobalPostProcessVolume::default());
    scene.add_component(volume, ComponentBloom::default());
    let view = SceneThumbnailView {
        camera: Camera::default(),
        camera_transform: Transform::from_xyz(0.0, 0.0, -5.0),
        clear_color: Color32::TRANSPARENT,
        from_main_camera: false,
    };
    let mut generator =
        ThumbnailGenerator::with_render_settings(ThumbnailRenderSettings::with_size_px(64));
    for _ in 0..2 {
        generator
            .render_scene_view(&context, context.render_context.as_ref(), &scene, &view)
            .expect("the scene thumbnail renders");
        let renderer = generator.scene_renderer.as_ref().unwrap();
        assert!(renderer
            .post_process_settings()
            .get::<ComponentBloom>()
            .is_none());
        assert!(renderer
            .post_process_settings()
            .get::<ComponentTonemap>()
            .is_none());
    }
}
