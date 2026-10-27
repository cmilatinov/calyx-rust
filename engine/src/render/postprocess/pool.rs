//! Intermediate targets shared by the passes of one chain.

use crate::assets::texture::Texture;
use crate::render::RenderContext;
use egui_wgpu::wgpu;
use std::sync::Arc;

/// What makes two intermediate targets interchangeable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetKey {
    /// Width and height in pixels.
    pub size: (u32, u32),
    /// Texel format.
    pub format: wgpu::TextureFormat,
    /// How the target is used.
    pub usage: wgpu::TextureUsages,
}

impl TargetKey {
    /// A color target a pass renders into and the next pass reads.
    pub fn color(size: (u32, u32), format: wgpu::TextureFormat) -> Self {
        Self {
            size,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        }
    }
}

/// Intermediate targets keyed by [`TargetKey`], kept across frames and
/// freed together when the renderer resizes.
#[derive(Default)]
pub struct TargetPool {
    targets: Vec<(TargetKey, Texture)>,
}

impl TargetPool {
    /// Makes sure `count` targets of `key` exist and returns their indices,
    /// for [`Self::texture`]. The same call returns the same targets every
    /// frame.
    pub fn acquire(
        &mut self,
        render_context: &Arc<RenderContext>,
        key: TargetKey,
        count: usize,
    ) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .targets
            .iter()
            .enumerate()
            .filter(|(_, (target_key, _))| *target_key == key)
            .map(|(index, _)| index)
            .take(count)
            .collect();
        while indices.len() < count {
            indices.push(self.targets.len());
            self.targets
                .push((key, Self::create(render_context.clone(), key)));
        }
        indices
    }

    /// The target at `index`, from [`Self::acquire`].
    pub fn texture(&self, index: usize) -> &Texture {
        &self.targets[index].1
    }

    /// How many targets the pool holds.
    pub fn allocated(&self) -> usize {
        self.targets.len()
    }

    /// Frees every target.
    pub fn clear(&mut self) {
        self.targets.clear();
    }

    fn create(render_context: Arc<RenderContext>, key: TargetKey) -> Texture {
        Texture::new(
            render_context,
            &wgpu::TextureDescriptor {
                label: Some("post_process_target"),
                size: wgpu::Extent3d {
                    width: key.size.0,
                    height: key.size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: key.format,
                usage: key.usage,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }
}
