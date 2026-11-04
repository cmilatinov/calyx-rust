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

/// Marks a component as a post-process effect: a component that sits beside
/// a volume and that a post-processing profile can hold an entry for.
///
/// Each effect component implements it and lists it in its `#[reflect(...)]`
/// attribute, so the type registry knows the effect types
/// ([`PostProcessProfile::effect_types`](crate::assets::post_process_profile::PostProcessProfile::effect_types))
/// and a profile can turn a reflected value back into an effect.
#[reflect_trait]
pub trait PostProcessEffect: Component {}
