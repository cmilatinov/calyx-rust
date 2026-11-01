//! The colour grading pass: white balance, contrast and saturation, then a
//! vignette, on display-linear color after tone mapping.

use super::{
    NoPipeline, PassContext, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage,
};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;

/// Linear sRGB to LMS (CAT02).
const LINEAR_TO_LMS: [[f32; 3]; 3] = [
    [3.90405e-1, 5.49941e-1, 8.92632e-3],
    [7.08416e-2, 9.63172e-1, 1.35775e-3],
    [2.31082e-2, 1.28021e-1, 9.36245e-1],
];

/// LMS (CAT02) to linear sRGB.
const LMS_TO_LINEAR: [[f32; 3]; 3] = [
    [2.85847e+0, -1.62879e+0, -2.48910e-2],
    [-2.10182e-1, 1.15820e+0, 3.24281e-4],
    [-4.18120e-2, -1.18169e-1, 1.06867e+0],
];

/// Von Kries gains in LMS that move the white point of an illuminant shifted
/// from D65 by `temperature` and `tint` back to D65.
fn lms_white_balance_gains(temperature: f32, tint: f32) -> [f32; 3] {
    let t1 = temperature * 100.0 / 60.0;
    let t2 = tint * 100.0 / 60.0;
    // The illuminant's chromaticity: x along the daylight locus, y from the
    // locus's approximation, offset by tint.
    let x = 0.31271 - t1 * if t1 < 0.0 { 0.1 } else { 0.05 };
    let y = 2.87 * x - 3.0 * x * x - 0.275_095_07 + t2 * 0.05;
    let (cie_x, cie_y, cie_z) = (x / y, 1.0, (1.0 - x - y) / y);
    let l = 0.7328 * cie_x + 0.4296 * cie_y - 0.1624 * cie_z;
    let m = -0.7036 * cie_x + 1.6975 * cie_y + 0.0061 * cie_z;
    let s = 0.0030 * cie_x + 0.0136 * cie_y + 0.9834 * cie_z;
    [0.949237 / l, 1.03542 / m, 1.08728 / s]
}

/// The linear-sRGB white balance matrix for `temperature` and `tint`, row
/// major. Exactly the identity at 0 and 0.
pub(crate) fn white_balance(temperature: f32, tint: f32) -> [[f32; 3]; 3] {
    if temperature == 0.0 && tint == 0.0 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    // Relative to the neutral gains, so the D65 approximation's own error
    // does not tint a neutral grade.
    let neutral = lms_white_balance_gains(0.0, 0.0);
    let gains = lms_white_balance_gains(temperature, tint);
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..3)
                .map(|k| LMS_TO_LINEAR[row][k] * (gains[k] / neutral[k]) * LINEAR_TO_LMS[k][column])
                .sum()
        })
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct ColorGradingUniform {
    /// Column major, each column padded to four floats like WGSL's `mat3x3f`.
    white_balance: [[f32; 4]; 3],
    contrast: f32,
    saturation: f32,
    vignette_intensity: f32,
    vignette_smoothness: f32,
    /// Display-linear.
    vignette_color: [f32; 4],
}

impl ColorGradingUniform {
    fn new(settings: &PostProcessSettings) -> Self {
        // A missing effect reaches the shader as neutral values.
        let grading = settings.color_grading.unwrap_or_default();
        let matrix = white_balance(grading.temperature, grading.tint);
        let vignette = settings.vignette.unwrap_or_default();
        Self {
            white_balance: std::array::from_fn(|column| {
                [matrix[0][column], matrix[1][column], matrix[2][column], 0.0]
            }),
            contrast: grading.contrast,
            saturation: grading.saturation,
            vignette_intensity: if settings.vignette.is_some() {
                vignette.intensity
            } else {
                0.0
            },
            vignette_smoothness: vignette.smoothness.max(0.01),
            vignette_color: egui::Rgba::from(vignette.color).to_array(),
        }
    }
}

/// Colour grading and the vignette on display-linear color, in the `Ldr`
/// stage. Alpha passes through. It runs while either is enabled.
pub struct ColorGradingPass {
    shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
}

impl ColorGradingPass {
    /// Shader asset the pass draws with.
    pub const SHADER: &'static str = "shaders/postprocess/color_grading";

    /// Creates the pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, Self::SHADER)?;
        let uniform_buffer =
            context
                .render_context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("color_grading_uniform_buffer"),
                    size: size_of::<ColorGradingUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        Ok(Self {
            shader,
            uniform_buffer,
            bind_group: Default::default(),
        })
    }
}

impl PostProcessPass for ColorGradingPass {
    fn name(&self) -> &str {
        "color_grading"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Ldr
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.color_grading.is_some() || settings.vignette.is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        context.render_context.queue().write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&ColorGradingUniform::new(settings)),
        );
    }

    fn record(
        &mut self,
        context: &PassContext,
        encoder: &mut wgpu::CommandEncoder,
        io: PassIo,
    ) -> Result<(), NoPipeline> {
        let shader = self.shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(io.output.descriptor.format))
                .build(),
        ) else {
            return Err(NoPipeline::of(&shader));
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let device = context.render_context.device();
        let uniform_buffer = &self.uniform_buffer;
        let bind_group =
            self.bind_group
                .get_or_create((layout.clone(), io.input.view.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("color_grading_bind_group"),
                        layout: &layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&io.input.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: uniform_buffer.as_entire_binding(),
                            },
                        ],
                    })
                });
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &io.output.view, "Color Grading");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
