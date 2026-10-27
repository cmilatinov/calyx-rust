//! Single-sample scene depth for passes that read it.

use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderContext, SceneRendererError, Shader};
use egui_wgpu::wgpu;
use std::sync::Arc;

/// Copies sample 0 of a multisampled depth texture into a single-sample one.
pub struct DepthResolve {
    shader: Ref<Shader>,
    target: Option<Texture>,
    bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
}

impl DepthResolve {
    /// Shader asset the copy draws with.
    pub const SHADER: &'static str = "shaders/postprocess/depth_resolve";

    /// Creates the copy pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        Ok(Self {
            shader: SceneRendererError::load::<Shader>(context, Self::SHADER)?,
            target: None,
            bind_group: Default::default(),
        })
    }

    /// Single-sample depth for `depth`: `depth` itself at one sample, else
    /// its sample 0, copied by a pass recorded into `encoder`.
    pub fn resolve<'a>(
        &'a mut self,
        render_context: &Arc<RenderContext>,
        encoder: &mut wgpu::CommandEncoder,
        depth: &'a Texture,
    ) -> &'a Texture {
        if depth.descriptor.sample_count <= 1 {
            return depth;
        }
        let size = depth.descriptor.size;
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.descriptor.size != size)
        {
            self.bind_group.clear();
            self.target = Some(Texture::new(
                render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some("post_process_depth"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Depth32Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                },
                None,
                None,
                false,
            ));
        }
        let target = self.target.as_ref().expect("the target was just created");

        let shader = self.shader.read();
        let pipeline = shader.pipeline(
            &PipelineOptions::builder()
                .cull_mode(None)
                .depth_stencil(Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: Default::default(),
                    bias: Default::default(),
                }))
                .vertex_layouts(vec![])
                .build(),
        );
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let device = render_context.device();
        let bind_group =
            self.bind_group
                .get_or_create((layout.clone(), depth.view.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("post_process_depth_bind_group"),
                        layout: &layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&depth.view),
                        }],
                    })
                });
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Post Process Depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        drop(render_pass);
        target
    }

    /// Frees the single-sample copy.
    pub fn clear(&mut self) {
        self.target = None;
        self.bind_group.clear();
    }
}
