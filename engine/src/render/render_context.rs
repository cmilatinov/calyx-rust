use std::sync::Arc;

use egui_wgpu::wgpu;

use utils::singleton_with_init;

#[derive(Default)]
pub struct RenderContext {
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
    texture_format: Option<wgpu::TextureFormat>,
}

singleton_with_init!(RenderContext);

impl RenderContext {
    pub fn init_from_eframe(&mut self, cc: &eframe::CreationContext) {
        let render_state = cc.wgpu_render_state.as_ref().unwrap();
        self.device = Some(render_state.device.clone());
        self.queue = Some(render_state.queue.clone());
        self.texture_format = Some(render_state.target_format);
    }
    pub fn destroy(&mut self) {
        self.device = None;
        self.queue = None;
    }
    pub fn device() -> Option<Arc<wgpu::Device>> {
        RenderContext::get().device.clone()
    }
    pub fn queue() -> Option<Arc<wgpu::Queue>> {
        RenderContext::get().queue.clone()
    }
    pub fn texture_format() -> Option<wgpu::TextureFormat> {
        RenderContext::get().texture_format
    }
}
