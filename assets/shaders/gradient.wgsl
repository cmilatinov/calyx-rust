// Gradient evaluation on the GPU, matching `Gradient::evaluate` on the CPU
// (`engine/src/math/gradient.rs`) and `GpuGradientKey` / `GpuGradientRange`
// (`engine/src/render/gpu_gradient.rs`).
//
// Include it with `//#include "shaders/gradient.wgsl"`; it sits next to
// `curve.wgsl` without clashing. It declares no bindings: the including
// shader keeps its gradient keys wherever it likes, packed one gradient after
// another with each gradient's keys sorted by time, and defines
//
//     fn gradient_key(index: u32) -> GradientKey
//
// returning key `index` of that storage. A `GradientRange` names one
// gradient's keys in it. WGSL cannot pass a storage pointer to a function,
// so the keys reach this module through `gradient_key` rather than as an
// argument.

// One color key of a gradient: unmultiplied sRGB RGBA at `time`. 32 bytes,
// so arrays of keys fit uniform layout too.
struct GradientKey {
    color: vec4f,
    time: f32,
    _padding0: u32,
    _padding1: u32,
    _padding2: u32,
};

// The keys of one gradient: `count` keys from key `offset` on. 16 bytes so a
// range can sit in a uniform buffer, alone or in an array.
struct GradientRange {
    offset: u32,
    count: u32,
    _padding: vec2u,
};

// The color of an empty gradient.
const GRADIENT_EMPTY_COLOR: vec4f = vec4f(1.0, 1.0, 1.0, 1.0);

fn gradient_is_nan(value: f32) -> bool {
    // A bit test, since shader compilers may fold `value != value` to false.
    return (bitcast<u32>(value) & 0x7fffffffu) > 0x7f800000u;
}

// The color of `range`'s gradient at `time`, unmultiplied sRGB RGBA: the
// first key's color before it and for a NaN time, the last key's after it,
// linear between keys, opaque white for an empty gradient.
fn gradient_evaluate(range: GradientRange, time: f32) -> vec4f {
    if range.count == 0u {
        return GRADIENT_EMPTY_COLOR;
    }
    let first = gradient_key(range.offset);
    if gradient_is_nan(time) || time < first.time {
        return first.color;
    }
    let last = gradient_key(range.offset + range.count - 1u);
    if time >= last.time {
        return last.color;
    }
    // The first key after `time`, as `partition_point` finds it on the CPU.
    var low = 0u;
    var high = range.count;
    while low < high {
        let middle = (low + high) / 2u;
        if gradient_key(range.offset + middle).time <= time {
            low = middle + 1u;
        } else {
            high = middle;
        }
    }
    let previous = gradient_key(range.offset + low - 1u);
    let next = gradient_key(range.offset + low);
    let span = next.time - previous.time;
    if span <= 0.0 {
        return next.color;
    }
    let t = (time - previous.time) / span;
    return previous.color + (next.color - previous.color) * t;
}
