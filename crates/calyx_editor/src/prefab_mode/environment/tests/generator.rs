use image::Rgb;
use nalgebra_glm::Vec3;

/// The HDRI's file inside the built-in asset root, `assets/`.
pub(super) const PREFAB_ENVIRONMENT_FILE: &str = "skyboxes/prefab_studio.hdr";
/// Width of the generated HDRI; its height is half of it.
const HDRI_WIDTH: usize = 256;
/// Radiance straight up: a bright, slightly cool sky.
const ZENITH: [f32; 3] = [1.15, 1.2, 1.3];
/// Radiance at the horizon: neutral mid-gray.
const HORIZON: [f32; 3] = [0.5, 0.5, 0.5];
/// Radiance straight down: a dark, slightly warm floor.
const NADIR: [f32; 3] = [0.1, 0.095, 0.09];
/// Elevation of the key, the HDRI's bright spot that gives the prefab a
/// shading direction.
const KEY_ELEVATION_DEG: f32 = 45.0;
/// Azimuth of the key, from +X towards +Z.
const KEY_AZIMUTH_DEG: f32 = 45.0;
/// Peak radiance of the key's soft blob, on top of the sky.
const KEY_PEAK: [f32; 3] = [30.0, 28.5, 25.5];
/// Angular falloff of the key blob, as `1 - cos` of the angle at which it
/// drops to `1/e`: about 11 degrees.
const KEY_SPREAD: f32 = 0.02;

/// The direction towards the key, from the prefab: up and to the side at
/// [`KEY_ELEVATION_DEG`] and [`KEY_AZIMUTH_DEG`].
pub(super) fn key_direction() -> Vec3 {
    direction(KEY_ELEVATION_DEG.to_radians(), KEY_AZIMUTH_DEG.to_radians())
}

/// The world direction at `elevation` above the horizon and `azimuth`
/// around it, as the engine's environment shaders map an equirectangular
/// image: azimuth runs from the +X axis towards +Z.
fn direction(elevation: f32, azimuth: f32) -> Vec3 {
    Vec3::new(
        elevation.cos() * azimuth.cos(),
        elevation.sin(),
        elevation.cos() * azimuth.sin(),
    )
}

/// The studio environment's radiance towards `direction`.
pub(super) fn studio_radiance(direction: Vec3) -> [f32; 3] {
    let up = direction.y.clamp(-1.0, 1.0);
    let (to, t) = if up >= 0.0 {
        (ZENITH, smoothstep(up.sqrt()))
    } else {
        (NADIR, smoothstep((-up).sqrt()))
    };
    let key = ((direction.dot(&key_direction()) - 1.0) / KEY_SPREAD).exp();
    std::array::from_fn(|channel| {
        HORIZON[channel] + (to[channel] - HORIZON[channel]) * t + KEY_PEAK[channel] * key
    })
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The studio HDRI as an equirectangular image, row by row from the top,
/// `HDRI_WIDTH` by half of it.
pub(super) fn studio_hdri() -> (Vec<Rgb<f32>>, usize, usize) {
    let (width, height) = (HDRI_WIDTH, HDRI_WIDTH / 2);
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let elevation = (0.5 - (y as f32 + 0.5) / height as f32) * std::f32::consts::PI;
            let azimuth = ((x as f32 + 0.5) / width as f32 - 0.5) * std::f32::consts::TAU;
            Rgb(studio_radiance(direction(elevation, azimuth)))
        })
        .collect();
    (pixels, width, height)
}
