use std::{ops::Range, path::Path};

use eframe::wgpu::{self, CommandEncoder};
use egui_wgpu::RenderState;
use engine_derive::TypeUuid;

use super::{error::AssetError, texture::Texture2D, Asset, Assets, LoadedAsset};
use crate::{
    self as engine,
    core::Ref,
    render::{PipelineOptionsBuilder, RenderUtils, Shader},
};

#[derive(TypeUuid)]
#[uuid = "bdb5dd3a-cca4-453d-8260-ff2bdf2a05b2"]
pub struct Skybox {
    pub texture: Texture2D,
    pub cubemap: Texture2D,
    pub irradiance_cubemap: Texture2D,
    pub prefilter_cubemap: Texture2D,
    pub brdf_map: Texture2D,
    pub dirty: bool,
}

pub struct SkyboxShaders<'a> {
    pub cubemap_shader: &'a Ref<Shader>,
    pub irradiance_cubemap_shader: &'a Ref<Shader>,
    pub prefilter_cubemap_shader: &'a Ref<Shader>,
    pub brdf_shader: &'a Ref<Shader>,
}

impl Asset for Skybox {
    fn get_file_extensions() -> &'static [&'static str] {
        &["exr", "hdr"]
    }

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError> {
        let LoadedAsset { asset: texture, .. } = Texture2D::from_file(path)?;
        let cubemap = Texture2D::new(
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1024,
                    height: 1024,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg11b10Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            Some(wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            }),
            false,
        );
        let irradiance_cubemap = Texture2D::new(
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 512,
                    height: 512,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg11b10Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            Some(wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            }),
            false,
        );
        let prefilter_cubemap = Texture2D::new(
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 5,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg11b10Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            Some(wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            }),
            false,
        );
        let brdf_map = Texture2D::new(
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 512,
                    height: 512,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
            None,
            None,
            false,
        );
        Ok(LoadedAsset::new(Self {
            texture,
            cubemap,
            irradiance_cubemap,
            prefilter_cubemap,
            brdf_map,
            dirty: true,
        }))
    }
}

impl Skybox {
    const NUM_ROUGHNESS_VALUES: u32 = 5;

    fn prepare_step(
        &self,
        shader_ref: &Ref<Shader>,
        render_state: &RenderState,
        encoder: &mut CommandEncoder,
        src: Option<&Texture2D>,
        dest: &Texture2D,
        dest_cubemap: bool,
        dest_mip_level: Option<u32>,
        instances: Option<Range<u32>>,
    ) {
        let device = &render_state.device;
        let mut shader = shader_ref.write();
        let quad_binding = Assets::screen_space_quad().unwrap();
        let quad_mesh = quad_binding.read();
        let views;
        let attachment_list;
        if dest_cubemap {
            views = dest.create_cubemap_views(dest_mip_level);
            attachment_list = views
                .iter()
                .map(|v| {
                    Some(wgpu::RenderPassColorAttachment {
                        view: v,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })
                })
                .collect::<Vec<_>>();
        } else {
            attachment_list = [Some(wgpu::RenderPassColorAttachment {
                view: &dest.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })]
            .into();
        };
        let color_attachments = attachment_list.as_slice();
        let targets = color_attachments
            .iter()
            .map(|_| {
                Some(wgpu::ColorTargetState {
                    format: dest.format,
                    blend: None,
                    write_mask: Default::default(),
                })
            })
            .collect::<Vec<_>>();
        let options = &PipelineOptionsBuilder::default()
            .fragment_targets(targets)
            .depth_stencil(None)
            .build();
        let bind_group = src.map(|src| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &shader.bind_group_layouts[0],
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&src.sampler),
                    },
                ],
            })
        });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            shader.build_pipeline(&options);
            if let Some(pipeline) = shader.get_pipeline(&options) {
                render_pass.set_pipeline(pipeline);
                if let Some(bind_group) = bind_group.as_ref() {
                    render_pass.set_bind_group(0, &bind_group, &[]);
                }
                RenderUtils::bind_mesh_buffers(&mut render_pass, &quad_mesh);
                RenderUtils::draw_mesh_instanced(
                    &mut render_pass,
                    &quad_mesh,
                    instances.unwrap_or(0..1),
                );
            }
        }
    }

    pub(crate) fn prepare(
        &mut self,
        shaders: SkyboxShaders,
        render_state: &RenderState,
        encoder: &mut CommandEncoder,
    ) {
        if !self.dirty {
            return;
        }
        self.prepare_step(
            shaders.cubemap_shader,
            render_state,
            encoder,
            Some(&self.texture),
            &self.cubemap,
            true,
            None,
            None,
        );
        self.prepare_step(
            shaders.irradiance_cubemap_shader,
            render_state,
            encoder,
            Some(&self.cubemap),
            &self.irradiance_cubemap,
            true,
            None,
            None,
        );
        for i in 0..Self::NUM_ROUGHNESS_VALUES {
            self.prepare_step(
                shaders.prefilter_cubemap_shader,
                render_state,
                encoder,
                Some(&self.cubemap),
                &self.prefilter_cubemap,
                true,
                Some(i as u32),
                Some(i..i + 1),
            );
        }
        self.prepare_step(
            shaders.brdf_shader,
            render_state,
            encoder,
            None,
            &self.brdf_map,
            false,
            None,
            None,
        );
        self.dirty = false;
    }
}
