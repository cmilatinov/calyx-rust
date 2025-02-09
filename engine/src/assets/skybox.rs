use eframe::wgpu::{self, CommandEncoder};
use egui_wgpu::RenderState;
use engine_derive::TypeUuid;
use std::path::Path;
use std::sync::RwLockReadGuard;

use super::{error::AssetError, texture::Texture, Asset, LoadedAsset};
use crate::{self as engine, core::Ref, render::Shader};

#[derive(TypeUuid)]
#[uuid = "bdb5dd3a-cca4-453d-8260-ff2bdf2a05b2"]
pub struct Skybox {
    pub texture: Texture,
    pub cubemap: Texture,
    pub irradiance_cubemap: Texture,
    pub prefilter_cubemap: Texture,
    pub brdf_map: Texture,
    pub dirty: bool,
}

pub struct SkyboxShaders<'a> {
    pub cubemap_shader: &'a Ref<Shader>,
    pub irradiance_cubemap_shader: &'a Ref<Shader>,
    pub prefilter_cubemap_shader: &'a Ref<Shader>,
    pub brdf_shader: &'a Ref<Shader>,
}

impl Asset for Skybox {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Skybox"
    }

    fn file_extensions() -> &'static [&'static str] {
        &["exr", "hdr"]
    }

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError> {
        let LoadedAsset { asset: texture, .. } = Texture::from_file(path)?;
        let cubemap = Texture::new(
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1024,
                    height: 1024,
                    depth_or_array_layers: 6,
                },
                mip_level_count: Self::NUM_ROUGHNESS_VALUES,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            Some(wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            }),
            false,
        );
        let irradiance_cubemap = Texture::new(
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
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            Some(wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            }),
            false,
        );
        let prefilter_cubemap = Texture::new(
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 6,
                },
                mip_level_count: Self::NUM_ROUGHNESS_VALUES,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            Some(wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            }),
            false,
        );
        let brdf_map = Texture::new(
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
                format: wgpu::TextureFormat::Rg32Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
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
    const WORKGROUP_SIZE: f32 = 8.0;
    const NUM_ROUGHNESS_VALUES: u32 = 5;

    fn num_workgroups_xy(texture_size: u32) -> u32 {
        (texture_size as f32 / Self::WORKGROUP_SIZE).ceil() as u32
    }

    fn create_src_dst_bind_group(
        device: &wgpu::Device,
        shader: &Shader,
        src: &Texture,
        dst: &Texture,
    ) -> wgpu::BindGroup {
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &dst.create_cubemap_array_view(None),
                    ),
                },
            ],
        })
    }

    fn create_src_dst_array_bind_group(
        device: &wgpu::Device,
        shader: &Shader,
        src: &Texture,
        dst: &Texture,
        dst_mips: u32,
    ) -> wgpu::BindGroup {
        let views = (0..dst_mips)
            .into_iter()
            .map(|mip| dst.create_cubemap_array_view(Some(mip)))
            .collect::<Vec<_>>();
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureViewArray(
                        views.iter().map(|v| v).collect::<Vec<_>>().as_slice(),
                    ),
                },
            ],
        })
    }

    fn create_dst_bind_group(
        device: &wgpu::Device,
        shader: &Shader,
        dst: &Texture,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &shader.bind_group_layouts[0],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&dst.view),
            }],
        })
    }

    fn prepare_step(
        &self,
        shader: &RwLockReadGuard<Shader>,
        encoder: &mut CommandEncoder,
        bind_group: &wgpu::BindGroup,
        num_workgroups_xy: u32,
        num_workgroups_z: Option<u32>,
    ) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        if let Some(pipeline) = shader.get_compute_pipeline() {
            compute_pass.set_pipeline(pipeline);
            compute_pass.set_bind_group(0, bind_group, &[]);
            compute_pass.dispatch_workgroups(
                num_workgroups_xy,
                num_workgroups_xy,
                num_workgroups_z.unwrap_or(6),
            );
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
        {
            let shader = shaders.cubemap_shader.read();
            let bind_group = Self::create_src_dst_bind_group(
                &render_state.device,
                &shader,
                &self.texture,
                &self.cubemap,
            );
            self.prepare_step(
                &shader,
                encoder,
                &bind_group,
                Self::num_workgroups_xy(self.cubemap.descriptor.size.width),
                None,
            );
        }
        self.cubemap.generate_cubemap_mips(render_state, encoder);
        {
            let shader = shaders.irradiance_cubemap_shader.read();
            let bind_group = Self::create_src_dst_bind_group(
                &render_state.device,
                &shader,
                &self.cubemap,
                &self.irradiance_cubemap,
            );
            self.prepare_step(
                &shader,
                encoder,
                &bind_group,
                Self::num_workgroups_xy(self.irradiance_cubemap.descriptor.size.width),
                None,
            );
        }
        {
            let shader = shaders.prefilter_cubemap_shader.read();
            let bind_group = Self::create_src_dst_array_bind_group(
                &render_state.device,
                &shader,
                &self.cubemap,
                &self.prefilter_cubemap,
                self.prefilter_cubemap.descriptor.mip_level_count,
            );
            self.prepare_step(
                &shader,
                encoder,
                &bind_group,
                Self::num_workgroups_xy(self.prefilter_cubemap.descriptor.size.width),
                Some(Self::NUM_ROUGHNESS_VALUES * 6),
            );
        }
        {
            let shader = shaders.brdf_shader.read();
            let bind_group =
                Self::create_dst_bind_group(&render_state.device, &shader, &self.brdf_map);
            self.prepare_step(
                &shader,
                encoder,
                &bind_group,
                Self::num_workgroups_xy(self.brdf_map.descriptor.size.width),
                Some(1),
            );
        }
        self.dirty = false;
        // self.prepare_step(
        //     shaders.cubemap_shader,
        //     render_state,
        //     encoder,
        //     Some(&self.texture),
        //     &self.cubemap,
        //     true,
        //     None,
        //     None,
        // );
        // self.cubemap.generate_mips(render_state, encoder);
        // self.prepare_step(
        //     shaders.irradiance_cubemap_shader,
        //     render_state,
        //     encoder,
        //     Some(&self.cubemap),
        //     &self.irradiance_cubemap,
        //     true,
        //     None,
        //     None,
        // );
        // for i in 0..Self::NUM_ROUGHNESS_VALUES {
        //     self.prepare_step(
        //         shaders.prefilter_cubemap_shader,
        //         render_state,
        //         encoder,
        //         Some(&self.cubemap),
        //         &self.prefilter_cubemap,
        //         true,
        //         Some(i as u32),
        //         Some(i..i + 1),
        //     );
        // }
        // self.prepare_step(
        //     shaders.brdf_shader,
        //     render_state,
        //     encoder,
        //     None,
        //     &self.brdf_map,
        //     false,
        //     None,
        //     None,
        // );
        // self.dirty = false;
    }
}
