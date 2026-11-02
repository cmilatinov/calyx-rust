//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/common/pbr_lighting.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"

// The deferred lighting pass: shades every pixel the geometry pass covered from the G-buffer
// and its depth, through the `pbr_lighting` the forward PBR pass shades with, over the same
// group 0 and group 2 bind groups. Two shaders include this file and add the entry point:
// `shaders/deferred/lighting.wgsl` at one sample, and `shaders/deferred/lighting_msaa.wgsl`
// above one, which also writes the G-buffer's depth into the multisampled scene depth. The
// G-buffer's layout is in `docs/design/rendering/gbuffer.md`.

// Group 1: the G-buffer, single-sample, and the depth the geometry pass drew it against.
@group(1) @binding(0)
var albedo_texture: texture_2d<f32>;

@group(1) @binding(1)
var normals_texture: texture_2d<f32>;

@group(1) @binding(2)
var material_texture: texture_2d<f32>;

@group(1) @binding(3)
var emissive_texture: texture_2d<f32>;

@group(1) @binding(4)
var depth_texture: texture_depth_2d;

// One shaded pixel of the G-buffer.
struct DeferredPixel {
    // Whether the geometry pass drew a surface there. Where it did not, the forward pass or
    // the skybox owns the pixel, and the lighting pass leaves it.
    covered: bool,
    // Linear HDR radiance, opaque.
    color: vec4f,
    // The G-buffer's depth.
    depth: f32,
};

// `view_direction`, a view-space direction, in world space.
fn deferred_world_direction(view_direction: vec3f) -> vec3f {
    return normalize((camera.inverse_view * vec4f(view_direction, 0.0)).xyz);
}

// The world position of the pixel at `position`, framebuffer coordinates, whose depth is
// `depth`: the pixel's center unprojected by the camera that drew the G-buffer.
fn deferred_world_position(position: vec2f, depth: f32) -> vec3f {
    let size = vec2f(textureDimensions(depth_texture));
    let uv = position / size;
    let clip = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let view = camera.inverse_projection * clip;
    return (camera.inverse_view * vec4f(view.xyz / view.w, 1.0)).xyz;
}

// Shades the G-buffer's pixel at `position`, framebuffer coordinates.
fn deferred_shade(position: vec2f) -> DeferredPixel {
    let texel = vec2i(position);
    var pixel: DeferredPixel;
    let albedo = textureLoad(albedo_texture, texel, 0);
    pixel.covered = albedo.a > 0.0;
    if !pixel.covered {
        return pixel;
    }
    pixel.depth = textureLoad(depth_texture, texel, 0);
    let normals = textureLoad(normals_texture, texel, 0);
    let material = textureLoad(material_texture, texel, 0);
    var point: PbrShadingPoint;
    point.world_position = deferred_world_position(position, pixel.depth);
    point.albedo = albedo.rgb;
    point.normal = deferred_world_direction(gbuffer_decode_normal(normals.rg));
    point.geometric_normal = deferred_world_direction(gbuffer_decode_normal(normals.ba));
    point.metallic = material.r;
    point.roughness = material.g;
    point.occlusion = material.b;
    point.emission = textureLoad(emissive_texture, texel, 0).rgb;
    pixel.color = vec4f(pbr_lighting(point), 1.0);
    return pixel;
}
