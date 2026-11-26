use super::super::Shader;
use crate::render::{PipelineOptions, RenderUtils};
use crate::test_support::TestAssets;
use egui_wgpu::wgpu;
use notify::{Event, EventKind};
use std::path::Path;
use std::time::Duration;

const NAME: &str = "reload_probe";

/// A full-screen shader with one uniform; `color` is its fragment color.
fn shader_source(color: &str) -> String {
    format!(
        r#"
struct Tint {{ color: vec4f }}
@group(0) @binding(0) var<uniform> tint: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {{
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}}

@fragment
fn fs_main() -> @location(0) vec4f {{
    return tint.color * {color};
}}
"#
    )
}

const NO_FRAGMENT: &str = r#"
struct Tint { color: vec4f }
@group(0) @binding(0) var<uniform> tint: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    return tint.color;
}
"#;

const MOVED_BINDING: &str = r#"
struct Tint { color: vec4f }
@group(0) @binding(1) var<uniform> tint: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    return vec4f(0.0, 0.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return tint.color;
}
"#;

/// Same entry points and bindings, but the vertex stage reads a vertex
/// attribute that a full-screen draw has no buffer for, so wgpu rejects
/// its pipeline.
const NEEDS_A_VERTEX_BUFFER: &str = r#"
struct Tint { color: vec4f }
@group(0) @binding(0) var<uniform> tint: Tint;

@vertex
fn vs_main(@location(0) position: vec3f) -> @builtin(position) vec4f {
    return vec4f(position, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return tint.color;
}
"#;

fn options() -> PipelineOptions {
    PipelineOptions::fullscreen_builder()
        .target(RenderUtils::color_alpha_blending(
            wgpu::TextureFormat::Rgba8Unorm,
        ))
        .build()
}

fn write_and_reload(assets: &crate::context::ReadOnlyAssetContext, path: &Path, source: &str) {
    std::fs::write(path, source).expect("the shader is writable");
    let registry = assets.registries.assets.read();
    registry.recv_notify_event(
        Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(path.to_path_buf()),
    );
    std::thread::sleep(Duration::from_millis(200));
    registry.reload_assets();
}

fn reload_error_messages(assets: &crate::context::ReadOnlyAssetContext) -> Vec<String> {
    assets
        .registries
        .assets
        .read()
        .reload_errors()
        .iter()
        .map(|error| error.error.to_string())
        .collect()
}

fn entry_points(shader: &Shader) -> Vec<String> {
    shader
        .module
        .entry_points
        .iter()
        .map(|entry_point| entry_point.name.clone())
        .collect()
}

/// An empty write, a version without `fs_main` and a version with a
/// moved binding are each refused and keep the running shader; a valid
/// edit that keeps the layout reloads.
#[test]
fn a_reload_that_would_break_the_running_shader_keeps_it() {
    let assets = TestAssets::engine();
    let path = assets.engine_root().join(format!("{NAME}.wgsl"));
    std::fs::write(&path, shader_source("1.0")).expect("the shader is writable");
    let context = assets.asset_context();
    let context = context.lock_read();
    let shader = context
        .registries
        .assets
        .read()
        .load::<Shader>(NAME)
        .expect("the shader loads");
    assert!(shader.read().pipeline(&options()).is_some());
    let expected = vec!["vs_main".to_string(), "fs_main".to_string()];

    write_and_reload(&context, &path, "");
    assert_eq!(entry_points(&shader.read()), expected);
    assert!(shader.read().pipeline(&options()).is_some());
    assert!(
        reload_error_messages(&context).is_empty(),
        "an empty write is in progress"
    );

    write_and_reload(&context, &path, NO_FRAGMENT);
    assert_eq!(entry_points(&shader.read()), expected);
    assert!(shader.read().pipeline(&options()).is_some());
    let errors = reload_error_messages(&context);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("fs_main"), "{errors:?}");
    assert!(errors[0].contains("restart"), "{errors:?}");

    write_and_reload(&context, &path, MOVED_BINDING);
    assert_eq!(entry_points(&shader.read()), expected);
    assert!(shader.read().pipeline(&options()).is_some());
    let errors = reload_error_messages(&context);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("group 0 binding"), "{errors:?}");

    write_and_reload(&context, &path, &shader_source("0.5"));
    assert!(shader.read().source.contains("0.5"));
    assert!(shader.read().pipeline(&options()).is_some());
    assert!(reload_error_messages(&context).is_empty());
}

/// A reload wgpu rejects the pipeline of keeps drawing with the pipeline
/// the shader built before, and a shader with none skips the draw.
#[test]
fn a_pipeline_wgpu_rejects_keeps_the_previous_one_or_none() {
    let assets = TestAssets::engine();
    let path = assets.engine_root().join(format!("{NAME}.wgsl"));
    std::fs::write(&path, shader_source("1.0")).expect("the shader is writable");
    let context = assets.asset_context();
    let context = context.lock_read();
    let shader = context
        .registries
        .assets
        .read()
        .load::<Shader>(NAME)
        .expect("the shader loads");
    let before = shader.read().pipeline(&options()).expect("a pipeline");

    write_and_reload(&context, &path, NEEDS_A_VERTEX_BUFFER);
    assert!(reload_error_messages(&context).is_empty());
    assert_eq!(shader.read().pipeline(&options()), Some(before));

    let fresh = assets.engine_root().join("fresh_probe.wgsl");
    std::fs::write(&fresh, NEEDS_A_VERTEX_BUFFER).expect("the shader is writable");
    use crate::assets::Asset;
    let fresh = Shader::from_file(&context, &fresh)
        .expect("the shader loads")
        .asset;
    assert!(fresh.pipeline(&options()).is_none());
}

/// A 1x1 target the probe shaders draw into, and a buffer to read it.
struct Target {
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
}

impl Target {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            texture: device.create_texture(&wgpu::TextureDescriptor {
                label: Some("layout_probe_target"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("layout_probe_readback"),
                size: 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
        }
    }
}

/// Draws a frame of `shader` into `target` with `groups` bound in order
/// and returns its pixel, or the wgpu validation error the frame raised.
fn draw(
    context: &crate::context::ReadOnlyAssetContext,
    shader: &Shader,
    groups: &[&wgpu::BindGroup],
    target: &Target,
) -> Result<[u8; 4], wgpu::Error> {
    let pipeline = shader.pipeline(&options()).expect("a pipeline");
    let device = context.render_context.device();
    let queue = context.render_context.queue();
    let scope = super::super::ValidationScope::push(device);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = target.texture.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("layout_probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        for (index, group) in groups.iter().enumerate() {
            pass.set_bind_group(index as u32, *group, &[]);
        }
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        target.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &target.readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    if let Some(error) = scope.pop() {
        return Err(error);
    }
    let slice = target.readback.slice(..4);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let pixel = {
        let bytes = slice.get_mapped_range();
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    };
    target.readback.unmap();
    Ok(pixel)
}

fn uniform(context: &crate::context::ReadOnlyAssetContext, color: [f32; 4]) -> wgpu::Buffer {
    use egui_wgpu::wgpu::util::DeviceExt;
    context
        .render_context
        .device()
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("layout_probe_uniform"),
            contents: bytemuck::cast_slice(&color),
            usage: wgpu::BufferUsages::UNIFORM,
        })
}

/// The probe shader with its tint uniform renumbered to binding 1 and
/// declared after a helper function.
const RENUMBERED: &str = r#"
struct Tint { color: vec4f }

fn tinted() -> vec4f {
    return tint.color;
}

@group(0) @binding(1) var<uniform> tint: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return tinted();
}
"#;

/// The probe shader with a second uniform its renderer knows nothing of.
const UNKNOWN_BINDING: &str = r#"
struct Tint { color: vec4f }
@group(0) @binding(0) var<uniform> tint: Tint;
@group(0) @binding(1) var<uniform> extra: Tint;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return tint.color * extra.color;
}
"#;

/// A renderer that binds its uniform by name follows a reload that
/// renumbers it: the reload is accepted with a new layout generation,
/// the bind group built for the old layout no longer validates, and one
/// rebuilt for the new layout draws the same color. A reload that adds a
/// binding the renderer does not bind is refused, naming it.
#[test]
fn a_renderer_binding_by_name_follows_a_renumbered_binding() {
    let assets = TestAssets::engine();
    let path = assets.engine_root().join("named_probe.wgsl");
    std::fs::write(&path, shader_source("1.0")).expect("the shader is writable");
    let context = assets.asset_context();
    let context = context.lock_read();
    let shader = context
        .registries
        .assets
        .read()
        .load::<Shader>("named_probe")
        .expect("the shader loads");
    let tint = uniform(&context, [0.0, 1.0, 0.0, 1.0]);
    let target = Target::new(context.render_context.device());
    let bind = |shader: &Shader| {
        shader
            .create_bind_group(0, None, &[("tint", tint.as_entire_binding())])
            .expect("the tint binds")
    };
    let before = bind(&shader.read());
    let generation = shader.read().layout_generation();
    assert_eq!(
        draw(&context, &shader.read(), &[&before], &target).expect("a valid frame"),
        [0, 255, 0, 255]
    );

    write_and_reload(&context, &path, RENUMBERED);
    assert!(reload_error_messages(&context).is_empty());
    assert!(shader.read().source.contains("@binding(1)"));
    assert_ne!(shader.read().layout_generation(), generation);
    assert!(
        draw(&context, &shader.read(), &[&before], &target).is_err(),
        "the bind group of the old layout no longer validates"
    );
    let after = bind(&shader.read());
    assert_eq!(
        draw(&context, &shader.read(), &[&after], &target).expect("a valid frame"),
        [0, 255, 0, 255]
    );

    // A larger uniform than the renderer's buffer is not bound at all,
    // rather than failing when it draws.
    write_and_reload(
        &context,
        &path,
        &RENUMBERED.replace("color: vec4f }", "color: vec4f, scale: vec4f }"),
    );
    assert!(reload_error_messages(&context).is_empty());
    assert!(shader
        .read()
        .create_bind_group(0, None, &[("tint", tint.as_entire_binding())])
        .is_none());

    // A version that no longer binds the tint at all is refused: the
    // renderer would skip its draws.
    write_and_reload(
        &context,
        &path,
        &shader_source("1.0")
            .replace("return tint.color * 1.0;", "return vec4f(1.0);")
            .replace("@group(0) @binding(0) var<uniform> tint: Tint;", ""),
    );
    let errors = reload_error_messages(&context);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("group 0 no longer declares"),
        "{errors:?}"
    );

    write_and_reload(&context, &path, UNKNOWN_BINDING);
    let errors = reload_error_messages(&context);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("group 0 binding 1 `extra`"),
        "{errors:?}"
    );
    assert!(shader.read().source.contains("tinted"));
}

/// A reload that keeps every binding keeps the layout generation, so
/// bind groups built before it stay valid.
#[test]
fn a_reload_that_keeps_the_layout_keeps_its_generation() {
    let assets = TestAssets::engine();
    let path = assets.engine_root().join("kept_probe.wgsl");
    std::fs::write(&path, shader_source("1.0")).expect("the shader is writable");
    let context = assets.asset_context();
    let context = context.lock_read();
    let shader = context
        .registries
        .assets
        .read()
        .load::<Shader>("kept_probe")
        .expect("the shader loads");
    let generation = shader.read().layout_generation();
    write_and_reload(&context, &path, &shader_source("0.5"));
    assert!(shader.read().source.contains("0.5"));
    assert_eq!(shader.read().layout_generation(), generation);
}

/// A material shader: groups 0 to 2 hold a uniform each, as a renderer
/// fills them, and group 3 the material's own color. `extra` is spliced
/// into group 3 and `sample` into the fragment color.
fn material_shader(extra: &str, sample: &str) -> String {
    format!(
        r#"
struct Tint {{ color: vec4f }}
@group(0) @binding(0) var<uniform> u0: Tint;
@group(1) @binding(0) var<uniform> u1: Tint;
@group(2) @binding(0) var<uniform> u2: Tint;

struct MaterialProperties {{ base_color: vec4f }}
@group(3) @binding(0) var<uniform> material: MaterialProperties;
{extra}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {{
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}}

@fragment
fn fs_main() -> @location(0) vec4f {{
    return u0.color * u1.color * u2.color * material.base_color{sample};
}}
"#
    )
}

/// A reload that adds a texture and its sampler to a material's group is
/// accepted; the material gains the new inputs with their defaults,
/// keeps its color, and its rebuilt bind group draws a valid frame. A
/// storage buffer, which a material cannot fill, is refused, naming it.
#[test]
fn a_material_follows_a_reload_that_adds_an_optional_binding() {
    use crate::assets::material::{Material, MaterialTexture, ShaderVariableValue};
    use crate::render::AssetRenderState;

    let assets = TestAssets::engine();
    let path = assets.engine_root().join("material_probe.wgsl");
    std::fs::write(&path, material_shader("", "")).expect("the shader is writable");
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    let shader = registry
        .load::<Shader>("material_probe")
        .expect("the shader loads");
    let default_texture = registry.white_texture().expect("the white texture");
    drop(registry);
    let mut material = Material::from_shader(&context, shader.clone());
    assert!(material.set_variable(
        "base_color",
        ShaderVariableValue::Color([0.0, 0.0, 1.0, 1.0])
    ));
    let white = uniform(&context, [1.0; 4]);
    let target = Target::new(context.render_context.device());
    let draw_material = |material: &mut Material| {
        material.follow_shader(&context);
        material.load_buffers(&context.render_context);
        let mut state = AssetRenderState::default();
        state.shaders.insert(shader.id(), shader.clone());
        material.collect_textures(&context, &mut state.textures, default_texture.clone());
        let locked = state.lock();
        let groups = material.bind_groups(
            context.render_context.device(),
            &context,
            &locked,
            default_texture.clone(),
        );
        let shader = shader.read();
        let renderer_groups = ["u0", "u1", "u2"]
            .iter()
            .enumerate()
            .map(|(group, name)| {
                shader
                    .create_bind_group(group as u32, None, &[(name, white.as_entire_binding())])
                    .expect("the uniform binds")
            })
            .collect::<Vec<_>>();
        let material_group = groups.get(&3).expect("the material binds group 3");
        draw(
            &context,
            &shader,
            &[
                &renderer_groups[0],
                &renderer_groups[1],
                &renderer_groups[2],
                material_group,
            ],
            &target,
        )
    };
    assert_eq!(
        draw_material(&mut material).expect("a valid frame"),
        [0, 0, 255, 255]
    );

    let texture = "@group(3) @binding(1) var tint_texture: texture_2d<f32>;\n\
                       @group(3) @binding(2) var tint_sampler: sampler;";
    write_and_reload(
        &context,
        &path,
        &material_shader(
            texture,
            " * textureSample(tint_texture, tint_sampler, vec2f(0.5))",
        ),
    );
    assert!(reload_error_messages(&context).is_empty());
    assert!(shader.read().source.contains("tint_texture"));
    assert_eq!(
        draw_material(&mut material).expect("a valid frame"),
        [0, 0, 255, 255],
        "the new texture defaults to white and the color is kept"
    );
    assert!(material
        .variables
        .iter()
        .any(|variable| variable.name == "tint_texture"));

    // A renumbered texture keeps the value set on it, by its name.
    assert!(material.set_variable(
        "tint_texture",
        ShaderVariableValue::Texture2D(MaterialTexture::Color([0.0, 0.0, 0.5, 1.0])),
    ));
    assert_eq!(
        draw_material(&mut material).expect("a valid frame"),
        [0, 0, 128, 255]
    );
    let renumbered = "@group(3) @binding(3) var tint_sampler: sampler;\n\
                          @group(3) @binding(4) var tint_texture: texture_2d<f32>;";
    write_and_reload(
        &context,
        &path,
        &material_shader(
            renumbered,
            " * textureSample(tint_texture, tint_sampler, vec2f(0.5))",
        ),
    );
    assert!(reload_error_messages(&context).is_empty());
    assert_eq!(
        draw_material(&mut material).expect("a valid frame"),
        [0, 0, 128, 255],
        "the texture keeps its value at its new binding"
    );

    // A lone scalar uniform gets a buffer of its own size, which a
    // unit multiplier fills with one.
    write_and_reload(
        &context,
        &path,
        &material_shader(
            "@group(3) @binding(1) var<uniform> normal_scale: f32;",
            " * normal_scale",
        ),
    );
    assert!(reload_error_messages(&context).is_empty());
    assert_eq!(
        draw_material(&mut material).expect("a valid frame"),
        [0, 0, 255, 255]
    );

    // A nested struct's members are written at their own offsets: the
    // inner scale multiplies and the outer offset, zero, adds nothing.
    write_and_reload(
        &context,
        &path,
        &material_shader(
            "struct Inner { scale: vec4f }
                 struct Outer { bias: vec4f, inner: Inner }
                 @group(3) @binding(1) var<uniform> extra: Outer;",
            " * extra.inner.scale + extra.bias",
        ),
    );
    assert!(reload_error_messages(&context).is_empty());
    // The material takes the new variables as it draws its first frame.
    draw_material(&mut material).expect("a valid frame");
    assert!(material.set_variable("scale", ShaderVariableValue::Vec4([1.0; 4])));
    assert_eq!(
        draw_material(&mut material).expect("a valid frame"),
        [0, 0, 255, 255]
    );

    // A uniform whose entry stays the same but whose type no variable
    // reflects is refused too: the material would drop its buffer.
    write_and_reload(
        &context,
        &path,
        &material_shader(
            "struct Flags { flags: vec4u }
                 @group(3) @binding(1) var<uniform> extra: Flags;",
            " * vec4f(extra.flags)",
        ),
    );
    let errors = reload_error_messages(&context);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("group 3 binding 1 `extra`"),
        "{errors:?}"
    );

    // Dropping the whole material group is refused: the passes that
    // bind it would skip every batch.
    write_and_reload(
        &context,
        &path,
        &material_shader("", "")
            .replace(
                "@group(3) @binding(0) var<uniform> material: MaterialProperties;",
                "",
            )
            .replace(" * material.base_color", ""),
    );
    let errors = reload_error_messages(&context);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("group 3 no longer declares"),
        "{errors:?}"
    );

    // A storage buffer, and a uniform array no variable reflects, are
    // bindings a material cannot fill.
    for binding in [
        "@group(3) @binding(1) var<storage, read> weights: array<f32>;",
        "@group(3) @binding(1) var<uniform> weights: array<vec4f, 2>;",
    ] {
        write_and_reload(
            &context,
            &path,
            &material_shader(binding, " * vec4f(weights[0])"),
        );
        let errors = reload_error_messages(&context);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("group 3 binding 1 `weights`"),
            "{errors:?}"
        );
        assert!(errors[0].contains("material cannot fill"), "{errors:?}");
    }
}

/// A pass's shader whose group 2 binds the materials of a material
/// shader's group 3, as the G-buffer and masked shadow shaders do.
/// `extra` is spliced into group 2 and `color` is its fragment color.
fn pass_shader(extra: &str, color: &str) -> String {
    format!(
        r#"
struct Tint {{ color: vec4f }}
@group(0) @binding(0) var<uniform> u0: Tint;
@group(1) @binding(0) var<uniform> u1: Tint;

struct MaterialProperties {{ base_color: vec4f }}
@group(2) @binding(0) var<uniform> material: MaterialProperties;
{extra}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {{
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}}

@fragment
fn fs_main() -> @location(0) vec4f {{
    return u0.color * u1.color * material.base_color * {color};
}}
"#
    )
}

/// Once a pass's group 2 is found to share a material group, a reload
/// of the pass that keeps its bindings is accepted, and so is one that
/// adds a binding a material fills, which then no longer matches the
/// material shader until it follows.
#[test]
fn a_pass_sharing_a_material_group_follows_its_reloads() {
    let assets = TestAssets::engine();
    let material_path = assets.engine_root().join("shared_material_probe.wgsl");
    std::fs::write(&material_path, material_shader("", "")).expect("the shader is writable");
    let path = assets.engine_root().join("pass_probe.wgsl");
    std::fs::write(&path, pass_shader("", "1.0")).expect("the shader is writable");
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    let material = registry
        .load::<Shader>("shared_material_probe")
        .expect("the material shader loads");
    let pass = registry
        .load::<Shader>("pass_probe")
        .expect("the pass loads");
    drop(registry);
    assert!(pass.read().shares_material_group(2, &material.read(), 3));

    // A material uniform of another size, with the same entries, is a
    // buffer the pass's group cannot bind.
    write_and_reload(
        &context,
        &material_path,
        &material_shader("", "").replace("base_color: vec4f }", "base_color: vec4f, gain: vec4f }"),
    );
    assert!(reload_error_messages(&context).is_empty());
    assert!(!pass.read().shares_material_group(2, &material.read(), 3));
    // Nor is one whose bindings keep their kinds and sizes under other
    // variables.
    write_and_reload(
        &context,
        &material_path,
        &material_shader("", "")
            .replace("var<uniform> material:", "var<uniform> props:")
            .replace("* material.base_color", "* props.base_color"),
    );
    assert!(reload_error_messages(&context).is_empty());
    assert!(!pass.read().shares_material_group(2, &material.read(), 3));
    write_and_reload(&context, &material_path, &material_shader("", ""));
    assert!(pass.read().shares_material_group(2, &material.read(), 3));

    write_and_reload(&context, &path, &pass_shader("", "0.5"));
    assert!(reload_error_messages(&context).is_empty());
    assert!(pass.read().source.contains("0.5"));

    // A group the pass does not bind materials to stays its renderer's:
    // the G-buffer renderer binds nothing at group 3.
    write_and_reload(
        &context,
        &path,
        &pass_shader(
            "@group(3) @binding(0) var<uniform> extra: Tint;",
            "extra.color",
        ),
    );
    let errors = reload_error_messages(&context);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("group 3 binding 0 `extra` was added"),
        "{errors:?}"
    );

    let texture = "@group(2) @binding(1) var tint_texture: texture_2d<f32>;\n\
                       @group(2) @binding(2) var tint_sampler: sampler;";
    write_and_reload(
        &context,
        &path,
        &pass_shader(
            texture,
            "textureSample(tint_texture, tint_sampler, vec2f(0.5))",
        ),
    );
    assert!(reload_error_messages(&context).is_empty());
    assert!(!pass.read().shares_material_group(2, &material.read(), 3));
}

const INVALID: &str = "fn main() { let x: u32 = 1.0; }";
const VALID: &str = "@compute @workgroup_size(1) fn main() {}";

fn create_module(device: &wgpu::Device, source: &'static str) {
    let _module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scope_probe"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
}

/// A scope dropped without `pop` is popped by its guard, taking its
/// error with it, so the scope pushed before it is the one popped next.
/// Its outer scope is pushed outside the guard, so it runs on a device of
/// its own that no other test's scopes can reach.
#[test]
fn a_dropped_validation_scope_pops_itself() {
    let adapter = crate::test_support::test_adapter();
    let (device, _queue) = pollster::block_on(adapter.request_device(&Default::default(), None))
        .expect("a second test device");
    let device = &device;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    {
        let _scope = super::super::ValidationScope::push(device);
        create_module(device, INVALID);
    }
    assert!(pollster::block_on(device.pop_error_scope()).is_none());
}

/// Scopes opened on two threads at once each see only their own errors.
#[test]
fn validation_scopes_on_two_threads_keep_their_own_errors() {
    let render_context = crate::test_support::test_render_context();
    let device = render_context.device();
    std::thread::scope(|threads| {
        let invalid = threads.spawn(|| {
            (0..200).all(|_| {
                let scope = super::super::ValidationScope::push(device);
                create_module(device, INVALID);
                scope.pop().is_some()
            })
        });
        let valid = threads.spawn(|| {
            (0..200).all(|_| {
                let scope = super::super::ValidationScope::push(device);
                create_module(device, VALID);
                scope.pop().is_none()
            })
        });
        assert!(
            invalid.join().unwrap(),
            "an invalid module's error was lost"
        );
        assert!(valid.join().unwrap(), "a valid module got another's error");
    });
}
