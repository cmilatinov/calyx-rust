//! Runs a one-off compute kernel over storage buffers, for tests of shader
//! modules.

use crate::assets::Asset;
use crate::render::{buffer_entry, read_buffer, AssetShaderLoader, Shader};
use crate::test_support::TestAssets;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;

/// Compiles `source`, which may include the engine's shader modules, and
/// runs its `main` entry point, 64 threads a workgroup, over `outputs`
/// threads. Binding `i` of group 0 holds `inputs[i]`, read-only, and the
/// binding after them a read-write array of `outputs` values of `T`, which
/// it returns.
pub(crate) fn run_kernel<T: bytemuck::Pod>(
    source: &str,
    inputs: &[&[u8]],
    outputs: usize,
) -> Vec<T> {
    let assets = TestAssets::engine();
    let path = assets.engine_root().join("test_kernel.wgsl");
    std::fs::write(&path, source).expect("the test kernel is writable");
    let context = assets.asset_context().lock_read();
    let shader = Shader::from_file(&context, &path)
        .expect("the test kernel compiles")
        .asset;
    let kernel = shader.compute_pipeline("main").expect("main is a kernel");

    let device = context.render_context.device();
    let storage = |contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("test_kernel"),
            contents,
            usage: wgpu::BufferUsages::STORAGE | usage,
        })
    };
    let mut buffers: Vec<wgpu::Buffer> = inputs
        .iter()
        .map(|contents| storage(contents, wgpu::BufferUsages::empty()))
        .collect();
    buffers.push(storage(
        &vec![0u8; outputs * size_of::<T>()],
        wgpu::BufferUsages::COPY_SRC,
    ));
    let entries: Vec<wgpu::BindGroupEntry> = buffers
        .iter()
        .enumerate()
        .map(|(binding, buffer)| buffer_entry(binding as u32, buffer))
        .collect();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("test_kernel"),
        layout: &kernel.bind_group_layouts[0],
        entries: &entries,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&kernel.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((outputs as u32).div_ceil(64), 1, 1);
    }
    context
        .render_context
        .queue()
        .submit(Some(encoder.finish()));
    read_buffer(&context.render_context, buffers.last().expect("the output"))
}

/// Size and member offsets of the struct `name` in `module`.
pub(crate) fn wgsl_layout(module: &naga::Module, name: &str) -> (u32, Vec<(String, u32)>) {
    let (_, ty) = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("the module defines {name}"));
    let naga::TypeInner::Struct { members, span } = &ty.inner else {
        panic!("{name} is a struct");
    };
    let members = members
        .iter()
        .map(|member| (member.name.clone().unwrap_or_default(), member.offset))
        .collect();
    (*span, members)
}

/// `source` preprocessed as a shader of the engine's assets: includes
/// expanded and parameters bound.
pub(crate) fn preprocess(source: &str) -> Result<String, calyx_shader::ShaderError> {
    let assets = TestAssets::engine();
    let path = assets.engine_root().join("test_shader.wgsl");
    std::fs::write(&path, source).expect("the test shader is writable");
    let registries = assets.registries();
    let registry = registries.assets.read();
    Ok(AssetShaderLoader::new(&registry).preprocess(&path)?.source)
}
