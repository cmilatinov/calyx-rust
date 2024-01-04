struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
//    @location(3) uv1: vec2<f32>,
//    @location(4) uv2: vec2<f32>,
//    @location(5) uv3: vec2<f32>,
    @location(6) model_0: vec4<f32>,
    @location(7) model_1: vec4<f32>,
    @location(8) model_2: vec4<f32>,
    @location(9) model_3: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>
};

struct CameraUniforms {
    projection: mat4x4<f32>,
    view: mat4x4<f32>,
    inverse_projection: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
    near_plane: f32,
    far_plane: f32
};

struct PointLight {
    position: vec3<f32>,
    radius: f32,
    color: vec3<f32>,
};

struct LightStorage {
    point_lights_size: u32,
    point_lights: array<PointLight>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

@group(1) @binding(0)
var<storage, read> lights: LightStorage;

@group(2) @binding(0)
var texture_diffuse: texture_2d<f32>;

@group(2) @binding(1)
var sampler_diffuse: sampler;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(
        vertex.model_0,
        vertex.model_1,
        vertex.model_2,
        vertex.model_3
    );
    var out: VertexOut;
    out.world_position = (model * vec4<f32>(vertex.position, 1.0)).xyz;
    out.position =
        camera.projection *
        camera.view *
        model *
        vec4<f32>(vertex.position, 1.0);
    out.normal = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
    out.uv = vertex.uv0;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let diffuse_color = textureSample(texture_diffuse, sampler_diffuse, in.uv);
    let normal = normalize(in.normal);

    var color = vec3f(0.0);
    for (var i = 0u; i < lights.point_lights_size; i++) {
        let worldToLight = lights.point_lights[i].position - in.world_position;
        let dist = length(worldToLight);
        let dir = normalize(worldToLight);

        // Determine the contribution of this light to the surface color.
        let radiance = lights.point_lights[i].color * (1.0 / (dist * dist));
        let nDotL = max(dot(normal, dir), 0.0);

        // Accumulate light contribution to the surface color.
        color += vec3f(diffuse_color.rgb * radiance * nDotL);
    }

    // Gamma correction
    let gamma = 2.2;
    color = pow(color, vec3f(1.0 / gamma));

    return vec4f(color.xyz, diffuse_color.a);
}