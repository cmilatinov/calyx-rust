//! The post-processing chain: full-screen passes from the resolved linear HDR
//! scene color to the presented, display-linear scene texture.
//!
//! See `docs/design/post-processing/architecture.md`.

pub use bloom::*;
pub use chain::*;
pub use color_grading::*;
pub use depth::*;
pub use dof::*;
pub use fxaa::*;
pub use pass::*;
pub use pool::*;
pub use settings::*;
pub use ssao::*;
pub use tonemap::*;

mod bloom;
mod chain;
mod color_grading;
mod depth;
mod dof;
mod fxaa;
mod pass;
mod pool;
mod settings;
mod ssao;
mod tonemap;

#[cfg(test)]
mod tests;
