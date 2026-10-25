//! [`Gradient`]s as `assets/shaders/gradient.wgsl` reads them: keys packed
//! one gradient after another in one buffer, and a range per gradient.

use crate::math::{Gradient, GradientKey};

/// `GradientKey` of `gradient.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuGradientKey {
    /// Unmultiplied sRGB RGBA.
    pub color: [f32; 4],
    pub time: f32,
    _padding: [u32; 3],
}

impl From<&GradientKey> for GpuGradientKey {
    fn from(key: &GradientKey) -> Self {
        Self {
            color: key.color.into(),
            time: key.time,
            _padding: [0; 3],
        }
    }
}

/// `GradientRange` of `gradient.wgsl`: the keys of one gradient in a key
/// buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuGradientRange {
    /// Index of the gradient's first key.
    pub offset: u32,
    /// Number of keys.
    pub count: u32,
    _padding: [u32; 2],
}

/// The contents of a gradient key buffer: gradients packed one after
/// another, each with its keys in time order, as [`Gradient`] keeps them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuGradientKeys {
    keys: Vec<GpuGradientKey>,
}

impl GpuGradientKeys {
    /// An empty key buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Packs `gradients` in order and returns the keys with each gradient's
    /// range.
    pub fn pack<'a>(
        gradients: impl IntoIterator<Item = &'a Gradient>,
    ) -> (Self, Vec<GpuGradientRange>) {
        let mut keys = Self::new();
        let ranges = gradients
            .into_iter()
            .map(|gradient| keys.push(gradient))
            .collect();
        (keys, ranges)
    }

    /// Appends `gradient`'s keys and returns their range.
    pub fn push(&mut self, gradient: &Gradient) -> GpuGradientRange {
        let range = GpuGradientRange {
            offset: self.keys.len() as u32,
            count: gradient.keys.len() as u32,
            _padding: [0; 2],
        };
        self.keys
            .extend(gradient.keys.iter().map(GpuGradientKey::from));
        range
    }

    /// The packed keys.
    pub fn keys(&self) -> &[GpuGradientKey] {
        &self.keys
    }

    /// The buffer contents: the packed keys, or one zeroed key when there are
    /// none, since a bound buffer cannot be empty.
    pub fn contents(&self) -> &[u8] {
        if self.keys.is_empty() {
            bytemuck::bytes_of(&EMPTY_KEY)
        } else {
            bytemuck::cast_slice(&self.keys)
        }
    }
}

static EMPTY_KEY: GpuGradientKey = GpuGradientKey {
    color: [0.0; 4],
    time: 0.0,
    _padding: [0; 3],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::test_kernel::{run_kernel, shader_source, wgsl_layout};
    use std::mem::offset_of;

    /// A kernel that evaluates one query per thread through `gradient.wgsl`,
    /// included next to `curve.wgsl` as a shader using both would.
    const KERNEL: &str = r#"
//#include "shaders/curve.wgsl"
//#include "shaders/gradient.wgsl"

struct Query {
    gradient: u32,
    time: f32,
};

@group(0) @binding(0) var<storage, read> keys: array<GradientKey>;
@group(0) @binding(1) var<storage, read> ranges: array<GradientRange>;
@group(0) @binding(2) var<storage, read> queries: array<Query>;
@group(0) @binding(3) var<storage, read_write> colors: array<vec4f>;

fn gradient_key(index: u32) -> GradientKey {
    return keys[index];
}

// This kernel evaluates no curve; `curve.wgsl` still needs its key source.
fn curve_key(index: u32) -> CurveKey {
    return CurveKey();
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= arrayLength(&queries) {
        return;
    }
    let query = queries[gid.x];
    colors[gid.x] = gradient_evaluate(ranges[query.gradient], query.time);
}
"#;

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Query {
        gradient: u32,
        time: f32,
    }

    #[test]
    fn gpu_gradient_types_match_the_layout_of_gradient_wgsl() {
        let source = shader_source("gradient");
        let stub = "fn gradient_key(index: u32) -> GradientKey { return GradientKey(); }";
        let module = naga::front::wgsl::parse_str(&format!("{source}\n{stub}"))
            .expect("gradient.wgsl parses");

        let (size, members) = wgsl_layout(&module, "GradientKey");
        assert_eq!(size as usize, size_of::<GpuGradientKey>());
        assert_eq!(
            members[..2],
            [
                ("color".into(), offset_of!(GpuGradientKey, color) as u32),
                ("time".into(), offset_of!(GpuGradientKey, time) as u32),
            ]
        );
        let (size, members) = wgsl_layout(&module, "GradientRange");
        assert_eq!(size as usize, size_of::<GpuGradientRange>());
        assert_eq!(
            members[..2],
            [
                ("offset".into(), offset_of!(GpuGradientRange, offset) as u32),
                ("count".into(), offset_of!(GpuGradientRange, count) as u32),
            ]
        );
        assert_eq!(size_of::<GpuGradientKey>(), 32);
        assert_eq!(size_of::<GpuGradientRange>(), 16);
    }

    #[test]
    fn packing_appends_each_gradient_after_the_last() {
        let first = Gradient::default();
        let empty = Gradient::new(Vec::new());
        let second = Gradient::new(vec![GradientKey::rgba(0.5, [0.1, 0.2, 0.3, 0.4])]);
        let (keys, ranges) = GpuGradientKeys::pack([&first, &empty, &second]);
        let spans: Vec<(u32, u32)> = ranges.iter().map(|r| (r.offset, r.count)).collect();
        assert_eq!(spans, [(0, 2), (2, 0), (2, 1)]);
        assert_eq!(keys.keys()[2].time, 0.5);
        assert_eq!(keys.keys()[2].color, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(keys.contents().len(), 3 * 32);
        assert_eq!(GpuGradientKeys::new().contents().len(), 32);
    }

    /// Gradients covering empty and one-key gradients, several keys, and
    /// keys that share a time: first, in the middle and all of them.
    fn gradients() -> Vec<Gradient> {
        let key = GradientKey::rgba;
        vec![
            Gradient::new(Vec::new()),
            Gradient::new(vec![key(0.3, [0.2, 0.4, 0.6, 0.8])]),
            Gradient::default(),
            Gradient::new(vec![
                key(0.0, [1.0, 0.843, 0.118, 1.0]),
                key(0.35, [1.0, 0.51, 0.0, 1.0]),
                key(0.6, [0.62, 0.071, 0.039, 0.9]),
                key(1.0, [0.3, 0.05, 0.02, 0.0]),
            ]),
            Gradient::new(vec![
                key(0.1, [0.0, 0.0, 0.0, 0.0]),
                key(0.15, [0.267, 0.196, 0.173, 0.922]),
                key(0.95, [0.1, 0.1, 0.1, 0.0]),
            ]),
            Gradient::new(vec![
                key(0.0, [0.0; 4]),
                key(0.0, [1.0; 4]),
                key(1.0, [0.5; 4]),
            ]),
            Gradient::new(vec![
                key(0.0, [0.0; 4]),
                key(0.5, [1.0; 4]),
                key(0.5, [0.25, 0.5, 0.75, 1.0]),
                key(1.0, [0.0; 4]),
            ]),
            Gradient::new(vec![key(0.5, [0.0; 4]), key(0.5, [1.0; 4])]),
        ]
    }

    /// Times before, on, between and after every gradient's keys.
    fn times(gradients: &[Gradient]) -> Vec<f32> {
        let mut times = vec![f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -1.0, 2.0];
        times.extend((0..=64).map(|step| -0.25 + step as f32 * 1.5 / 64.0));
        for gradient in gradients {
            for pair in gradient.keys.windows(2) {
                times.push(pair[0].time);
                times.push(pair[0].time + (pair[1].time - pair[0].time) * 0.3);
            }
            times.extend(gradient.keys.last().map(|key| key.time));
        }
        times
    }

    /// Whether `time` falls strictly between two keys of `gradient` with
    /// different times, where the color is blended rather than a key's.
    fn blends(gradient: &Gradient, time: f32) -> bool {
        gradient
            .keys
            .windows(2)
            .any(|pair| pair[0].time < pair[1].time && time > pair[0].time && time < pair[1].time)
    }

    /// `gradient.wgsl` evaluates each gradient as [`Gradient::evaluate`]
    /// does, under the rule of the curve test: bit for bit wherever the color
    /// is a key's (outside the keys, on a key, between keys at one time, an
    /// empty gradient), and within 4 ulp of the segment's larger key channel
    /// between keys, where the GPU's division may round differently (WGSL
    /// allows 2.5 ulp).
    #[test]
    fn gradient_wgsl_evaluates_gradients_as_the_cpu_does() {
        let gradients = gradients();
        let times = times(&gradients);
        let (keys, ranges) = GpuGradientKeys::pack(&gradients);
        let queries: Vec<Query> = (0..gradients.len())
            .flat_map(|gradient| {
                times.iter().map(move |&time| Query {
                    gradient: gradient as u32,
                    time,
                })
            })
            .collect();
        let colors: Vec<[f32; 4]> = run_kernel(
            KERNEL,
            &[
                keys.contents(),
                bytemuck::cast_slice(&ranges),
                bytemuck::cast_slice(&queries),
            ],
            queries.len(),
        );

        let mut blended = 0;
        for (query, gpu) in queries.iter().zip(colors) {
            let gradient = &gradients[query.gradient as usize];
            let cpu: [f32; 4] = gradient.evaluate(query.time).into();
            let label = format!(
                "gradient {} at {}: {gradient:?}",
                query.gradient, query.time
            );
            if !blends(gradient, query.time) {
                assert_eq!(
                    gpu.map(f32::to_bits),
                    cpu.map(f32::to_bits),
                    "{label}: GPU {gpu:?}, CPU {cpu:?}"
                );
                continue;
            }
            blended += 1;
            let next = gradient.keys.partition_point(|key| key.time <= query.time);
            let (from, to) = (gradient.keys[next - 1].color, gradient.keys[next].color);
            for channel in 0..4 {
                let ulp = f32::EPSILON * from[channel].abs().max(to[channel].abs());
                assert!(
                    (gpu[channel] - cpu[channel]).abs() <= 4.0 * ulp,
                    "{label}: channel {channel}: GPU {gpu:?}, CPU {cpu:?}"
                );
            }
        }
        assert!(blended > 100, "only {blended} queries blend");
    }
}
