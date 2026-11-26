use super::*;
use crate::test_support::test_render_context;

fn uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: false,
    })
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

#[test]
fn rebuilds_only_when_a_bound_resource_changes() {
    let render_context = test_render_context();
    let device = render_context.device();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let first_buffer = uniform_buffer(device);
    let second_buffer = uniform_buffer(device);
    let mut cache = CachedBindGroup::default();
    let builds = std::cell::Cell::new(0);
    let get = |cache: &mut CachedBindGroup<_>, buffer: &wgpu::Buffer| {
        cache
            .get_or_create((layout.clone(), buffer.clone()), || {
                builds.set(builds.get() + 1);
                bind_group(device, &layout, buffer)
            })
            .clone()
    };

    let first = get(&mut cache, &first_buffer);
    assert_eq!(get(&mut cache, &first_buffer), first);
    let second = get(&mut cache, &second_buffer);
    assert_ne!(second, first);
    cache.clear();
    assert_ne!(get(&mut cache, &second_buffer), second);
    assert_eq!(builds.get(), 3);
}
