//! The post-process effect components.
//!
//! Each effect is its own component on the game object of a
//! [`ComponentGlobalPostProcessVolume`](super::ComponentGlobalPostProcessVolume).
//! [`PostProcessSettings::resolve`](crate::render::postprocess::PostProcessSettings::resolve)
//! gathers the enabled ones beside the selected volume, and the passes in
//! `render/postprocess/` read them from there.

use crate as calyx_engine;
use crate::component::Component;
use calyx_engine_derive::reflect_trait;
pub use calyx_engine_derive::PostProcessEffect;

pub use ambient_occlusion::*;
pub use anti_aliasing::*;
pub use bloom::*;
pub use color_grading::*;
pub use depth_of_field::*;
pub use fog::*;
pub use outline::*;
pub use tonemap::*;
pub use vignette::*;

mod ambient_occlusion;
mod anti_aliasing;
mod bloom;
mod color_grading;
mod depth_of_field;
mod fog;
mod outline;
mod tonemap;
mod vignette;

/// A post-process effect's settings, with in-place copy and blend operations.
/// The component sits beside a volume and can be held in a profile entry.
///
/// Each effect component implements it and lists it in its `#[reflect(...)]`
/// attribute, so the type registry knows the effect types
/// ([`PostProcessProfile::effect_types`](crate::assets::post_process_profile::PostProcessProfile::effect_types))
/// and a profile can turn a reflected value back into an effect.
/// Derive it with `#[derive(PostProcessEffect)]` on a component that implements
/// [`BlendEffect`](crate::render::postprocess::BlendEffect); the derive dispatches
/// to that implementation's in-place operations.
#[reflect_trait]
pub trait PostProcessEffect: Component {
    /// Copies `source` into existing storage, reusing owned buffers where possible.
    ///
    /// # Panics
    /// Panics when `source` has a different concrete component type.
    fn copy_from(&mut self, source: &dyn PostProcessEffect);

    /// Blends this effect toward `target` in place, clamping `factor` to 0..1.
    /// Uses the same field and discrete-value rules as `BlendEffect`.
    ///
    /// # Panics
    /// Panics when `target` has a different concrete component type.
    fn blend_into(&mut self, target: &dyn PostProcessEffect, factor: f32);

    /// Resets the effect's strengths to neutral, preserving its other settings.
    /// Returns false, leaving it unchanged, when no neutral value exists (FXAA).
    /// A resolver then switches the effect on at half weight instead of fading it.
    fn reset_neutral(&mut self) -> bool;
}

#[cfg(test)]
mod tests;
