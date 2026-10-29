//! The post-processing chain: full-screen passes from the resolved linear HDR
//! scene color to the presented, display-linear scene texture.
//!
//! See `docs/design/post-processing/architecture.md`.

pub use chain::*;
pub use depth::*;
pub use pass::*;
pub use pool::*;
pub use settings::*;
pub use ssao::*;
pub use tonemap::*;

mod chain;
mod depth;
mod pass;
mod pool;
mod settings;
mod ssao;
mod tonemap;

#[cfg(test)]
mod tests;
