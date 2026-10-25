// Gradient evaluation on the GPU, matching `Gradient::evaluate` on the CPU
// (`engine/src/math/gradient.rs`) and `GpuGradientKey` / `GpuGradientRange`
// (`engine/src/render/gpu_gradient.rs`).
//
// Include it with `//#include "shaders/gradient.wgsl"`; it sits next to
// `curve.wgsl` without clashing. It declares no bindings: it reads its keys
// from the parameter `gradient_storage`, gradients packed one after another
// with each gradient's keys sorted by time, and a `GradientRange` names one
// gradient's keys in it. The including shader binds the parameter by marking
// its key buffer:
//
//     //#binding gradient_storage
//     @group(0) @binding(11) var<storage, read> gradient_keys: array<GradientKey>;

//#param gradient_storage: storage<array<GradientKey>>

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

// Key `index` of the gradient key buffer.
fn gradient_key(index: u32) -> GradientKey {
    return #gradient_storage[index];
}

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
