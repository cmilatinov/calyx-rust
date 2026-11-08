//! Paints a scene's HUD offscreen for a game capture at a requested size, so
//! the capture shows the HUD as the Game tab paints it over the game view.

use calyx_engine::render::RenderContext;
use calyx_engine::scene::{Scene, SceneHud};
use calyx_engine::ui::UiInput;
use egui_wgpu::wgpu;
use std::sync::mpsc;

/// Egui paints into an 8-bit target that is not sRGB-aware, as it paints the
/// editor window, so the layer holds gamma-space colors with premultiplied
/// alpha.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const BYTES_PER_PIXEL: u32 = 4;

/// Paints the HUD with its own egui context, renderer and HUD state, so a
/// capture leaves the Game tab's HUD and the editor's egui pass untouched.
#[derive(Default)]
pub struct HudCapture {
    ctx: egui::Context,
    hud: SceneHud,
    renderer: Option<egui_wgpu::Renderer>,
}

impl HudCapture {
    /// Paints `scene`'s HUD over a game view of `size` physical pixels at
    /// `pixels_per_point`, as the Game tab would at that size, and reads it
    /// back. Returns `None` when the scene paints no HUD.
    pub fn paint(
        &mut self,
        render_context: &RenderContext,
        scene: &Scene,
        (width, height): (u32, u32),
        pixels_per_point: f32,
    ) -> Result<Option<image::RgbaImage>, String> {
        let Self { ctx, hud, renderer } = self;
        let device = render_context.device();
        let queue = render_context.queue();
        let renderer = renderer
            .get_or_insert_with(|| egui_wgpu::Renderer::new(device, FORMAT, None, 1, false));

        let rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32, height as f32) / pixels_per_point,
        );
        let mut raw_input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        raw_input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(pixels_per_point);

        // The HUD's fonts are added during a pass and only take effect on the
        // next one, so the second pass is the one painted.
        let mut output = None;
        for _ in 0..2 {
            let pass = ctx.run(raw_input.clone(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                hud.paint(scene, &painter, rect, UiInput::default());
            });
            for (id, delta) in &pass.textures_delta.set {
                renderer.update_texture(device, queue, *id, delta);
            }
            if let Some(previous) = output.replace(pass) {
                free_textures(renderer, &previous);
            }
        }
        let output = output.expect("two passes ran");
        let primitives = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
        let image = if primitives.is_empty() {
            None
        } else {
            Some(render(
                device,
                queue,
                renderer,
                &primitives,
                (width, height),
                output.pixels_per_point,
            )?)
        };
        free_textures(renderer, &output);
        Ok(image)
    }
}

fn free_textures(renderer: &mut egui_wgpu::Renderer, output: &egui::FullOutput) {
    for id in &output.textures_delta.free {
        renderer.free_texture(id);
    }
}

/// Renders `primitives` into a transparent target and reads it back,
/// blocking until the GPU is done.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut egui_wgpu::Renderer,
    primitives: &[egui::ClippedPrimitive],
    (width, height): (u32, u32),
    pixels_per_point: f32,
) -> Result<image::RgbaImage, String> {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hud_capture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hud_capture"),
    });
    let mut commands = renderer.update_buffers(device, queue, &mut encoder, primitives, &screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud_capture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, primitives, &screen);
    }

    let unpadded_bytes_per_row = width * BYTES_PER_PIXEL;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hud_capture_readback"),
        size: padded_bytes_per_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    commands.push(encoder.finish());
    queue.submit(commands);

    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| "HUD readback channel disconnected".to_string())?
        .map_err(|error| format!("HUD readback failed: {error}"))?;
    let mut pixels = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    {
        let data = slice.get_mapped_range();
        for row in data.chunks_exact(padded_bytes_per_row as usize) {
            pixels.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
        }
    }
    buffer.unmap();
    image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| "HUD readback produced a malformed image".to_string())
}

/// Paints `hud`, gamma-space colors with premultiplied alpha, over `view`, as
/// egui blends the HUD over the game image in the window.
pub fn composite_hud(view: &mut image::RgbaImage, hud: &image::RgbaImage) -> Result<(), String> {
    if view.dimensions() != hud.dimensions() {
        return Err(format!(
            "the HUD is {:?} but the game view is {:?}",
            hud.dimensions(),
            view.dimensions()
        ));
    }
    for (pixel, hud) in view.pixels_mut().zip(hud.pixels()) {
        let coverage = 255 - hud[3] as u32;
        for channel in 0..4 {
            let under = (pixel[channel] as u32 * coverage + 127) / 255;
            pixel[channel] = (hud[channel] as u32 + under).min(255) as u8;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::composite_hud;
    use image::{Rgba, RgbaImage};

    #[test]
    fn the_hud_covers_the_game_view_by_its_alpha() {
        let mut view = RgbaImage::from_pixel(3, 1, Rgba([200, 100, 50, 255]));
        let mut hud = RgbaImage::new(3, 1);
        // Clear, opaque white, and half-covering premultiplied black.
        hud.put_pixel(1, 0, Rgba([255, 255, 255, 255]));
        hud.put_pixel(2, 0, Rgba([0, 0, 0, 128]));
        composite_hud(&mut view, &hud).unwrap();
        assert_eq!(view.get_pixel(0, 0), &Rgba([200, 100, 50, 255]));
        assert_eq!(view.get_pixel(1, 0), &Rgba([255, 255, 255, 255]));
        assert_eq!(view.get_pixel(2, 0), &Rgba([100, 50, 25, 255]));
    }

    #[test]
    fn a_hud_of_another_size_is_refused() {
        let mut view = RgbaImage::new(4, 4);
        assert!(composite_hud(&mut view, &RgbaImage::new(2, 2)).is_err());
    }
}
