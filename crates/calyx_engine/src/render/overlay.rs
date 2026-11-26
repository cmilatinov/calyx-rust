//! Editor overlays: the grid, gizmos and selection outline, drawn after the
//! post-processing chain and composited over its output, so post-processing
//! never changes their colors.
//!
//! See `docs/design/post-processing/architecture.md`.

pub use compositor::*;
pub use pass::*;

mod compositor;
mod pass;

#[cfg(test)]
mod tests;
