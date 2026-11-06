//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/postprocess/camera_planes.wgsl"

// Fog of war on display-linear color, after tone mapping. Each pixel is fogged by the ground
// position (x, z) of the surface it shows, rebuilt from the scene depth: a viewer sees it when
// it lies closer than the reach of the viewer's sight rays either side of it, as much as the
// viewer's distance fade leaves, and the side sees it as well as its best viewer does. What the
// side does not see mixes towards the fog colour in sRGB-encoded colour, as a translucent overlay
// drawn over the image would. `ComponentFogOfWar` and `FogOfWarViewer` in
// `crates/calyx_engine/src/component/post_process/fog_of_war.rs` hold the same rules on the CPU.
// Alpha passes through.

struct FogOfWarUniform {
    // sRGB-encoded fog colour in rgb, opacity in a.
    color: vec4f,
    // The camera, to turn depth into view-space positions.
    camera: CameraPlanes,
    // From the camera's view space to world space.
    view_to_world: mat4x4f,
    viewer_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

// One viewer, as `GpuViewer` in `crates/calyx_engine/src/render/postprocess/fog_of_war.rs`.
struct Viewer {
    // Ground position (x, z).
    position: vec2f,
    fade_start: f32,
    fade_end: f32,
    fade_curve: f32,
    shadow_jump: f32,
    // The viewer's rays in `reach`: `ray_count` of them from `first_ray`.
    first_ray: u32,
    ray_count: u32,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var depth_texture: texture_depth_2d;

@group(0) @binding(2)
var<uniform> fog: FogOfWarUniform;

@group(0) @binding(3)
var<storage, read> viewers: array<Viewer>;

@group(0) @binding(4)
var<storage, read> reach: array<f32>;

const TAU: f32 = 6.283185307179586;

// How much `viewer` sees at `distance` from it, cover aside.
fn range_fade(viewer: Viewer, distance: f32) -> f32 {
    var t: f32;
    if viewer.fade_end <= viewer.fade_start {
        t = select(0.0, 1.0, distance >= viewer.fade_start);
    } else {
        t = saturate((distance - viewer.fade_start) / (viewer.fade_end - viewer.fade_start));
    }
    let k = viewer.fade_curve;
    if abs(k) < 1e-4 {
        return 1.0 - t;
    }
    return 1.0 - (exp(k * t) - 1.0) / (exp(k) - 1.0);
}

// How far `viewer` sees in the ground direction `offset`: the nearer ray's reach across a shadow
// side, where the two rays either side differ by more than the viewer's shadow jump, else the
// two interpolated.
fn reach_at(viewer: Viewer, offset: vec2f) -> f32 {
    let count = viewer.ray_count;
    if count == 0u {
        return 3.0e38;
    }
    let rays = f32(count);
    var turns = atan2(offset.y, offset.x) / TAU * rays;
    turns = turns - floor(turns / rays) * rays;
    let index = min(u32(floor(turns)), count - 1u);
    let next = (index + 1u) % count;
    let blend = turns - floor(turns);
    let a = reach[viewer.first_ray + index];
    let b = reach[viewer.first_ray + next];
    if abs(a - b) > viewer.shadow_jump {
        return select(a, b, blend >= 0.5);
    }
    return mix(a, b, blend);
}

// How much of the ground point `point` the side sees: the most any viewer sees of it.
fn visibility(point: vec2f) -> f32 {
    var seen = 0.0;
    for (var index = 0u; index < fog.viewer_count; index++) {
        let viewer = viewers[index];
        let offset = point - viewer.position;
        let distance = length(offset);
        let fade = range_fade(viewer, distance);
        if fade > seen && distance < reach_at(viewer, offset) {
            seen = fade;
        }
    }
    return seen;
}

fn linear_to_srgb(value: vec3f) -> vec3f {
    let low = value * 12.92;
    let high = 1.055 * pow(value, vec3f(1.0 / 2.4)) - 0.055;
    return select(high, low, value <= vec3f(0.0031308));
}

fn srgb_to_linear(value: vec3f) -> vec3f {
    let low = value / 12.92;
    let high = pow((value + 0.055) / 1.055, vec3f(2.4));
    return select(high, low, value <= vec3f(0.04045));
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let texel = textureLoad(input_texture, pixel, 0);
    let depth = textureLoad(depth_texture, pixel, 0);
    let ndc = camera_ndc(in.position.xy / vec2f(textureDimensions(input_texture)));
    let view = camera_view_position(fog.camera, ndc, camera_linear_depth(fog.camera, depth));
    let world = (fog.view_to_world * vec4f(view, 1.0)).xyz;
    let amount = saturate(fog.color.a) * (1.0 - visibility(world.xz));
    if amount <= 0.0 {
        return texel;
    }
    let encoded = linear_to_srgb(saturate(texel.rgb));
    let fogged = srgb_to_linear(mix(encoded, fog.color.rgb, amount));
    return vec4f(fogged, texel.a);
}
