use crate::render::{PipelineOptions, PipelineOptionsBuilder, RenderUtils};
use eframe::wgpu::ColorTargetState;
use egui::epaint;
use egui_wgpu::{wgpu, Renderer};
use std::sync::Arc;

enum RenderBackend {
    Eframe {
        render_state: egui_wgpu::RenderState,
        texture_manager: Arc<epaint::mutex::RwLock<epaint::TextureManager>>,
    },
    Headless {
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    },
}

pub struct RenderContext {
    backend: RenderBackend,
}

impl RenderContext {
    pub fn from_eframe(cc: &eframe::CreationContext) -> Self {
        Self {
            backend: RenderBackend::Eframe {
                render_state: cc
                    .wgpu_render_state
                    .clone()
                    .expect("eframe context not using wgpu"),
                texture_manager: cc.egui_ctx.tex_manager(),
            },
        }
    }

    pub fn headless(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self {
            backend: RenderBackend::Headless { device, queue },
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => &render_state.device,
            RenderBackend::Headless { device, .. } => device,
        }
    }

    pub fn queue(&self) -> &wgpu::Queue {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => &render_state.queue,
            RenderBackend::Headless { queue, .. } => queue,
        }
    }

    pub fn target_format(&self) -> wgpu::TextureFormat {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => render_state.target_format,
            RenderBackend::Headless { .. } => wgpu::TextureFormat::Bgra8UnormSrgb,
        }
    }

    pub fn is_headless(&self) -> bool {
        matches!(&self.backend, RenderBackend::Headless { .. })
    }

    pub fn render_state(&self) -> &egui_wgpu::RenderState {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => render_state,
            RenderBackend::Headless { .. } => panic!("render_state unavailable in headless mode"),
        }
    }

    pub fn renderer(&self) -> Arc<epaint::mutex::RwLock<Renderer>> {
        self.render_state().renderer.clone()
    }

    pub fn texture_manager(&self) -> Arc<epaint::mutex::RwLock<epaint::TextureManager>> {
        match &self.backend {
            RenderBackend::Eframe { texture_manager, .. } => texture_manager.clone(),
            RenderBackend::Headless { .. } => {
                panic!("texture_manager unavailable in headless mode")
            }
        }
    }

    pub fn pipeline_options_builder(
        &self,
    ) -> PipelineOptionsBuilder<((), (), (), (Vec<Option<ColorTargetState>>,), (), ())> {
        PipelineOptions::builder()
            .fragment_targets(vec![Some(RenderUtils::color_default(self.target_format()))])
    }
}
