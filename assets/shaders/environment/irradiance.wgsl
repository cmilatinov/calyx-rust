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
const PI = 3.14159265359;

@group(0) @binding(0)
var skybox_texture: texture_cube<f32>;

@group(0) @binding(1)
var skybox_sampler: sampler;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.world_position = vertex.position;
    out.position = vec4f(out.world_position, 1.0);
    return out;
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
        var normal = normalize((CUBE_FACE_TRANSFORMS[i] * vec4f(in.world_position, 1.0)).xyz);
        var irradiance = vec3f(0.0);
        var up = vec3f(0.0, 1.0, 0.0);
        let right = normalize(cross(up, normal));
        up = normalize(cross(normal, right)); 
        let delta = 0.025;
        var num_samples = 0u;
        for (var phi = 0.0; phi < 2.0 * PI; phi += delta) {
            for (var theta = 0.0; theta < 0.5 * PI; theta += delta) {
                let tangent_sample = vec3f(
                    sin(theta) * cos(phi), 
                    sin(theta) * sin(phi), 
                    cos(theta)
                );
                let uvw = normalize(
                    tangent_sample.x * right + 
                    tangent_sample.y * up + 
                    tangent_sample.z * normal
                );
                irradiance += 
                    clamp(textureSample(skybox_texture, skybox_sampler, uvw).rgb, vec3f(0.0), vec3f(50.0)) * 
                    cos(theta) * 
                    sin(theta);
                num_samples++;
            }
        }
        irradiance = PI * irradiance * (1.0 / f32(num_samples));
        colors[i] = vec4f(irradiance, 1.0);
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