use super::*;
use crate::test_support::TestAssets;
use egui_wgpu::wgpu::util::DeviceExt;

#[test]
fn creates_camera_bind_group_from_grid_shader_layout() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let device = read_only_context.render_context.device();
    let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("test_camera_uniform_buffer"),
        contents: &[0; 272],
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let mut renderer = GridRenderer::new(&read_only_context, &camera_uniform_buffer)
        .expect("grid renderer should load its shader");

    assert!(renderer.camera_bind_group().is_some());
}
