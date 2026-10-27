//! The ordered chain of post-processing passes.

use super::{
    DepthResolve, PassContext, PassIo, PostProcessError, PostProcessPass, PostProcessSettings,
    TargetKey, TargetPool,
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
    /// Pool target A (`0`) or B (`1`).
    Intermediate(usize),
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
            match inputs.check(pass.name()) {
                Ok(()) => {
                    plan.depth |= inputs.depth;
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
                    PassTarget::Intermediate((position - 1) % 2)
                },
                output: if position + 1 == count {
                    PassTarget::Output
                } else {
                    PassTarget::Intermediate(position % 2)
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
}

/// Post-processing passes in stage order, then insertion order, with the
/// pool their intermediate targets come from.
pub struct PostProcessChain {
    passes: Vec<Box<dyn PostProcessPass>>,
    pool: TargetPool,
    depth: DepthResolve,
}

impl PostProcessChain {
    /// An empty chain, or the reason its shared shaders did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        Ok(Self {
            passes: Vec::new(),
            pool: TargetPool::default(),
            depth: DepthResolve::new(context)?,
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

    /// The passes, in the order they run.
    pub fn passes(&self) -> &[Box<dyn PostProcessPass>] {
        &self.passes
    }

    /// The pool intermediate targets come from.
    pub fn pool(&self) -> &TargetPool {
        &self.pool
    }

    /// Frees intermediate targets and the depth copy, as a resize needs.
    pub fn clear_targets(&mut self) {
        self.pool.clear();
        self.depth.clear();
    }

    /// Records the frame's enabled passes into `encoder`, from
    /// `targets.source` to `targets.output`, and returns the plan it ran.
    ///
    /// With no pass enabled, the source is copied to the output.
    pub fn run(
        &mut self,
        render_context: &Arc<RenderContext>,
        encoder: &mut wgpu::CommandEncoder,
        settings: &PostProcessSettings,
        targets: ChainTargets,
    ) -> ChainPlan {
        let plan = ChainPlan::build(&self.passes, settings);
        for error in &plan.skipped {
            crate::logging::warn_once(format!("post-process-skipped:{error}"), || {
                format!("Skipping a post-process pass: {error}")
            });
        }
        if plan.passes.is_empty() {
            encoder.copy_texture_to_texture(
                targets.source.texture.as_image_copy(),
                targets.output.texture.as_image_copy(),
                targets.output.descriptor.size,
            );
            return plan;
        }

        let Self {
            passes,
            pool,
            depth,
        } = self;
        let size = (
            targets.output.descriptor.size.width,
            targets.output.descriptor.size.height,
        );
        let intermediates = pool.acquire(
            render_context,
            TargetKey::color(size, targets.output.descriptor.format),
            plan.intermediates,
        );
        let depth = plan
            .depth
            .then(|| depth.resolve(render_context, encoder, targets.depth));
        let context = PassContext {
            render_context,
            size,
        };
        for planned in &plan.passes {
            passes[planned.pass].prepare(&context, settings);
        }
        let texture = |target: PassTarget| match target {
            PassTarget::Source => targets.source,
            PassTarget::Intermediate(slot) => pool.texture(intermediates[slot]),
            PassTarget::Output => targets.output,
        };
        for planned in &plan.passes {
            let pass = &mut passes[planned.pass];
            let io = PassIo {
                input: texture(planned.input),
                output: texture(planned.output),
                depth: pass.inputs().depth.then_some(depth).flatten(),
            };
            pass.record(&context, encoder, io);
        }
        plan
    }
}
