//#include "shaders/inputs.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
};

struct OutlineUniform {
    target_object_id: u32,
    outline_radius: u32,
    _padding: vec2u,
    outline_color: vec4f,
};

@group(0) @binding(0)
var object_ids: texture_2d<u32>;

@group(0) @binding(1)
var<uniform> outline: OutlineUniform;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = vec4f(vertex.position, 1.0);
    return out;
}

fn load_object_id(pixel: vec2i, size: vec2i) -> u32 {
    let clamped = clamp(pixel, vec2i(0, 0), size - vec2i(1, 1));
    return textureLoad(object_ids, clamped, 0).r;
}

fn has_neighbor(pixel: vec2i, size: vec2i, target_id: u32, radius: u32) -> bool {
    if (target_id == 0u) {
        return false;
    }

    let radius_i32 = i32(radius);
    for (var y = -radius_i32; y <= radius_i32; y += 1) {
        for (var x = -radius_i32; x <= radius_i32; x += 1) {
            if (x == 0 && y == 0) {
                continue;
            }
            if (load_object_id(pixel + vec2i(x, y), size) == target_id) {
                return true;
            }
        }
    }
    return false;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let size = vec2i(textureDimensions(object_ids));
    let pixel = clamp(vec2i(in.position.xy), vec2i(0, 0), size - vec2i(1, 1));
    let center_id = load_object_id(pixel, size);

    if (center_id == outline.target_object_id) {
        return vec4f(0.0);
    }

    if (has_neighbor(pixel, size, outline.target_object_id, outline.outline_radius)) {
        return outline.outline_color;
    }

    return vec4f(0.0);
}
