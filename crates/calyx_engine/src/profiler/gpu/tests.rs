use super::*;

#[test]
fn a_device_without_timestamps_reports_which_feature_it_lacks() {
    let (status, timer) = timing_status(wgpu::Features::empty());
    assert!(!status.available);
    assert_eq!(timer, None);
    assert!(status.reason.unwrap().contains("TIMESTAMP_QUERY"));

    let (status, timer) = timing_status(wgpu::Features::TIMESTAMP_QUERY);
    assert!(!status.available);
    assert_eq!(timer, None);
    assert!(status
        .reason
        .unwrap()
        .contains("TIMESTAMP_QUERY_INSIDE_ENCODERS"));

    let (status, timer) = timing_status(
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    );
    assert!(status.available);
    assert_eq!(
        timer,
        Some(false),
        "the UI pass needs timestamps inside passes"
    );
    assert!(status.reason.unwrap().contains("UI pass"));
}

/// Times a clear pass on the test device, when it has the timestamp
/// features, through a timer of its own rather than the global one.
/// The inner scope names no parent and nests in the one open around it.
#[test]
fn a_timed_pass_reads_back_as_a_nested_scope() {
    let context = crate::test_support::test_render_context();
    let device = context.device();
    let (_, Some(pass_timestamps)) = timing_status(device.features()) else {
        eprintln!("skipped: the test device has no timestamp queries");
        return;
    };
    let mut timer = GpuTimer::new(device, context.queue(), pass_timestamps);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let outer = timer
        .begin(&mut encoder, Cow::Borrowed("Frame"), None)
        .expect("a free slot");
    let inner = timer
        .begin(&mut encoder, Cow::Borrowed("Clear"), None)
        .expect("room for a second scope");
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    }));
    timer.end(&mut encoder, inner);
    timer.end(&mut encoder, outer);
    context.queue().submit(Some(encoder.finish()));
    timer.end_frame(5);
    let _ = device.poll(wgpu::Maintain::Wait);

    let frames = timer.collect();
    assert_eq!(frames.len(), 1);
    let (frame, passes) = &frames[0];
    assert_eq!(*frame, 5);
    assert_eq!(passes.len(), 1);
    assert_eq!(passes[0].name, "Frame");
    assert_eq!(passes[0].children[0].name, "Clear");
    assert!(passes[0].children[0].ns <= passes[0].ns);
}

/// A stretch timed inside a render pass, as [`PassScope`] times one
/// shader's draws, nests in the encoder scope open around the pass.
#[test]
fn a_stretch_inside_a_pass_reads_back_nested_in_the_scope_around_it() {
    let context = crate::test_support::test_render_context();
    let device = context.device();
    let (_, Some(true)) = timing_status(device.features()) else {
        eprintln!("skipped: the test device has no timestamps inside passes");
        return;
    };
    let mut timer = GpuTimer::new(device, context.queue(), true);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let pass_scope = timer
        .begin(&mut encoder, Cow::Borrowed("Forward"), None)
        .expect("a free slot");
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        let stretch = timer
            .begin(&mut pass, Cow::Borrowed("water"), None)
            .expect("room for a second scope");
        timer.end(&mut pass, stretch);
    }
    timer.end(&mut encoder, pass_scope);
    context.queue().submit(Some(encoder.finish()));
    timer.end_frame(7);
    let _ = device.poll(wgpu::Maintain::Wait);

    let frames = timer.collect();
    let (_, passes) = &frames[0];
    assert_eq!(passes[0].name, "Forward");
    assert_eq!(passes[0].children[0].name, "water");
}

fn query(label: &'static str, parent: Option<usize>, start: u32, ended: bool) -> GpuQuery {
    GpuQuery {
        label: Cow::Borrowed(label),
        parent,
        start,
        ended,
    }
}

#[test]
fn timestamps_become_a_tree_of_passes_in_nanoseconds() {
    let queries = [
        query("Scene view", None, 0, true),
        query("Shadows", Some(0), 2, true),
        query("Lighting", Some(0), 4, true),
        query("Shadows", Some(0), 6, true),
        query("UI", None, 8, false),
    ];
    let timestamps = [100, 400, 110, 150, 150, 300, 300, 320, 400, 500];
    let samples = samples_of(&queries, &timestamps, 2.0);
    assert_eq!(samples.len(), 1, "the unfinished UI scope is dropped");
    let scene = &samples[0];
    assert_eq!((scene.name.as_str(), scene.ns), ("Scene view", 600));
    let children: Vec<(&str, u64, u32)> = scene
        .children
        .iter()
        .map(|child| (child.name.as_str(), child.ns, child.calls))
        .collect();
    assert_eq!(children, [("Shadows", 120, 2), ("Lighting", 300, 1)]);
}
