//! [`Curve`]s as `assets/shaders/curve.wgsl` reads them: keys packed one curve
//! after another in one buffer, and a range per curve.

use crate::math::{Curve, CurveInterpolation, CurveKey};

/// `CurveKey` of `curve.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuCurveKey {
    pub time: f32,
    pub value: f32,
    /// One of [`GpuCurveKey::CONSTANT`], [`GpuCurveKey::LINEAR`] or
    /// [`GpuCurveKey::SMOOTH`].
    pub interpolation: u32,
    _padding: u32,
}

impl GpuCurveKey {
    /// `CURVE_CONSTANT` of `curve.wgsl`.
    pub const CONSTANT: u32 = 0;
    /// `CURVE_LINEAR` of `curve.wgsl`.
    pub const LINEAR: u32 = 1;
    /// `CURVE_SMOOTH` of `curve.wgsl`.
    pub const SMOOTH: u32 = 2;
}

impl From<&CurveKey> for GpuCurveKey {
    fn from(key: &CurveKey) -> Self {
        Self {
            time: key.time,
            value: key.value,
            interpolation: match key.interpolation {
                CurveInterpolation::Constant => Self::CONSTANT,
                CurveInterpolation::Linear => Self::LINEAR,
                CurveInterpolation::Smooth => Self::SMOOTH,
            },
            _padding: 0,
        }
    }
}

/// `CurveRange` of `curve.wgsl`: the keys of one curve in a key buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuCurveRange {
    /// Index of the curve's first key.
    pub offset: u32,
    /// Number of keys.
    pub count: u32,
    _padding: [u32; 2],
}

/// The contents of a curve key buffer: curves packed one after another, each
/// with its keys in time order, as [`Curve`] keeps them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuCurveKeys {
    keys: Vec<GpuCurveKey>,
}

impl GpuCurveKeys {
    /// An empty key buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Packs `curves` in order and returns the keys with each curve's range.
    pub fn pack<'a>(curves: impl IntoIterator<Item = &'a Curve>) -> (Self, Vec<GpuCurveRange>) {
        let mut keys = Self::new();
        let ranges = curves.into_iter().map(|curve| keys.push(curve)).collect();
        (keys, ranges)
    }

    /// Appends `curve`'s keys and returns their range.
    pub fn push(&mut self, curve: &Curve) -> GpuCurveRange {
        let range = GpuCurveRange {
            offset: self.keys.len() as u32,
            count: curve.keys.len() as u32,
            _padding: [0; 2],
        };
        self.keys.extend(curve.keys.iter().map(GpuCurveKey::from));
        range
    }

    /// The packed keys.
    pub fn keys(&self) -> &[GpuCurveKey] {
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

static EMPTY_KEY: GpuCurveKey = GpuCurveKey {
    time: 0.0,
    value: 0.0,
    interpolation: 0,
    _padding: 0,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::test_kernel::{run_kernel, shader_source, wgsl_layout};
    use std::mem::offset_of;

    /// A kernel that evaluates one query per thread through `curve.wgsl`.
    /// It includes the module twice, as two files of one shader might.
    const KERNEL: &str = r#"
//#include "shaders/curve.wgsl"
//#include "shaders/curve.wgsl"

struct Query {
    curve: u32,
    time: f32,
};

@group(0) @binding(0) var<storage, read> keys: array<CurveKey>;
@group(0) @binding(1) var<storage, read> ranges: array<CurveRange>;
@group(0) @binding(2) var<storage, read> queries: array<Query>;
@group(0) @binding(3) var<storage, read_write> values: array<f32>;

fn curve_key(index: u32) -> CurveKey {
    return keys[index];
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= arrayLength(&queries) {
        return;
    }
    let query = queries[gid.x];
    values[gid.x] = curve_evaluate(ranges[query.curve], query.time);
}
"#;

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Query {
        curve: u32,
        time: f32,
    }

    #[test]
    fn gpu_curve_types_match_the_layout_of_curve_wgsl() {
        let source = shader_source("curve");
        let stub = "fn curve_key(index: u32) -> CurveKey { return CurveKey(); }";
        let module =
            naga::front::wgsl::parse_str(&format!("{source}\n{stub}")).expect("curve.wgsl parses");

        let (size, members) = wgsl_layout(&module, "CurveKey");
        assert_eq!(size as usize, size_of::<GpuCurveKey>());
        assert_eq!(
            members[..3],
            [
                ("time".into(), offset_of!(GpuCurveKey, time) as u32),
                ("value".into(), offset_of!(GpuCurveKey, value) as u32),
                (
                    "interpolation".into(),
                    offset_of!(GpuCurveKey, interpolation) as u32
                ),
            ]
        );
        let (size, members) = wgsl_layout(&module, "CurveRange");
        assert_eq!(size as usize, size_of::<GpuCurveRange>());
        assert_eq!(
            members[..2],
            [
                ("offset".into(), offset_of!(GpuCurveRange, offset) as u32),
                ("count".into(), offset_of!(GpuCurveRange, count) as u32),
            ]
        );
        assert_eq!(size_of::<GpuCurveKey>(), 16);
        assert_eq!(size_of::<GpuCurveRange>(), 16);
    }

    #[test]
    fn packing_appends_each_curve_after_the_last() {
        let first = Curve::linear(0.0, 1.0);
        let second = Curve::new(vec![
            CurveKey::new(0.5, 3.0).with_interpolation(CurveInterpolation::Smooth)
        ]);
        let empty = Curve::new(Vec::new());
        let (keys, ranges) = GpuCurveKeys::pack([&first, &empty, &second]);
        let spans: Vec<(u32, u32)> = ranges.iter().map(|r| (r.offset, r.count)).collect();
        assert_eq!(spans, [(0, 2), (2, 0), (2, 1)]);
        assert_eq!(keys.keys()[2].time, 0.5);
        assert_eq!(keys.keys()[2].value, 3.0);
        assert_eq!(keys.keys()[2].interpolation, GpuCurveKey::SMOOTH);
        assert_eq!(keys.contents().len(), 3 * 16);
        assert_eq!(GpuCurveKeys::new().contents().len(), 16);
    }

    /// Curves covering every interpolation, empty and one-key curves, and
    /// keys that share a time: first, in the middle and all of them.
    fn curves() -> Vec<Curve> {
        let key = CurveKey::new;
        let constant = CurveInterpolation::Constant;
        let smooth = CurveInterpolation::Smooth;
        vec![
            Curve::new(Vec::new()),
            Curve::new(vec![key(0.3, 7.0)]),
            Curve::linear(0.6, 0.0),
            Curve::new(vec![
                key(0.0, 0.0),
                key(0.5, 2.0).with_interpolation(constant),
                key(0.75, 4.0).with_interpolation(smooth),
                key(1.0, 0.0),
            ]),
            Curve::new(vec![
                key(0.1, -1.5).with_interpolation(smooth),
                key(0.35, 3.25).with_interpolation(smooth),
                key(0.8, 0.125).with_interpolation(constant),
                key(0.95, 9.0),
            ]),
            Curve::new(vec![key(0.0, 1.0), key(0.0, 3.0), key(1.0, 3.0)]),
            Curve::new(vec![
                key(0.0, 0.0),
                key(0.5, 1.0),
                key(0.5, 3.0),
                key(1.0, 3.0),
            ]),
            Curve::new(vec![key(0.5, 1.0), key(0.5, 2.0)]),
            Curve::new(vec![key(0.2, 1.0), key(0.8, 3.0)]),
        ]
    }

    /// Times before, on, between and after every curve's keys.
    fn times(curves: &[Curve]) -> Vec<f32> {
        let mut times = vec![f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -1.0, 2.0];
        times.extend((0..=64).map(|step| -0.25 + step as f32 * 1.5 / 64.0));
        for curve in curves {
            for pair in curve.keys.windows(2) {
                times.push(pair[0].time);
                times.push(pair[0].time + (pair[1].time - pair[0].time) * 0.3);
            }
            times.extend(curve.keys.last().map(|key| key.time));
        }
        times
    }

    /// `curve.wgsl` evaluates each curve as [`Curve::evaluate`] does: bit for
    /// bit wherever the value is a key's (outside the keys, on a key, on a
    /// constant segment, between keys at one time, an empty curve), and
    /// within 4 ulp of the segment's larger key value between keys, where
    /// the GPU's division may round differently (WGSL allows 2.5 ulp).
    #[test]
    fn curve_wgsl_evaluates_curves_as_the_cpu_does() {
        let curves = curves();
        let times = times(&curves);
        let (keys, ranges) = GpuCurveKeys::pack(&curves);
        let queries: Vec<Query> = (0..curves.len())
            .flat_map(|curve| {
                times.iter().map(move |&time| Query {
                    curve: curve as u32,
                    time,
                })
            })
            .collect();
        let values: Vec<f32> = run_kernel(
            KERNEL,
            &[
                keys.contents(),
                bytemuck::cast_slice(&ranges),
                bytemuck::cast_slice(&queries),
            ],
            queries.len(),
        );

        let mut interpolated = 0;
        for (query, gpu) in queries.iter().zip(values) {
            let curve = &curves[query.curve as usize];
            let segment = curve.segment(query.time);
            let cpu = curve.evaluate(query.time);
            let label = format!("curve {} at {}: {curve:?}", query.curve, query.time);
            if segment.weight == 0.0 {
                assert_eq!(
                    gpu.to_bits(),
                    cpu.to_bits(),
                    "{label}: GPU {gpu}, CPU {cpu}"
                );
            } else {
                interpolated += 1;
                let ulp = f32::EPSILON * segment.start.abs().max(segment.end.abs());
                assert!(
                    (gpu - cpu).abs() <= 4.0 * ulp,
                    "{label}: GPU {gpu}, CPU {cpu}"
                );
            }
        }
        assert!(
            interpolated > 100,
            "only {interpolated} queries interpolate"
        );
    }
}
