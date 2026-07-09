use super::*;

pub(super) struct ThumbnailCache {
    root: PathBuf,
    size_px: u32,
}

impl ThumbnailCache {
    fn project_root(context: &ReadOnlyAssetContext) -> PathBuf {
        let asset_root = context.registries.assets.read().root_path().clone();
        let project_hash = project_cache_hash(&asset_root);
        dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Calyx")
            .join("Editor")
            .join("thumbnails")
            .join(project_hash)
    }

    pub(super) fn new(
        context: &ReadOnlyAssetContext,
        render_settings: ThumbnailRenderSettings,
    ) -> Self {
        let render_settings = render_settings.sanitized();
        let cache_root = Self::project_root(context)
            .join(format!("{}px", render_settings.size_px))
            .join(render_settings.cache_key());

        Self {
            root: cache_root,
            size_px: render_settings.size_px,
        }
    }

    pub(super) fn invalidate_project(context: &ReadOnlyAssetContext) -> Result<(), String> {
        let root = Self::project_root(context);
        if !root.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&root).map_err(|err| {
            format!(
                "failed to remove thumbnail cache directory {}: {err}",
                root.display()
            )
        })
    }

    pub(super) fn load(
        &self,
        context: &ReadOnlyAssetContext,
        request: &ThumbnailRequest,
    ) -> Result<Option<Texture>, String> {
        let path = self.path(request);
        if !path.is_file() {
            return Ok(None);
        }

        let image = image::open(&path)
            .map_err(|err| {
                format!(
                    "failed to decode cached thumbnail {}: {err}",
                    path.display()
                )
            })?
            .to_rgba8();
        if image.width() != self.size_px || image.height() != self.size_px {
            return Err(format!(
                "cached thumbnail {} has size {}x{}, expected {}x{}",
                path.display(),
                image.width(),
                image.height(),
                self.size_px,
                self.size_px
            ));
        }

        Ok(Some(texture_from_rgba8(
            context,
            "thumbnail_disk_cache",
            &image,
        )))
    }

    pub(super) fn store(
        &self,
        render_state: &RenderState,
        request: &ThumbnailRequest,
        texture: &Texture,
    ) -> Result<(), String> {
        let pixels = read_texture_rgba8(render_state, texture, self.size_px)?;
        let image: RgbaImage = ImageBuffer::from_vec(self.size_px, self.size_px, pixels)
            .ok_or_else(|| "thumbnail readback returned an unexpected byte count".to_string())?;
        let path = self.path(request);
        let parent = path
            .parent()
            .ok_or_else(|| format!("thumbnail cache path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create thumbnail cache directory {}: {err}",
                parent.display()
            )
        })?;

        let tmp_path = path.with_extension("png.tmp");
        if tmp_path.exists() {
            fs::remove_file(&tmp_path).map_err(|err| {
                format!(
                    "failed to remove stale thumbnail cache temp file {}: {err}",
                    tmp_path.display()
                )
            })?;
        }

        image
            .save_with_format(&tmp_path, ImageFormat::Png)
            .map_err(|err| {
                format!(
                    "failed to encode thumbnail cache file {}: {err}",
                    tmp_path.display()
                )
            })?;

        match fs::rename(&tmp_path, &path) {
            Ok(()) => Ok(()),
            Err(rename_error) if path.exists() => {
                fs::remove_file(&path).map_err(|err| {
                    format!(
                        "failed to replace thumbnail cache file {} after rename error {rename_error}: {err}",
                        path.display()
                    )
                })?;
                fs::rename(&tmp_path, &path).map_err(|err| {
                    format!(
                        "failed to move thumbnail cache file {} to {}: {err}",
                        tmp_path.display(),
                        path.display()
                    )
                })
            }
            Err(err) => Err(format!(
                "failed to move thumbnail cache file {} to {}: {err}",
                tmp_path.display(),
                path.display()
            )),
        }
    }

    pub(super) fn path(&self, request: &ThumbnailRequest) -> PathBuf {
        self.root
            .join(thumbnail_asset_type_name(request.asset_type))
            .join(format!(
                "{}-{:016x}.png",
                request.asset_id, request.source_version
            ))
    }
}

fn project_cache_hash(asset_root: &Path) -> String {
    let canonical = fs::canonicalize(asset_root).unwrap_or_else(|_| asset_root.to_path_buf());
    let normalized = canonical.to_string_lossy().replace('\\', "/");
    let mut hasher = Sha1::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn texture_from_image_file(
    context: &ReadOnlyAssetContext,
    label: &str,
    path: &Path,
) -> Result<Texture, String> {
    let reader = ImageReader::open(path)
        .map_err(|err| format!("failed to open texture source {}: {err}", path.display()))?;
    let image = transform_thumbnail_source_image(
        reader
            .decode()
            .map_err(|err| format!("failed to decode texture source {}: {err}", path.display()))?,
    );
    let texture_depth = image.color().bytes_per_pixel() as u32;
    let texture_format = thumbnail_source_texture_format(image.color());
    let texture_size = wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = Texture::new(
        context.render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        None,
        None,
        false,
    );
    context.render_context.queue().write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        image.as_bytes(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(texture_depth * image.width()),
            rows_per_image: Some(image.height()),
        },
        texture_size,
    );
    Ok(texture)
}

fn transform_thumbnail_source_image(image: DynamicImage) -> DynamicImage {
    match image.color() {
        ColorType::Rgba32F | ColorType::Rgba8 => image,
        ColorType::Rgb32F => image.to_rgba32f().into(),
        _ => image.to_rgba8().into(),
    }
}

fn thumbnail_source_texture_format(color: ColorType) -> wgpu::TextureFormat {
    match color {
        ColorType::Rgba32F => wgpu::TextureFormat::Rgba32Float,
        _ => wgpu::TextureFormat::Rgba8Unorm,
    }
}

fn texture_from_rgba8(context: &ReadOnlyAssetContext, label: &str, image: &RgbaImage) -> Texture {
    let size = wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = Texture::new(
        context.render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        },
        Some(wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        }),
        Some(thumbnail_display_view_descriptor()),
        true,
    );
    context.render_context.queue().write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        image.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width() * 4),
            rows_per_image: Some(image.height()),
        },
        size,
    );
    texture
}

pub(super) fn thumbnail_display_view_descriptor() -> wgpu::TextureViewDescriptor<'static> {
    wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
        ..Default::default()
    }
}

pub(super) fn thumbnail_storage_view(texture: &Texture) -> wgpu::TextureView {
    texture.texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    })
}

fn read_texture_rgba8(
    render_state: &RenderState,
    texture: &Texture,
    expected_size_px: u32,
) -> Result<Vec<u8>, String> {
    if texture.descriptor.format != wgpu::TextureFormat::Rgba8Unorm {
        return Err(format!(
            "thumbnail cache requires Rgba8Unorm textures, got {:?}",
            texture.descriptor.format
        ));
    }
    if texture.descriptor.dimension != wgpu::TextureDimension::D2
        || texture.descriptor.size.depth_or_array_layers != 1
        || texture.descriptor.sample_count != 1
    {
        return Err("thumbnail cache requires a single-sample 2D texture".into());
    }

    let width = texture.descriptor.size.width;
    let height = texture.descriptor.size.height;
    if width != expected_size_px || height != expected_size_px {
        return Err(format!(
            "thumbnail cache requires {}x{} textures, got {}x{}",
            expected_size_px, expected_size_px, width, height
        ));
    }

    let bytes_per_pixel = 4;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let padded_bytes_per_row = align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let output_buffer_size = padded_bytes_per_row as u64 * height as u64;
    let output_buffer = render_state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("thumbnail_cache_readback"),
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = render_state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("thumbnail_cache_readback"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &output_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    render_state.queue.submit(Some(encoder.finish()));

    let slice = output_buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });

    loop {
        let _ = render_state.device.poll(wgpu::Maintain::Poll);
        match rx.try_recv() {
            Ok(Ok(())) => break,
            Ok(Err(err)) => return Err(format!("thumbnail cache readback failed: {err:?}")),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("thumbnail cache readback disconnected".into());
            }
            Err(mpsc::TryRecvError::Empty) => thread::yield_now(),
        }
    }

    let data = slice.get_mapped_range();
    let mut pixels = vec![0; (unpadded_bytes_per_row * height) as usize];
    for row in 0..height as usize {
        let source_offset = row * padded_bytes_per_row as usize;
        let target_offset = row * unpadded_bytes_per_row as usize;
        let source = &data[source_offset..source_offset + unpadded_bytes_per_row as usize];
        let target = &mut pixels[target_offset..target_offset + unpadded_bytes_per_row as usize];
        target.copy_from_slice(source);
    }
    drop(data);
    output_buffer.unmap();

    Ok(pixels)
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}
