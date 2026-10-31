//! The post-process effect components.
//!
//! Each effect is its own component on the game object of a
//! [`ComponentGlobalPostProcessVolume`](super::ComponentGlobalPostProcessVolume).
//! [`PostProcessSettings::resolve`](crate::render::postprocess::PostProcessSettings::resolve)
//! gathers the enabled ones beside the selected volume, and the passes in
//! `render/postprocess/` read them from there.

pub use ambient_occlusion::*;
pub use anti_aliasing::*;
pub use bloom::*;
pub use color_grading::*;
pub use depth_of_field::*;
pub use tonemap::*;
pub use vignette::*;

mod ambient_occlusion;
mod anti_aliasing;
mod bloom;
mod color_grading;
mod depth_of_field;
mod tonemap;
mod vignette;
