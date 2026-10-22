//! Game runtime setup shared by the standalone player and the editor: the
//! project's runtime settings, the GPU device both open, and per-frame
//! bookkeeping.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_wgpu::{wgpu, SurfaceErrorAction, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use serde::{Deserialize, Serialize};

use crate::core::Time;
use crate::error::BoxedError;

/// File name of the project manifest at a project's root directory.
pub const PROJECT_FILE_NAME: &str = "project.toml";

/// Name of the [`Time`] timer that [`FrameCounter`] measures seconds with.
const FPS_TIMER: &str = "fps";

/// Runtime defaults that belong to a project rather than a particular player.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeSettings {
    pub window: WindowSettings,
}

/// Window size, in logical points, that the game opens at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
        }
    }
}

impl WindowSettings {
    /// Returns the window size as an egui size in logical points.
    pub fn inner_size(&self) -> egui::Vec2 {
        egui::vec2(self.width as f32, self.height as f32)
    }

    /// Returns `(width, height)` when neither dimension is zero.
    pub fn non_zero_size(&self) -> Option<(u32, u32)> {
        (self.width != 0 && self.height != 0).then_some((self.width, self.height))
    }
}

/// The runtime-relevant part of a project manifest.
#[derive(Default, Deserialize)]
#[serde(default)]
struct ProjectRuntimeManifest {
    startup_scene: Option<PathBuf>,
    runtime: RuntimeSettings,
}

/// A project's runtime settings, read from its `project.toml`.
///
/// Only the fields a runtime needs are read; the rest of the manifest is
/// ignored.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRuntimeSettings {
    root_directory: PathBuf,
    startup_scene: Option<PathBuf>,
    runtime: RuntimeSettings,
}

impl ProjectRuntimeSettings {
    /// Reads the runtime settings of the project rooted at `root_directory`.
    pub fn load(root_directory: impl Into<PathBuf>) -> Result<Self, BoxedError> {
        let root_directory = root_directory.into();
        let manifest_path = root_directory.join(PROJECT_FILE_NAME);
        let manifest = fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "Failed to read project file '{}': {error}",
                manifest_path.display()
            )
        })?;
        Self::from_manifest(root_directory, &manifest)
    }

    /// Parses runtime settings from the text of a project manifest.
    pub fn from_manifest(
        root_directory: impl Into<PathBuf>,
        manifest: &str,
    ) -> Result<Self, BoxedError> {
        let manifest: ProjectRuntimeManifest = toml::from_str(manifest)
            .map_err(|error| format!("Failed to parse project file: {error}"))?;
        Ok(Self {
            root_directory: root_directory.into(),
            startup_scene: manifest.startup_scene,
            runtime: manifest.runtime,
        })
    }

    pub fn root_directory(&self) -> &Path {
        &self.root_directory
    }

    /// Returns the project's assets directory.
    pub fn assets_directory(&self) -> PathBuf {
        self.root_directory.join("assets")
    }

    /// Returns the configured startup scene as an absolute path, resolving a
    /// relative entry against the project root.
    pub fn startup_scene_path(&self) -> Option<PathBuf> {
        self.startup_scene.as_ref().map(|scene| {
            if scene.is_absolute() {
                scene.clone()
            } else {
                self.root_directory.join(scene)
            }
        })
    }

    pub fn runtime(&self) -> &RuntimeSettings {
        &self.runtime
    }

    pub fn window(&self) -> &WindowSettings {
        &self.runtime.window
    }
}

/// Describes the GPU device the player and the editor both request.
pub fn device_descriptor(_adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    shared_device_descriptor()
}

fn shared_device_descriptor() -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        required_features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::POLYGON_MODE_LINE
            | wgpu::Features::CLEAR_TEXTURE
            | wgpu::Features::FLOAT32_FILTERABLE
            | wgpu::Features::DEPTH32FLOAT_STENCIL8
            | wgpu::Features::BUFFER_BINDING_ARRAY
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::STORAGE_RESOURCE_BINDING_ARRAY
            | wgpu::Features::UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING,
        required_limits: wgpu::Limits {
            max_storage_textures_per_shader_stage: 5,
            max_uniform_buffers_per_shader_stage: 30,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Returns the eframe wgpu configuration that opens [`device_descriptor`] on a
/// high-performance adapter and skips frames on surface errors.
pub fn wgpu_configuration() -> WgpuConfiguration {
    WgpuConfiguration {
        present_mode: Default::default(),
        desired_maximum_frame_latency: None,
        on_surface_error: Arc::new(|_| SurfaceErrorAction::SkipFrame),
        wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
            instance_descriptor: Default::default(),
            power_preference: wgpu::PowerPreference::HighPerformance,
            native_adapter_selector: None,
            device_descriptor: Arc::new(device_descriptor),
            trace_path: None,
        }),
    }
}

/// Returns the window's inner size in physical pixels, used to size render
/// targets before the first frame lays out. It is `(0, 0)` while the size is
/// unknown.
pub fn initial_render_size(ctx: &egui::Context) -> (u32, u32) {
    let Some(window_size) = ctx.input(|input| input.viewport().inner_rect) else {
        return (0, 0);
    };
    physical_size(window_size.size(), ctx.pixels_per_point())
}

fn physical_size(size: egui::Vec2, pixels_per_point: f32) -> (u32, u32) {
    (
        (size.x * pixels_per_point) as u32,
        (size.y * pixels_per_point) as u32,
    )
}

/// Counts presented frames and reports how many were drawn over the last
/// whole second.
#[derive(Clone, Debug, Default)]
pub struct FrameCounter {
    frames: usize,
    fps: usize,
}

impl FrameCounter {
    /// Records one frame, measuring the second with the runtime's clock.
    pub fn tick(&mut self, time: &Time) {
        if self.record_frame(time.timer(FPS_TIMER)) {
            time.reset_timer(FPS_TIMER);
        }
    }

    /// Records one frame `elapsed` seconds into the current second. Returns
    /// true when that second is over and its timer should restart.
    fn record_frame(&mut self, elapsed: f32) -> bool {
        self.frames += 1;
        if elapsed < 1.0 {
            return false;
        }
        self.fps = self.frames;
        self.frames = 0;
        true
    }

    /// Returns the frame count of the last whole second, or 0 before one has
    /// passed.
    pub fn fps(&self) -> usize {
        self.fps
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        physical_size, shared_device_descriptor, FrameCounter, ProjectRuntimeSettings,
        RuntimeSettings, WindowSettings,
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
        };

        assert_eq!(window.non_zero_size(), None);
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
}
