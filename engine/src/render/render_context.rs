use std::ops::DerefMut;
use std::sync::Arc;

use egui::epaint;
use egui_wgpu::{wgpu, Renderer};

use crate as engine;
use crate::utils::singleton_with_init;

#[derive(Default)]
pub struct RenderContext {
    render_state: Option<egui_wgpu::RenderState>,
    texture_manager: Option<Arc<epaint::mutex::RwLock<epaint::TextureManager>>>,
}

singleton_with_init!(RenderContext);

impl RenderContext {
    pub fn init_from_eframe(&mut self, cc: &eframe::CreationContext) {
        self.render_state = cc.wgpu_render_state.clone();
        self.texture_manager = Some(cc.egui_ctx.tex_manager());
    }
    pub fn destroy(&mut self) {
        self.render_state = None;
        self.texture_manager = None;
    }
    pub fn render_state() -> Option<egui_wgpu::RenderState> {
        RenderContext::get().render_state.clone()
    }
    pub fn device() -> Option<Arc<wgpu::Device>> {
        RenderContext::get()
            .render_state
            .as_ref()
            .map(|state| state.device.clone())
    }
    pub fn queue() -> Option<Arc<wgpu::Queue>> {
        RenderContext::get()
            .render_state
            .as_ref()
            .map(|state| state.queue.clone())
    }
    pub fn renderer() -> Option<Arc<epaint::mutex::RwLock<Renderer>>> {
        RenderContext::get()
            .render_state
            .as_ref()
            .map(|state| state.renderer.clone())
    }
    pub fn target_format() -> Option<wgpu::TextureFormat> {
        RenderContext::get()
            .render_state
            .as_ref()
            .map(|state| state.target_format.clone())
    }
    pub fn texture_manager() -> Option<Arc<epaint::mutex::RwLock<epaint::TextureManager>>> {
        RenderContext::get().texture_manager.clone()
    }
}
