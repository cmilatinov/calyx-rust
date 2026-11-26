use super::*;
use crate::math::{CurveInterpolation, CurveKey, GradientKey, Transform};
use approx::assert_abs_diff_eq;

fn particle() -> Particle {
    Particle {
        lifetime: 1.0,
        ..Default::default()
    }
}

#[test]
fn world_space_init_applies_the_emitter_transform() {
    let emitter = Transform::from_xyz(2.0, 0.0, 0.0);
    let space = SimulationSpace {
        emitter: &emitter,
        local: false,
    };
    let mut rng = ParticleRng::default();
    let mut particle = particle();
    ParticleSpawnShape::Sphere { radius: 0.0 }.init(&mut particle, &mut rng, space);
    assert_eq!(particle.position, [2.0, 0.0, 0.0]);

    let local = SimulationSpace {
        local: true,
        ..space
    };
    ParticleSpawnShape::Sphere { radius: 0.0 }.init(&mut particle, &mut rng, local);
    assert_eq!(particle.position, [0.0, 0.0, 0.0]);
}

#[test]
fn box_shape_stays_inside_its_extents() {
    let emitter = Transform::default();
    let space = SimulationSpace {
        emitter: &emitter,
        local: true,
    };
    let extents = vec3(0.5, 1.0, 2.0);
    let mut rng = ParticleRng::seeded(7);
    let mut particle = particle();
    for _ in 0..64 {
        ParticleSpawnShape::Box { extents }.init(&mut particle, &mut rng, space);
        for axis in 0..3 {
            assert!(particle.position[axis].abs() <= extents[axis]);
        }
    }
}

#[test]
fn velocity_randomness_varies_per_particle() {
    let emitter = Transform::default();
    let space = SimulationSpace {
        emitter: &emitter,
        local: true,
    };
    let velocity = ParticleVelocity {
        initial: Vec3::zeros(),
        randomness: Vec3::from_element(1.0),
    };
    let mut rng = ParticleRng::default();
    let mut first = particle();
    let mut second = particle();
    velocity.init(&mut first, &mut rng, space);
    velocity.init(&mut second, &mut rng, space);
    assert_ne!(first.velocity, second.velocity);
}

#[test]
fn an_empty_lifetime_range_draws_no_random_value() {
    let mut rng = ParticleRng::default();
    let mut particle = particle();
    ParticleScalarRange { min: 0.5, max: 0.5 }.init_lifetime(&mut particle, &mut rng);
    assert_eq!(particle.lifetime, 0.5);
    let mut untouched = ParticleRng::default();
    assert_eq!(rng.scalar(), untouched.scalar());

    ParticleScalarRange { min: 0.0, max: 0.0 }.init_lifetime(&mut particle, &mut rng);
    assert_eq!(particle.lifetime, MIN_PARTICLE_LIFETIME);
}

/// Size at normalized age `t` as the start/end size module computed it
/// before curves: each end offset and clamped, then interpolated.
fn start_end_size(start: f32, end: f32, offset: f32, t: f32) -> f32 {
    let start = (start + offset).max(0.0);
    let end = (end + offset).max(0.0);
    start + (end - start) * t
}

#[test]
fn size_interpolates_and_clamps_at_zero() {
    let size = ParticleSizeOverLifetime::linear(1.0, 0.25, 0.0);
    let mut particle = particle();
    assert_eq!(size.evaluate(&particle, 0.5), 0.625);
    particle.size_offset = -0.5;
    assert_eq!(size.evaluate(&particle, 1.0), 0.0);
}

#[test]
fn default_size_curve_reproduces_the_start_end_sizes() {
    let size = ParticleSizeOverLifetime::default();
    let mut particle = particle();
    for offset in [-0.3, -0.1, 0.0, 0.2, 0.3] {
        particle.size_offset = offset;
        for t in [0.0, 0.25, 0.5, 0.9, 1.0] {
            assert_abs_diff_eq!(
                size.evaluate(&particle, t),
                start_end_size(0.6, 0.0, offset, t),
                epsilon = 1e-6
            );
        }
    }
}

#[test]
fn size_offset_shifts_every_key_of_the_curve() {
    let size = ParticleSizeOverLifetime {
        curve: Curve::new(vec![
            CurveKey::new(0.0, 0.0).with_interpolation(CurveInterpolation::Smooth),
            CurveKey::new(0.5, 1.0),
            CurveKey::new(1.0, 0.2),
        ]),
        randomness: 0.0,
    };
    let mut particle = particle();
    particle.size_offset = 0.25;
    // Smoothstep halfway between 0.25 and 1.25.
    assert_abs_diff_eq!(size.evaluate(&particle, 0.25), 0.75);
    assert_abs_diff_eq!(size.evaluate(&particle, 1.0), 0.45);
    particle.size_offset = -0.5;
    // Keys 0.0 and 0.2 clamp to zero, 1.0 becomes 0.5.
    assert_eq!(size.evaluate(&particle, 0.0), 0.0);
    assert_abs_diff_eq!(size.evaluate(&particle, 0.75), 0.25);
    assert_eq!(size.evaluate(&particle, 1.0), 0.0);
}

#[test]
fn color_lerp_uses_unmultiplied_alpha() {
    let color = ParticleColorOverLifetime::linear(
        Color32::from_rgba_unmultiplied(255, 169, 0, 84),
        Color32::from_rgba_unmultiplied(255, 169, 0, 84),
    )
    .evaluate(0.5);

    assert!(color[0] > 0.9, "expected red to stay unpremultiplied");
    assert!(color[1] > 0.55, "expected green to stay unpremultiplied");
    assert!(color[2] < 0.05, "expected blue to stay near zero");
    assert!((color[3] - (84.0 / 255.0)).abs() < 0.02);
}

#[test]
fn default_color_gradient_reproduces_the_start_end_colors() {
    let color = ParticleColorOverLifetime::default();
    // The start/end module stored premultiplied `Color32`s and the
    // renderer interpolated their unmultiplied bytes.
    let bytes = |color: Color32| color.to_srgba_unmultiplied().map(f32::from);
    let start = bytes(Color32::from_rgba_unmultiplied(255, 4, 0, 255));
    let end = bytes(Color32::from_rgba_unmultiplied(255, 169, 0, 84));
    assert_eq!(color.gradient.keys.len(), 2);
    for t in [0.0, 0.3, 0.5, 1.0] {
        let expected: [f32; 4] =
            std::array::from_fn(|i| (start[i] + (end[i] - start[i]) * t) / 255.0);
        let actual = color.evaluate(t);
        for channel in 0..4 {
            assert_abs_diff_eq!(actual[channel], expected[channel], epsilon = 1e-6);
        }
    }
}

#[test]
fn color_follows_its_gradient_keys() {
    let color = ParticleColorOverLifetime {
        gradient: Gradient::new(vec![
            GradientKey::rgba(0.0, [1.0, 1.0, 0.0, 1.0]),
            GradientKey::rgba(0.5, [1.0, 0.0, 0.0, 1.0]),
            GradientKey::rgba(1.0, [0.0, 0.0, 0.0, 0.0]),
        ]),
    };
    assert_eq!(color.evaluate(0.25), [1.0, 0.5, 0.0, 1.0]);
    assert_eq!(color.evaluate(0.75), [0.5, 0.0, 0.0, 0.5]);
}

#[test]
fn start_end_size_loads_as_a_linear_curve() {
    let size: ParticleSizeOverLifetime =
        serde_json::from_str(r#"{"end":0.35,"randomness":0.15,"start":1.1}"#).unwrap();
    assert_eq!(size, ParticleSizeOverLifetime::linear(1.1, 0.35, 0.15));
    let partial: ParticleSizeOverLifetime = serde_json::from_str(r#"{"end":2.0}"#).unwrap();
    assert_eq!(partial, ParticleSizeOverLifetime::linear(0.6, 2.0, 0.3));
    let empty: ParticleSizeOverLifetime = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, ParticleSizeOverLifetime::default());
}

#[test]
fn start_end_color_loads_as_a_two_key_gradient() {
    // Color32 serializes its premultiplied bytes.
    let color: ParticleColorOverLifetime =
        serde_json::from_str(r#"{"end":[90,42,0,90],"start":[255,215,30,255]}"#).unwrap();
    let start = Color32::from_rgba_premultiplied(255, 215, 30, 255);
    let end = Color32::from_rgba_premultiplied(90, 42, 0, 90);
    assert_eq!(color, ParticleColorOverLifetime::linear(start, end));
    let keys = &color.gradient.keys;
    assert_eq!((keys.len(), keys[0].time, keys[1].time), (2, 0.0, 1.0));
    let unmultiplied = end.to_srgba_unmultiplied();
    assert!(unmultiplied[0] > 90, "the end red is unmultiplied");
    assert_eq!(keys[1].color.x, unmultiplied[0] as f32 / 255.0);
    assert_eq!(keys[1].color.w, 90.0 / 255.0);
    let empty: ParticleColorOverLifetime = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, ParticleColorOverLifetime::default());
}

#[test]
fn curve_modules_round_trip_through_json() {
    let size = ParticleSizeOverLifetime {
        curve: Curve::new(vec![
            CurveKey::new(0.0, 0.2).with_interpolation(CurveInterpolation::Smooth),
            CurveKey::new(0.3, 1.0),
            CurveKey::new(1.0, 0.0),
        ]),
        randomness: 0.1,
    };
    let json = serde_json::to_value(&size).unwrap();
    assert!(json.get("start").is_none());
    assert_eq!(
        serde_json::from_value::<ParticleSizeOverLifetime>(json).unwrap(),
        size
    );
    let color = ParticleColorOverLifetime {
        gradient: Gradient::new(vec![
            GradientKey::rgba(0.0, [1.0, 0.9, 0.1, 0.0]),
            GradientKey::rgba(0.2, [1.0, 0.5, 0.0, 1.0]),
            GradientKey::rgba(1.0, [0.3, 0.0, 0.0, 0.0]),
        ]),
    };
    let json = serde_json::to_value(&color).unwrap();
    assert!(json.get("start").is_none());
    assert_eq!(
        serde_json::from_value::<ParticleColorOverLifetime>(json).unwrap(),
        color
    );
}
