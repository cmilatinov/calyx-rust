//! The interface every editor overlay implements.

use crate::assets::texture::Texture;
use crate::render::{RenderContext, SceneRendererOptions};
use egui_wgpu::wgpu;
use std::sync::Arc;

/// What an overlay may use while preparing and recording.
pub struct OverlayContext<'a> {
    /// The render context the overlays render on.
    pub render_context: &'a Arc<RenderContext>,
    /// The options of the view the overlays draw over, which say which
    /// overlays it shows.
    pub options: &'a SceneRendererOptions,
    /// Format of the overlay target every overlay draws into.
    pub format: wgpu::TextureFormat,
    /// Sample count of the overlay target and of the scene depth it tests
    /// against.
    pub samples: u32,
    /// The scene's single-sample object-id texture.
    pub object_ids: &'a Texture,
}

/// One editor overlay: something drawn over the post-processed scene that
/// post-processing must never change, such as the grid, gizmos and the
/// selection outline.
///
/// Every enabled overlay records into one shared render pass over the
/// overlay target, cleared to transparent, and the scene's depth, both at the
/// scene's sample count. An overlay blends into it with
/// [`crate::render::RenderUtils::color_alpha_blending`], so the target holds
/// premultiplied color with coverage alpha, and its pipelines carry a
/// `Depth32Float` depth state, since the pass has a depth attachment.
pub trait OverlayPass {
    /// Name used in logs and errors.
    fn name(&self) -> &str;

    /// Whether the overlay draws this frame.
    fn enabled(&self, context: &OverlayContext) -> bool;

    /// Uploads the overlay's uniforms and builds its bind groups. Runs
    /// before any overlay records.
    fn prepare(&mut self, context: &OverlayContext);

    /// Records the overlay's draws into the shared overlay pass.
    fn record(&mut self, context: &OverlayContext, render_pass: &mut wgpu::RenderPass<'_>);
}
