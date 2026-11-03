//#include "shaders/common/fullscreen_pass.wgsl"

// Distance fog on linear HDR color, before tone mapping. Each pixel fades
// towards the fog colour by its distance from the camera, rebuilt from the
// scene depth. The background, cleared or drawn by the skybox at the far
// plane, takes the fog of the far plane's distance, so distant geometry and
// the sky meet in the same colour. Alpha passes through.

struct FogUniform {
    // Linear HDR fog colour (colour times intensity) in rgb, opacity in a.
    color: vec4f,
    // The camera's clip planes, to turn depth into view distance.
    near_plane: f32,
    far_plane: f32,
    // Tangents of half the horizontal and vertical fields of view.
    tan_half_fov: vec2f,
    // 0: linear, 1: exponential.
    mode: u32,
    start: f32,
    end: f32,
    density: f32,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var depth_texture: texture_depth_2d;

@group(0) @binding(2)
var<uniform> fog: FogUniform;

// Distance from the camera to the surface at `pixel`, along its ray: the
// view depth of a depth-buffer value (`perspective_lh` with OpenGL's [-1, 1]
// depth range, of which the buffer keeps [0, 1]), stretched by the ray's
// slope, so the fog does not change as the camera turns.
fn view_distance(pixel: vec2f, depth: f32) -> f32 {
    let near = fog.near_plane;
    let far = fog.far_plane;
    let z = 2.0 * near * far / (far + near - depth * (far - near));
    let size = vec2f(textureDimensions(input_texture));
    let ndc = vec2f(pixel.x / size.x * 2.0 - 1.0, 1.0 - pixel.y / size.y * 2.0);
    return z * length(vec3f(ndc * fog.tan_half_fov, 1.0));
}

// How much of the fog colour a surface `distance` away takes.
fn fog_amount(distance: f32) -> f32 {
    var thickness: f32;
    if fog.mode == 0u {
        if fog.end <= fog.start {
            thickness = select(0.0, 1.0, distance >= fog.start);
        } else {
            thickness = saturate((distance - fog.start) / (fog.end - fog.start));
        }
    } else {
        thickness = 1.0 - exp(-max(fog.density, 0.0) * distance);
    }
    return thickness * saturate(fog.color.a);
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let texel = textureLoad(input_texture, pixel, 0);
    let depth = textureLoad(depth_texture, pixel, 0);
    let amount = fog_amount(view_distance(in.position.xy, depth));
    return vec4f(mix(texel.rgb, fog.color.rgb, amount), texel.a);
}
