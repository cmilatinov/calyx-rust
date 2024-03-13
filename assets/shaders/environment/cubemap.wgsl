struct VertexIn {
    @location(0) position: vec3f,
};

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) world_position: vec3f,
};

struct FragmentOut {
    @location(0) positive_x: vec4f,
    @location(1) negative_x: vec4f,
    @location(2) positive_y: vec4f,
    @location(3) negative_y: vec4f,
    @location(4) positive_z: vec4f,
    @location(5) negative_z: vec4f,
};

const CUBE_NUM_FACES = 6u;

@group(0) @binding(0)
var skybox_texture: texture_2d<f32>;

@group(0) @binding(1)
var skybox_sampler: sampler;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {     
    var out: VertexOut;
    out.world_position = vertex.position;
    out.position = vec4f(out.world_position, 1.0);
    return out;
}

fn sample_spherical_map(v: vec3f) -> vec2f {
    var uv = vec2f(atan2(v.z, v.x), asin(v.y));
    uv *= vec2f(0.1591, 0.3183);
    uv += vec2f(0.5);
    return uv;
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    var CUBE_FACE_TRANSFORMS = array(
        mat4x4f(
            vec4f(0.0, 0.0, -1.0, 0.0),
            vec4f(0.0, 1.0, 0.0, 0.0),
            vec4f(-1.0, 0.0, 0.0, 0.0),
            vec4f(1.0, 0.0, 0.0, 1.0)
        ),
        mat4x4f(
            vec4f(0.0, 0.0, 1.0, 0.0),
            vec4f(0.0, 1.0, 0.0, 0.0),
            vec4f(-1.0, 0.0, 0.0, 0.0),
            vec4f(-1.0, 0.0, 0.0, 1.0)
        ),
        mat4x4f(
            vec4f(1.0, 0.0, 0.0, 0.0),
            vec4f(0.0, 0.0, -1.0, 0.0),
            vec4f(0.0, 1.0, 0.0, 0.0),
            vec4f(0.0, 1.0, 0.0, 1.0)
        ),
        mat4x4f(
            vec4f(1.0, 0.0, 0.0, 0.0),
            vec4f(0.0, 0.0, 1.0, 0.0),
            vec4f(0.0, 1.0, 0.0, 0.0),
            vec4f(0.0, -1.0, 0.0, 1.0)
        ),
        mat4x4f(
            vec4f(1.0, 0.0, 0.0, 0.0),
            vec4f(0.0, 1.0, 0.0, 0.0),
            vec4f(0.0, 0.0, 1.0, 0.0),
            vec4f(0.0, 0.0, 1.0, 1.0)
        ),
        mat4x4f(
            vec4f(-1.0, 0.0, 0.0, 0.0),
            vec4f(0.0, 1.0, 0.0, 0.0),
            vec4f(0.0, 0.0, 1.0, 0.0),
            vec4f(0.0, 0.0, -1.0, 1.0)
        )
    );
    var colors = array<vec4f, CUBE_NUM_FACES>();
    for (var i = 0u; i < CUBE_NUM_FACES; i++) {
        var v = normalize((CUBE_FACE_TRANSFORMS[i] * vec4f(in.world_position, 1.0)).xyz);
        var uv = sample_spherical_map(v);
        uv.y = 1.0 - uv.y;
        let color = textureSample(skybox_texture, skybox_sampler, uv);
        colors[i] = color;
    }
    var out: FragmentOut;
    out.positive_x = colors[0];
    out.negative_x = colors[1];
    out.positive_y = colors[2];
    out.negative_y = colors[3];
    out.positive_z = colors[4];
    out.negative_z = colors[5];
    return out;
}