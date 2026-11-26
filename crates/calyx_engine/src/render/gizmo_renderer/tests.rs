use super::*;
use crate::test_support::TestAssets;

#[test]
fn only_objects_holding_a_gizmo_type_are_asked_to_draw() {
    use crate::component::{ComponentCollider, ComponentPointLight};
    use crate::render::scene_renderer::CameraUniform;
    use crate::test_support::test_scene;

    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let device = context.render_context.device();
    let queue = context.render_context.queue();
    let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[CameraUniform::default()]),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let mut renderer = GizmoRenderer::new(&context, &camera_buffer, 1).unwrap();
    let mut scene = test_scene();
    // Many objects that draw no gizmo, and one light that does.
    for _ in 0..50 {
        let prop = scene.create(None, None);
        scene.add_component(prop, ComponentCollider::default());
    }
    let light = scene.create(None, None);
    scene.add_component(light, ComponentPointLight::default());
    let mut asked = Vec::new();
    renderer.draw_gizmos(
        device,
        queue,
        &Camera::default(),
        &Transform::default(),
        [64.0, 64.0],
        &scene,
        &GizmoOptions::default(),
        None,
        |id| {
            asked.push(id);
            1
        },
    );
    assert_eq!(asked, [scene.uuid(light)]);
    assert!(
        !renderer.circle_list.is_empty(),
        "the light draws its range sphere"
    );
}

#[test]
fn gizmo_icon_shader_builds_pipeline() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let shader = Shader::from_file(
        &read_only_context,
        &assets.engine_root().join("shaders/editor/gizmo_icons.wgsl"),
    )
    .expect("icon shader should load")
    .asset;
    let mut depth_stencil = RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float);
    depth_stencil.depth_write_enabled = false;
    let icon_options = PipelineOptions::builder()
        .cull_mode(None)
        .depth_stencil(Some(depth_stencil))
        .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(
            wgpu::TextureFormat::Rgba16Float,
        ))])
        .build();

    let pipeline = shader.pipeline(&icon_options);

    assert_eq!(shader.pipeline(&icon_options), pipeline);
}

#[test]
fn gizmo_icon_object_id_shader_builds_pipeline() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let shader = Shader::from_file(
        &read_only_context,
        &assets
            .engine_root()
            .join("shaders/editor/gizmo_icon_object_id.wgsl"),
    )
    .expect("icon object id shader should load")
    .asset;
    let mut depth_stencil = RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float);
    depth_stencil.depth_write_enabled = false;
    let icon_options = PipelineOptions::builder()
        .samples(1)
        .cull_mode(None)
        .depth_stencil(Some(depth_stencil))
        .fragment_targets(vec![Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R32Uint,
            blend: None,
            write_mask: wgpu::ColorWrites::RED,
        })])
        .build();

    let pipeline = shader.pipeline(&icon_options);

    assert_eq!(shader.pipeline(&icon_options), pipeline);
}

#[test]
fn instance_chunks_hold_gizmo_instances_beyond_the_cap() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let shader = Shader::from_file(
        &read_only_context,
        &assets.engine_root().join("shaders/editor/gizmos.wgsl"),
    )
    .expect("gizmo shader should load")
    .asset;
    let instance = GizmoInstance {
        transform: Mat4::identity().into(),
        color: [1.0; 4],
        enable_normals: 0,
        use_uv_colors: 0,
        _padding: Default::default(),
    };
    let instances = vec![instance; MAX_GIZMO_INSTANCES * 2 + 1];
    let mut chunks = InstanceChunks::new("test_instance_buffer");

    chunks.upload(
        read_only_context.render_context.device(),
        read_only_context.render_context.queue(),
        &shader.bind_group_layouts[1],
        &instances,
    );

    assert_eq!(chunks.chunks.len(), 3);
}
