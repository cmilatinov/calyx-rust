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

/// Rendering backend abstraction used by asset loading and scene rendering.
pub struct RenderContext {
    backend: RenderBackend,
}

impl RenderContext {
    /// Wraps the wgpu render state exposed by an eframe creation context.
    pub fn from_eframe(cc: &eframe::CreationContext) -> Self {
        let render_state = cc
            .wgpu_render_state
            .clone()
            .expect("eframe context not using wgpu");
        log::info!(
            "Initialized eframe render context with target format {:?}",
            render_state.target_format
        );
        Self {
            backend: RenderBackend::Eframe {
                render_state,
                texture_manager: cc.egui_ctx.tex_manager(),
            },
        }
    }

    /// Creates a headless render context from an explicit device and queue.
    pub fn headless(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        log::info!("Initialized headless render context");
        Self {
            backend: RenderBackend::Headless { device, queue },
        }
    }

    /// Returns the underlying wgpu device.
    pub fn device(&self) -> &wgpu::Device {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => &render_state.device,
            RenderBackend::Headless { device, .. } => device,
        }
    }

    /// Returns the underlying wgpu queue.
    pub fn queue(&self) -> &wgpu::Queue {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => &render_state.queue,
            RenderBackend::Headless { queue, .. } => queue,
        }
    }

    /// Returns the default color target format for this backend.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => render_state.target_format,
            RenderBackend::Headless { .. } => wgpu::TextureFormat::Bgra8UnormSrgb,
        }
    }

    /// Returns `true` when this context is headless.
    pub fn is_headless(&self) -> bool {
        matches!(&self.backend, RenderBackend::Headless { .. })
    }

    /// Returns the eframe render state.
    pub fn render_state(&self) -> &egui_wgpu::RenderState {
        match &self.backend {
            RenderBackend::Eframe { render_state, .. } => render_state,
            RenderBackend::Headless { .. } => panic!("render_state unavailable in headless mode"),
        }
    }

    /// Returns the shared egui-wgpu renderer.
    pub fn renderer(&self) -> Arc<epaint::mutex::RwLock<Renderer>> {
        self.render_state().renderer.clone()
    }

    /// Returns the egui texture manager.
    pub fn texture_manager(&self) -> Arc<epaint::mutex::RwLock<epaint::TextureManager>> {
        match &self.backend {
            RenderBackend::Eframe {
                texture_manager, ..
            } => texture_manager.clone(),
            RenderBackend::Headless { .. } => {
                panic!("texture_manager unavailable in headless mode")
            }
        }
    }

    /// Returns a [`PipelineOptions`] builder pre-populated for this render
    /// target format.
    pub fn pipeline_options_builder(
        &self,
    ) -> PipelineOptionsBuilder<((), (), (), (Vec<Option<ColorTargetState>>,), (), ())> {
        PipelineOptions::builder()
            .fragment_targets(vec![Some(RenderUtils::color_default(self.target_format()))])
    }
}
