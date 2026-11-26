use super::super::Shader;
use crate::assets::Asset;
use crate::test_support::TestAssets;

/// Two kernels that use five storage buffers each, nine between them:
/// within the per-stage limit of eight for each kernel, over it for the
/// shader as a whole.
const KERNELS: &str = r#"
@group(0) @binding(0) var<storage, read_write> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read> c: array<f32>;
@group(0) @binding(3) var<storage, read> d: array<f32>;
@group(0) @binding(4) var<storage, read> e: array<f32>;
@group(0) @binding(5) var<storage, read> f: array<f32>;
@group(0) @binding(6) var<storage, read> g: array<f32>;
@group(0) @binding(7) var<storage, read> h: array<f32>;
@group(0) @binding(8) var<storage, read> i: array<f32>;

@compute @workgroup_size(1)
fn first(@builtin(global_invocation_id) gid: vec3u) {
    a[gid.x] = b[gid.x] + c[gid.x] + d[gid.x] + e[gid.x];
}

@compute @workgroup_size(1)
fn second(@builtin(global_invocation_id) gid: vec3u) {
    a[gid.x] = f[gid.x] + g[gid.x] + h[gid.x] + i[gid.x];
}
"#;

#[test]
fn kernels_within_the_storage_limit_load_whatever_the_shader_declares() {
    let assets = TestAssets::engine();
    let path = assets.engine_root().join("nine_buffers.wgsl");
    std::fs::write(&path, KERNELS).expect("the shader is writable");
    let context = assets.asset_context().lock_read();
    let shader = Shader::from_file(&context, &path)
        .expect("the shader loads")
        .asset;
    for kernel in ["first", "second"] {
        let kernel = shader.compute_pipeline(kernel).expect("a kernel");
        assert_eq!(kernel.bind_group_layouts.len(), 1);
    }
    assert!(
        shader.bind_group_layouts.is_empty(),
        "a shader of several kernels keeps no layout of its own"
    );
}
