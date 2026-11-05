use super::*;

#[test]
fn rendering_defaults_preserve_legacy_billboards_and_new_settings_round_trip() {
    let legacy: ParticleRendering = serde_json::from_value(serde_json::json!({
        "blend_mode": "Alpha",
    }))
    .unwrap();
    assert_eq!(legacy.blend_mode, ParticleBlendMode::Alpha);
    assert_eq!(legacy.intensity, 1.0);
    assert_eq!(legacy.min_screen_size, 6.0);
    assert_eq!(legacy.billboard, ParticleBillboard::CameraFacing);

    for billboard in [
        ParticleBillboard::VelocityAligned { length_scale: 6.0 },
        ParticleBillboard::Directional {
            direction: vec3(0.0, 0.0, 1.0),
            length_scale: 1.5625,
        },
    ] {
        let settings = ParticleRendering {
            intensity: 30.0,
            min_screen_size: 0.0,
            billboard,
            ..Default::default()
        };
        let restored: ParticleRendering =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(restored.intensity, settings.intensity);
        assert_eq!(restored.min_screen_size, 0.0);
        assert_eq!(restored.billboard, billboard);
    }
}

fn single_particle() -> ComponentParticleSystem {
    let mut system = still_burst(1);
    system.shape = ParticleSpawnShape::Sphere { radius: 0.0 };
    system.size = ParticleSizeOverLifetime::linear(0.02, 0.02, 0.0);
    system.color = ParticleColorOverLifetime::linear(Color32::WHITE, Color32::WHITE);
    system
}

/// A non-square render target exposes aspect errors when a streak rotates.
fn render_particle(
    system: ComponentParticleSystem,
    emitter: Transform,
) -> (image::RgbaImage, ParticleRenderInstance) {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let mut scene = context.scene();
    let object = spawn_system(&mut scene, Uuid::new_v4(), &emitter, system);
    let mut simulation = ParticleSimulation::new();
    simulation.simulate(&scene, &render_context, 0.0);
    let (mut camera, transform) = camera_at(vec3(0.0, 0.0, 5.0));
    camera.aspect = 2.0;
    camera.update_projection();
    let mut renderer = SceneRenderer::new(
        &context,
        SceneRendererOptions {
            samples: 1,
            clear_color: Color32::BLACK,
            ..Default::default()
        },
        (128, 64),
    )
    .unwrap();
    renderer.render_scene_base(&camera, &transform, &scene, Some(&simulation), None);
    renderer.finalize_scene();
    let instance = renderer
        .particle_renderer()
        .read_instances(&render_context, object)[0];
    (read_scene_texture(&renderer, &render_context), instance)
}

fn lit_bounds(image: &image::RgbaImage) -> Option<(u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (x, y, _) in image.enumerate_pixels().filter(|(_, _, c)| c[0] > 30) {
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((left, top, right, bottom)) => {
                (left.min(x), top.min(y), right.max(x), bottom.max(y))
            }
        });
    }
    bounds.map(|(left, top, right, bottom)| (right - left + 1, bottom - top + 1))
}

#[test]
fn gpu_billboards_preserve_the_default_floor_and_rotate_streaks_in_physical_pixels() {
    let (disc, _) = render_particle(single_particle(), Transform::default());
    let (width, height) = lit_bounds(&disc).expect("default six-pixel disc draws");
    assert!(
        (5..=7).contains(&width) && (5..=7).contains(&height),
        "{width}x{height}"
    );

    let mut no_floor = single_particle();
    no_floor.rendering.min_screen_size = 0.0;
    let (tiny, _) = render_particle(no_floor, Transform::default());
    assert!(lit_bounds(&tiny).is_none_or(|(w, h)| w < width && h < height));

    for (direction, horizontal) in [
        (vec3(1.0, 0.0, 0.0), true),
        (vec3(0.0, 1.0, 0.0), false),
        (vec3(0.0, 0.0, 1.0), false),
        (Vec3::zeros(), false),
    ] {
        let mut streak = single_particle();
        streak.rendering.blend_mode = ParticleBlendMode::Alpha;
        streak.rendering.min_screen_size = 4.0;
        streak.rendering.billboard = ParticleBillboard::Directional {
            direction,
            length_scale: 4.0,
        };
        let (image, _) = render_particle(streak, Transform::default());
        let (w, h) = lit_bounds(&image).expect("directional streak draws");
        let (long, short) = if horizontal { (w, h) } else { (h, w) };
        assert!(
            (14..=17).contains(&long) && (3..=5).contains(&short),
            "{direction:?}: {w}x{h}"
        );
    }

    let mut hidden = single_particle();
    hidden.size = ParticleSizeOverLifetime::linear(0.0, 0.0, 0.0);
    hidden.rendering.billboard = ParticleBillboard::VelocityAligned { length_scale: 8.0 };
    let (image, _) = render_particle(hidden, Transform::default());
    assert_eq!(
        lit_bounds(&image),
        None,
        "zero-sized particles never draw the floor"
    );
}

#[test]
fn gpu_evaluation_keeps_hdr_alpha_and_transforms_velocity_or_fixed_direction() {
    for local_space in [false, true] {
        let mut system = single_particle();
        system.local_space = local_space;
        system.velocity.initial = vec3(2.0, 0.0, 0.0);
        system.color = ParticleColorOverLifetime {
            gradient: Gradient::linear([1.0, 0.43415, 0.0, 0.25], [1.0, 0.43415, 0.0, 0.25]),
        };
        system.rendering.intensity = 30.0;
        system.rendering.min_screen_size = 1.5;
        system.rendering.billboard = ParticleBillboard::VelocityAligned { length_scale: 5.0 };
        let emitter = Transform::from_components(
            Vec3::zeros(),
            UnitQuaternion::from_euler_angles(0.0, 0.0, std::f32::consts::FRAC_PI_2),
            vec3(1.0, 2.0, 1.0),
        );
        let (_, instance) = render_particle(system, emitter);
        for (actual, expected) in instance.color.into_iter().zip([30.0, 13.0245, 0.0, 0.25]) {
            assert!((actual - expected).abs() < 1e-3, "HDR/alpha: {instance:?}");
        }
        for (actual, expected) in instance
            .direction_min_size
            .into_iter()
            .zip([0.0, 5.0, 0.0, 1.5])
        {
            assert!(
                (actual - expected).abs() < 1e-3,
                "local={local_space}: {instance:?}"
            );
        }
    }
    // A fixed direction is still transformed by the emitter for a stationary flash.
    let mut system = single_particle();
    system.rendering.billboard = ParticleBillboard::Directional {
        direction: vec3(0.0, 0.0, 1.0),
        length_scale: 1.5625,
    };
    let emitter = Transform::from_components(
        Vec3::zeros(),
        UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
        vec3(1.0, 1.0, 1.0),
    );
    let (_, instance) = render_particle(system, emitter);
    assert!((instance.direction_min_size[0] - 1.5625).abs() < 1e-4);
    assert!(instance.direction_min_size[1].abs() < 1e-4);
    assert!(instance.direction_min_size[2].abs() < 1e-4);
}
