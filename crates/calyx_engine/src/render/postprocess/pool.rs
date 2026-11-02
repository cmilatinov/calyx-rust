//! The two intermediate targets the passes of one chain alternate between.

use crate::assets::texture::Texture;
use crate::render::RenderContext;
use egui_wgpu::wgpu;
use std::sync::Arc;

/// One of the two pool targets the chain alternates between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntermediateSlot {
    /// The first pool target.
    A,
    /// The second pool target.
    B,
}

impl IntermediateSlot {
    /// The slot the pass at `position` in the plan writes: A for even
    /// positions, B for odd ones.
    pub(super) fn written_by(position: usize) -> Self {
        if position % 2 == 0 {
            Self::A
        } else {
            Self::B
        }
    }

    fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }
}

/// At most two intermediate targets, created on first use, kept across
/// frames and freed when the renderer resizes.
#[derive(Default)]
pub struct TargetPool {
    targets: [Option<Texture>; 2],
}

impl TargetPool {
    /// Makes sure the first `count` slots, at most two, hold a `format`
    /// target of `size`, recreating any whose size or format differ.
    pub fn acquire(
        &mut self,
        render_context: &Arc<RenderContext>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        count: usize,
    ) {
        for target in self.targets.iter_mut().take(count) {
            let matches = target.as_ref().is_some_and(|texture| {
                let descriptor = &texture.descriptor;
                (descriptor.size.width, descriptor.size.height) == size
                    && descriptor.format == format
            });
            if !matches {
                *target = Some(Self::create(render_context.clone(), size, format));
            }
        }
    }

    /// The target in `slot`, which [`Self::acquire`] created.
    pub fn texture(&self, slot: IntermediateSlot) -> &Texture {
        self.targets[slot.index()]
            .as_ref()
            .expect("the slot was acquired before it was read")
    }

    /// How many targets the pool holds.
    pub fn allocated(&self) -> usize {
        self.targets.iter().flatten().count()
    }

    /// Frees every target.
    pub fn clear(&mut self) {
        self.targets = Default::default();
    }

    fn create(
        render_context: Arc<RenderContext>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Texture {
        Texture::new(
            render_context,
            &wgpu::TextureDescriptor {
                label: Some("post_process_target"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                // A pass renders into it and the next pass reads it. The
                // chain copies into and out of it for a pass that has no
                // pipeline.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }
}
