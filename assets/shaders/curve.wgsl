// Curve evaluation on the GPU, matching `Curve::evaluate` on the CPU
// (`crates/calyx_engine/src/math/curve.rs`) and `GpuCurveKey` / `GpuCurveRange`
// (`crates/calyx_engine/src/render/gpu_curve.rs`).
//
// Include it with `//#include "shaders/curve.wgsl"`. It declares no bindings:
// it reads its keys from the parameter `curve_storage`, curves packed one
// after another with each curve's keys sorted by time, and a `CurveRange`
// names one curve's keys in it. The including shader binds the parameter by
// marking its key buffer:
//
//     //#binding curve_storage
//     @group(0) @binding(10) var<storage, read> curve_keys: array<CurveKey>;

//#param curve_storage: storage<array<CurveKey>>

// How a curve moves from a key to the next one: `CurveInterpolation`.
const CURVE_CONSTANT: u32 = 0u;
const CURVE_LINEAR: u32 = 1u;
const CURVE_SMOOTH: u32 = 2u;

// One key of a curve, 16 bytes so arrays of keys fit uniform layout too.
struct CurveKey {
    time: f32,
    value: f32,
    // One of the `CURVE_*` interpolation constants.
    interpolation: u32,
    _padding: u32,
};

// The keys of one curve: `count` keys from key `offset` on. 16 bytes so a
// range can sit in a uniform buffer, alone or in an array.
struct CurveRange {
    offset: u32,
    count: u32,
    _padding: vec2u,
};

// Where a time falls on a curve: the values of the keys around it, and how
// far it is from `start` to `end`, eased by the interpolation. Outside the
// keys, on a key, and between keys at the same time, `start` and `end` are the
// value the curve takes and `weight` is 0.0.
struct CurveSegment {
    start: f32,
    end: f32,
    weight: f32,
};

// Key `index` of the curve key buffer.
fn curve_key(index: u32) -> CurveKey {
    return #curve_storage[index];
}

fn curve_is_nan(value: f32) -> bool {
    // A bit test, since shader compilers may fold `value != value` to false.
    return (bitcast<u32>(value) & 0x7fffffffu) > 0x7f800000u;
}

// The segment of `range`'s curve that `time` falls in.
fn curve_segment(range: CurveRange, time: f32) -> CurveSegment {
    if range.count == 0u {
        return CurveSegment(0.0, 0.0, 0.0);
    }
    let first = curve_key(range.offset);
    if curve_is_nan(time) || time < first.time {
        return CurveSegment(first.value, first.value, 0.0);
    }
    let last = curve_key(range.offset + range.count - 1u);
    if time >= last.time {
        return CurveSegment(last.value, last.value, 0.0);
    }
    // The first key after `time`, as `partition_point` finds it on the CPU.
    // It lies in [1, count - 1]: the first key is at or before `time`, and
    // the last one after it.
    var low = 0u;
    var high = range.count;
    while low < high {
        let middle = (low + high) / 2u;
        if curve_key(range.offset + middle).time <= time {
            low = middle + 1u;
        } else {
            high = middle;
        }
    }
    let previous = curve_key(range.offset + low - 1u);
    let next = curve_key(range.offset + low);
    let span = next.time - previous.time;
    if span <= 0.0 {
        return CurveSegment(next.value, next.value, 0.0);
    }
    var t = (time - previous.time) / span;
    switch previous.interpolation {
        case CURVE_CONSTANT: {
            t = 0.0;
        }
        case CURVE_SMOOTH: {
            t = t * t * (3.0 - 2.0 * t);
        }
        default: {}
    }
    return CurveSegment(previous.value, next.value, t);
}

// The value a segment stands for.
fn curve_segment_value(segment: CurveSegment) -> f32 {
    return segment.start + (segment.end - segment.start) * segment.weight;
}

// The value of `range`'s curve at `time`: the first key's value before it,
// the last key's after it and for a NaN time, 0.0 for an empty curve.
fn curve_evaluate(range: CurveRange, time: f32) -> f32 {
    return curve_segment_value(curve_segment(range, time));
}
