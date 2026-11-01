//! The ordered chain of post-processing passes.

use super::{
    check_inputs, CameraPlanes, DepthResolve, IntermediateSlot, PassContext, PassIo,
    PostProcessError, PostProcessPass, PostProcessSettings, SceneInput, TargetPool,
};
use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::render::{RenderContext, SceneRendererError};
use egui_wgpu::wgpu;
use std::sync::Arc;

/// A texture a planned pass reads or writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassTarget {
    /// The chain's input: the resolved HDR scene color.
    Source,
    /// One of the two pool targets.
    Intermediate(IntermediateSlot),
    /// The chain's output: the presented scene texture.
    Output,
}

/// One pass of a frame's plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedPass {
    /// The pass's index in the chain.
    pub pass: usize,
    /// The texture it reads.
    pub input: PassTarget,
    /// The texture it writes.
    pub output: PassTarget,
}

/// The passes a frame runs and the textures that connect them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChainPlan {
    /// The passes that run, in order.
    pub passes: Vec<PlannedPass>,
    /// Enabled passes skipped because an input they read is unavailable.
    pub skipped: Vec<PostProcessError>,
    /// How many pool targets the plan uses: none for one pass, else up to
    /// two that alternate.
    pub intermediates: usize,
    /// Whether a planned pass reads depth.
    pub depth: bool,
    /// Whether a planned pass reads [`SceneInput::ParticleDepth`].
    pub particle_depth: bool,
}

impl ChainPlan {
    /// Plans `passes`, already in chain order, for `settings`: keeps each
    /// enabled pass whose inputs are available, reads the source into the
    /// first, writes the output from the last, and alternates two pool
    /// targets between them.
    pub fn build(passes: &[Box<dyn PostProcessPass>], settings: &PostProcessSettings) -> Self {
        let mut plan = Self::default();
        let mut running = Vec::new();
        for (index, pass) in passes.iter().enumerate() {
            if !pass.enabled(settings) {
                continue;
            }
            let inputs = pass.inputs();
            match check_inputs(pass.name(), inputs) {
                Ok(()) => {
                    plan.depth |= inputs.contains(SceneInput::Depth);
                    plan.particle_depth |= inputs.contains(SceneInput::ParticleDepth);
                    running.push(index);
                }
                Err(error) => plan.skipped.push(error),
            }
        }
        let count = running.len();
        plan.intermediates = count.saturating_sub(1).min(2);
        plan.passes = running
            .into_iter()
            .enumerate()
            .map(|(position, pass)| PlannedPass {
                pass,
                input: if position == 0 {
                    PassTarget::Source
                } else {
                    PassTarget::Intermediate(IntermediateSlot::written_by(position - 1))
                },
                output: if position + 1 == count {
                    PassTarget::Output
                } else {
                    PassTarget::Intermediate(IntermediateSlot::written_by(position))
                },
            })
            .collect();
        plan
    }
}

/// The textures a chain run reads and writes.
pub struct ChainTargets<'a> {
    /// Resolved linear HDR scene color, single-sample. The chain never
    /// writes it.
    pub source: &'a Texture,
    /// The presented texture the last pass writes. Same size as `source`.
    pub output: &'a Texture,
    /// The scene's depth, multisampled or not.
    pub depth: &'a Texture,
    /// The scene's depth with every particle drawn in, nearest wins, at the
    /// same sample count, or `None` when the renderer drew none and it would
    /// equal `depth`.
    pub particle_depth: Option<&'a Texture>,
}

/// Post-processing passes in stage order, then insertion order, with the
/// pool their intermediate targets come from.
pub struct PostProcessChain {
    passes: Vec<Box<dyn PostProcessPass>>,
    pool: TargetPool,
    depth: DepthResolve,
    particle_depth: DepthResolve,
    camera: CameraPlanes,
}

impl PostProcessChain {
    /// An empty chain, or the reason its shared shaders did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        Ok(Self {
            passes: Vec::new(),
            pool: TargetPool::default(),
            depth: DepthResolve::new(context)?,
            particle_depth: DepthResolve::new(context)?,
            camera: CameraPlanes::default(),
        })
    }

    /// Adds `pass` after every pass of its stage and the stages before it.
    pub fn push(&mut self, pass: Box<dyn PostProcessPass>) {
        let stage = pass.stage();
        let position = self
            .passes
            .iter()
            .position(|existing| existing.stage() > stage)
            .unwrap_or(self.passes.len());
        self.passes.insert(position, pass);
    }

    /// Sets the planes of the camera the next frames are drawn with, which
    /// passes that read depth see in [`PassContext::camera`].
    pub fn set_camera(&mut self, camera: CameraPlanes) {
        self.camera = camera;
    }

    /// The passes, in the order they run.
    pub fn passes(&self) -> &[Box<dyn PostProcessPass>] {
        &self.passes
    }

    /// The pool intermediate targets come from.
    pub fn pool(&self) -> &TargetPool {
        &self.pool
    }

    /// Whether a pass that runs with `settings` reads `input`, so the
    /// renderer builds it only then.
    pub fn reads(&self, settings: &PostProcessSettings, input: SceneInput) -> bool {
        let plan = ChainPlan::build(&self.passes, settings);
        match input {
            SceneInput::Depth => plan.depth,
            SceneInput::ParticleDepth => plan.particle_depth,
            SceneInput::Normals | SceneInput::Velocity => false,
        }
    }

    /// Frees intermediate targets, the depth copies and the textures passes
    /// own, as a resize needs.
    pub fn clear_targets(&mut self) {
        self.pool.clear();
        self.depth.clear();
        self.particle_depth.clear();
        for pass in &mut self.passes {
            pass.release_targets();
        }
    }

    /// Records the frame's enabled passes into `encoder`, from
    /// `targets.source` to `targets.output`, and returns the plan it ran.
    ///
    /// With no pass enabled, the source is copied to the output. A pass that
    /// has no pipeline has its input copied to its output, and is logged
    /// once.
    pub fn run(
        &mut self,
        render_context: &Arc<RenderContext>,
        encoder: &mut wgpu::CommandEncoder,
        settings: &PostProcessSettings,
        targets: ChainTargets,
    ) -> ChainPlan {
        let plan = ChainPlan::build(&self.passes, settings);
        for (index, pass) in self.passes.iter_mut().enumerate() {
            if !plan.passes.iter().any(|planned| planned.pass == index) {
                pass.release_targets();
            }
        }
        for error in &plan.skipped {
            crate::logging::warn_once(format!("post-process-skipped:{error}"), || {
                format!("Skipping a post-process pass: {error}")
            });
        }
        if plan.passes.is_empty() {
            copy(encoder, targets.source, targets.output);
            return plan;
        }

        let Self {
            passes,
            pool,
            depth,
            particle_depth,
            camera,
        } = self;
        let size = (
            targets.output.descriptor.size.width,
            targets.output.descriptor.size.height,
        );
        pool.acquire(
            render_context,
            size,
            targets.output.descriptor.format,
            plan.intermediates,
        );
        // Without a particle depth of its own, particle depth is the scene
        // depth, resolved once for both.
        let depth = (plan.depth || (plan.particle_depth && targets.particle_depth.is_none()))
            .then(|| depth.resolve(render_context, encoder, targets.depth));
        if targets.particle_depth.is_none() || !plan.particle_depth {
            particle_depth.clear();
        }
        let particle_depth = match targets.particle_depth {
            Some(texture) if plan.particle_depth => {
                Some(particle_depth.resolve(render_context, encoder, texture))
            }
            _ => plan.particle_depth.then_some(depth).flatten(),
        };
        let context = PassContext {
            render_context,
            size,
            camera: *camera,
        };
        for planned in &plan.passes {
            passes[planned.pass].prepare(&context, settings);
        }
        let texture = |target: PassTarget| match target {
            PassTarget::Source => targets.source,
            PassTarget::Intermediate(slot) => pool.texture(slot),
            PassTarget::Output => targets.output,
        };
        for planned in &plan.passes {
            let pass = &mut passes[planned.pass];
            let inputs = pass.inputs();
            let io = PassIo {
                input: texture(planned.input),
                output: texture(planned.output),
                depth: inputs
                    .contains(SceneInput::Depth)
                    .then_some(depth)
                    .flatten(),
                particle_depth: inputs
                    .contains(SceneInput::ParticleDepth)
                    .then_some(particle_depth)
                    .flatten(),
            };
            let (input, output) = (io.input, io.output);
            if let Err(error) = pass.record(&context, encoder, io) {
                crate::logging::warn_once(
                    format!("post-process-no-pipeline:{}:{error}", pass.name()),
                    || {
                        format!(
                            "Post-process pass `{}` passes its input through: {error}",
                            pass.name()
                        )
                    },
                );
                copy(encoder, input, output);
            }
        }
        plan
    }
}

/// Copies `source` to `destination`, which has its size and format.
fn copy(encoder: &mut wgpu::CommandEncoder, source: &Texture, destination: &Texture) {
    encoder.copy_texture_to_texture(
        source.texture.as_image_copy(),
        destination.texture.as_image_copy(),
        destination.descriptor.size,
    );
}
