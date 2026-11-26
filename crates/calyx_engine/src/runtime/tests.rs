use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    device_descriptor, physical_size, shared_device_descriptor, FrameCounter,
    ProjectRuntimeSettings, RuntimeSettings, WindowSettings,
};

fn temp_project(manifest: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("calyx-runtime-settings-{unique}"));
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(root.join("project.toml"), manifest).unwrap();
    root
}

#[test]
fn loads_startup_scene_and_window_from_project_manifest() {
    let root = temp_project(
            "name = \"Configured project\"\ncreation_date = \"2026-01-01T00:00:00Z\"\nstartup_scene = \"assets/startup.cxscene\"\n[runtime.window]\nwidth = 1024\nheight = 768\n",
        );

    let settings = ProjectRuntimeSettings::load(&root).unwrap();

    assert_eq!(settings.root_directory(), root.as_path());
    assert_eq!(settings.assets_directory(), root.join("assets"));
    assert_eq!(
        settings.startup_scene_path(),
        Some(root.join("assets/startup.cxscene"))
    );
    assert_eq!(
        settings.runtime(),
        &RuntimeSettings {
            window: WindowSettings {
                width: 1024,
                height: 768,
                icon: None,
            },
        }
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_runtime_sections_use_defaults() {
    let settings =
        ProjectRuntimeSettings::from_manifest("root", "name = \"Bare project\"\n").unwrap();

    assert_eq!(settings.startup_scene_path(), None);
    assert_eq!(settings.window(), &WindowSettings::default());
    assert_eq!(settings.window().non_zero_size(), Some((1280, 720)));
}

#[test]
fn absolute_startup_scene_is_kept() {
    let scene = std::env::temp_dir().join("absolute.cxscene");
    let manifest = format!("startup_scene = {:?}\n", scene.display().to_string());

    let settings = ProjectRuntimeSettings::from_manifest("root", &manifest).unwrap();

    assert_eq!(settings.startup_scene_path(), Some(scene));
}

#[test]
fn missing_project_file_is_an_error() {
    let root = std::env::temp_dir().join("calyx-runtime-settings-missing");

    let error = ProjectRuntimeSettings::load(&root).unwrap_err();

    assert!(error.to_string().contains("project.toml"), "{error}");
}

#[test]
fn zero_window_dimension_has_no_size() {
    let window = WindowSettings {
        width: 0,
        height: 720,
        icon: None,
    };

    assert_eq!(window.non_zero_size(), None);
}

#[test]
fn zero_window_dimension_opens_at_default_size() {
    let window = WindowSettings {
        width: 1024,
        height: 0,
        icon: None,
    };

    assert_eq!(window.inner_size(), WindowSettings::default().inner_size());
    assert_eq!(window.inner_size(), egui::vec2(1280.0, 720.0));
}

#[test]
fn physical_size_scales_by_pixels_per_point() {
    assert_eq!(physical_size(egui::vec2(640.0, 360.0), 1.5), (960, 540));
}

#[test]
fn frame_counter_reports_frames_of_the_last_whole_second() {
    let mut counter = FrameCounter::default();

    assert!(!counter.record_frame(0.2));
    assert!(!counter.record_frame(0.6));
    assert_eq!(counter.fps(), 0);
    assert!(counter.record_frame(1.0));
    assert_eq!(counter.fps(), 3);
    assert!(!counter.record_frame(0.1));
    assert_eq!(counter.fps(), 3);
}

#[test]
fn device_descriptor_adds_depth_clamping_and_timestamps_where_the_adapter_has_them() {
    use egui_wgpu::wgpu::Features;
    let adapter = crate::test_support::test_adapter();
    let requested = device_descriptor(adapter).required_features;
    assert!(requested.contains(shared_device_descriptor().required_features));
    for feature in [
        Features::DEPTH_CLIP_CONTROL,
        Features::TIMESTAMP_QUERY,
        Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
        Features::TIMESTAMP_QUERY_INSIDE_PASSES,
    ] {
        assert_eq!(
            requested.contains(feature),
            adapter.features().contains(feature),
            "{feature:?}"
        );
    }
}

#[test]
fn device_descriptor_requests_shared_features_and_limits() {
    let descriptor = shared_device_descriptor();

    assert!(descriptor
        .required_features
        .contains(egui_wgpu::wgpu::Features::POLYGON_MODE_LINE));
    assert_eq!(
        descriptor
            .required_limits
            .max_uniform_buffers_per_shader_stage,
        30
    );
}
