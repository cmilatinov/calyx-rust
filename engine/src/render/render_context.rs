use std::sync::Arc;

use crate::render::{PipelineOptionsBuilder, RenderUtils};
use egui::epaint;
use egui_wgpu::{wgpu, Renderer};

pub struct RenderContext {
    render_state: egui_wgpu::RenderState,
    texture_manager: Arc<epaint::mutex::RwLock<epaint::TextureManager>>,
}

impl RenderContext {
    pub fn from_eframe(cc: &eframe::CreationContext) -> Self {
        Self {
            render_state: cc
                .wgpu_render_state
                .clone()
                .expect("eframe context not using wgpu"),
            texture_manager: cc.egui_ctx.tex_manager(),
        }
    }

    pub fn render_state(&self) -> &egui_wgpu::RenderState {
        &self.render_state
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.render_state.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.render_state.queue
    }

    pub fn renderer(&self) -> Arc<epaint::mutex::RwLock<Renderer>> {
        self.render_state.renderer.clone()
    }

    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.render_state.target_format
    }

    pub fn texture_manager(&self) -> Arc<epaint::mutex::RwLock<epaint::TextureManager>> {
        self.texture_manager.clone()
    }

    pub fn pipeline_options_builder(&self) -> PipelineOptionsBuilder {
        PipelineOptionsBuilder::default()
            .fragment_targets(vec![Some(RenderUtils::color_default(self.target_format()))])
    }
}
