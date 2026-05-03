pub use asset::*;
pub use asset_registry::*;
pub use loaded_asset::*;

/// Skeletal animation clip assets.
pub mod animation;
/// Animation graph assets and blend-tree primitives.
pub mod animation_graph;
mod asset;
mod asset_registry;
/// Asset loading and type errors.
pub mod error;
mod loaded_asset;
/// Material assets and shader variable bindings.
pub mod material;
/// Mesh assets and GPU upload helpers.
pub mod mesh;
/// HDR skybox assets and environment-map generation.
pub mod skybox;
/// Texture assets and mip-generation helpers.
pub mod texture;

#[cfg(test)]
mod tests;
