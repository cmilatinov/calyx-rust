//! Game runtime setup shared by the standalone player and the editor: the
//! project's runtime settings, the GPU device both open, and per-frame
//! bookkeeping. [`run_player`] hosts a linked project in the standalone player.

mod host_command;
mod icon;
mod player;

pub use host_command::{HostCommand, HostCommands};
pub use icon::{
    calyx_icon, decode_icon, player_icon, set_window_icons, CALYX_ICON_PNG, CALYX_ICON_SIZE,
};
pub use player::run_player;

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

/// The game's window: its size, in logical points, and its icon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    /// An image file for the window's icon, relative to the project root. The
    /// player shows the Calyx icon when it is unset or cannot be loaded (see
    /// [`player_icon`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<PathBuf>,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            icon: None,
        }
    }
}

impl WindowSettings {
    /// Returns the window size as an egui size in logical points. A size with
    /// a zero dimension falls back to the default size, since a window cannot
    /// open collapsed.
    pub fn inner_size(&self) -> egui::Vec2 {
        let (width, height) = self.non_zero_size().unwrap_or_else(|| {
            let default = Self::default();
            (default.width, default.height)
        });
        egui::vec2(width as f32, height as f32)
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
        self.startup_scene
            .as_ref()
            .map(|scene| self.project_path(scene))
    }

    /// Returns `path` as an absolute path, resolving a relative one against
    /// the project root.
    pub fn project_path(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root_directory.join(path)
        }
    }

    pub fn runtime(&self) -> &RuntimeSettings {
        &self.runtime
    }

    pub fn window(&self) -> &WindowSettings {
        &self.runtime.window
    }
}

/// Describes the GPU device the player and the editor both request: the
/// shared features and limits, plus the optional features `adapter` has.
pub fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    let mut descriptor = shared_device_descriptor();
    descriptor.required_features |= OPTIONAL_FEATURES & adapter.features();
    descriptor
}

/// Features used when the adapter has them, with a fallback otherwise:
/// `DEPTH_CLIP_CONTROL` lets the shadow cascades clamp casters past their
/// near plane instead of clipping them, and the timestamp queries give the
/// profiler GPU timings per pass (see [`crate::profiler::gpu`]), which it
/// reports as unavailable without them.
const OPTIONAL_FEATURES: wgpu::Features = wgpu::Features::DEPTH_CLIP_CONTROL
    .union(wgpu::Features::TIMESTAMP_QUERY)
    .union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
    .union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);

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
mod tests;
