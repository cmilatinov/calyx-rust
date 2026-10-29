//! The interface every post-processing pass implements.

use super::PostProcessSettings;
use crate::assets::texture::Texture;
use crate::render::RenderContext;
use egui_wgpu::wgpu;
use enumflags2::{bitflags, make_bitflags, BitFlags};
use std::fmt;
use std::sync::Arc;

/// Where a pass runs in the chain. Passes run in stage order, then in the
/// order they were added within a stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PostProcessStage {
    /// Linear HDR color in, linear HDR color out: bloom, depth of field.
    Hdr,
    /// The display transform, from linear HDR to display-linear color.
    Tonemap,
    /// Display-linear color in and out: color grading, vignette, film grain.
    Ldr,
}

/// A scene input besides the color a pass reads.
#[bitflags]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SceneInput {
    /// Scene depth, single-sample.
    Depth = 0b001,
    /// View-space normals. Not produced yet.
    Normals = 0b010,
    /// Screen-space motion vectors. Not produced yet.
    Velocity = 0b100,
}

impl fmt::Display for SceneInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Depth => "depth",
            Self::Normals => "normals",
            Self::Velocity => "velocity",
        })
    }
}

/// The scene inputs a pass reads besides color. The chain builds each one
/// only when an enabled pass asks for it.
pub type PassInputs = BitFlags<SceneInput>;

/// The scene inputs the renderer produces.
pub const PRODUCED_INPUTS: PassInputs = make_bitflags!(SceneInput::{Depth});

/// Checks the inputs `pass` reads against what the renderer produces, and
/// reports the first one it does not.
pub fn check_inputs(pass: &str, inputs: PassInputs) -> Result<(), PostProcessError> {
    match (inputs & !PRODUCED_INPUTS).iter().next() {
        Some(input) => Err(PostProcessError::InputUnavailable {
            pass: pass.to_string(),
            input,
        }),
        None => Ok(()),
    }
}

/// Why the chain skipped a pass.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PostProcessError {
    /// The pass reads a scene input the renderer does not produce yet.
    #[error("post-process pass `{pass}` needs {input}, which the renderer does not produce yet")]
    InputUnavailable {
        /// The pass's name.
        pass: String,
        /// The input it asked for.
        input: SceneInput,
    },
}

/// What a pass may use while preparing and recording.
pub struct PassContext<'a> {
    /// The render context the chain renders on.
    pub render_context: &'a Arc<RenderContext>,
    /// Size in pixels of every texture the chain reads and writes.
    pub size: (u32, u32),
    /// The near and far planes of the camera the scene was drawn with, so a
    /// pass can turn depth into view distance.
    pub camera: CameraPlanes,
}

/// The clip planes of the camera a frame was drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPlanes {
    /// Distance to the near plane.
    pub near: f32,
    /// Distance to the far plane.
    pub far: f32,
}

impl Default for CameraPlanes {
    /// The default render camera's planes.
    fn default() -> Self {
        Self {
            near: 0.1,
            far: 1000.0,
        }
    }
}

impl CameraPlanes {
    /// View distance of a depth-buffer value. The projection is
    /// `perspective_lh` with OpenGL's `[-1, 1]` depth range, of which the
    /// depth buffer keeps `[0, 1]`.
    pub fn linear_depth(&self, depth: f32) -> f32 {
        let (near, far) = (self.near, self.far);
        2.0 * near * far / (far + near - depth * (far - near))
    }

    /// The depth-buffer value of view distance `distance`, the inverse of
    /// [`Self::linear_depth`].
    pub fn depth_of(&self, distance: f32) -> f32 {
        let (near, far) = (self.near, self.far);
        (far + near - 2.0 * near * far / distance) / (far - near)
    }
}

/// The textures one pass reads and writes. `input` and `output` are never the
/// same texture.
pub struct PassIo<'a> {
    /// The color the pass reads.
    pub input: &'a Texture,
    /// The color target the pass writes. It is not cleared first: the pass
    /// writes every pixel.
    pub output: &'a Texture,
    /// Single-sample scene depth, when the pass asked for it.
    pub depth: Option<&'a Texture>,
}

/// One full-screen pass of the post-processing chain.
pub trait PostProcessPass: Send + Sync {
    /// Name used in logs and errors.
    fn name(&self) -> &str;

    /// The stage the pass runs in.
    fn stage(&self) -> PostProcessStage;

    /// The scene inputs the pass reads besides color.
    fn inputs(&self) -> PassInputs {
        PassInputs::empty()
    }

    /// Whether the pass runs this frame.
    fn enabled(&self, settings: &PostProcessSettings) -> bool;

    /// Uploads the pass's uniforms from `settings`. Runs before any pass
    /// records.
    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings);

    /// Records the pass: reads `io.input`, writes `io.output`.
    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo);

    /// Frees textures the pass owns besides the chain's pool targets. The
    /// chain calls it on resize and on every frame the pass does not run.
    fn release_targets(&mut self) {}
}
