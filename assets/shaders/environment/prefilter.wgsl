//#include "shaders/constants.wgsl"
//#include "shaders/cubemap_face.wgsl"

fn d(n: vec3f, h: vec3f, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let n_dot_h2 = n_dot_h * n_dot_h;

    let numerator = a2;
    var denominator = n_dot_h2 * (a2 - 1.0) + 1.0;
    denominator = PI * denominator * denominator;
    denominator = max(denominator, 0.000001);

    return numerator / denominator;
}

fn radical_inverse_vdc(input: u32) -> f32 {
    var bits = input;
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}

fn hammersly(i: u32, n: u32) -> vec2f {
    return vec2f(f32(i) / f32(n), radical_inverse_vdc(i));
}

fn importance_sample_ggx(xi: vec2f, n: vec3f, roughness: f32) -> vec3f {
    let a = roughness * roughness;
	
	let phi = 2.0 * PI * xi.x;
	let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
	let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
	
	// from spherical coordinates to cartesian coordinates - halfway vector
	let h = vec3f(
        cos(phi) * sin_theta,
	    sin(phi) * sin_theta,
	    cos_theta
    );
	
	// from tangent-space H vector to world-space sample vector
    let up = select(
        vec3(1.0, 0.0, 0.0), 
        vec3(0.0, 0.0, 1.0), 
        abs(n.z) < 0.999
    );
	let tangent = normalize(cross(up, n));
	let bitangent = cross(n, tangent);

	return normalize(tangent * h.x + bitangent * h.y + n * h.z);
}

const ROUGHNESS_NUM_VALUES = 5u;
const ROUGHNESS_VALUES = array<f32, ROUGHNESS_NUM_VALUES>(0.0, 0.25, 0.5, 0.75, 1.0);
const SAMPLE_COUNT = 1024u;

@group(0) @binding(0)
var src: texture_cube<f32>;

@group(0) @binding(1)
var src_sampler: sampler;

@group(0) @binding(2)
var dst: binding_array<texture_storage_2d_array<rgba16float, write>, ROUGHNESS_NUM_VALUES>;

@compute
@workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) gid: vec3u) {
    let roughness_index = gid.z / 6u;
    let dst_dimensions = textureDimensions(dst[roughness_index]);
    if (gid.x >= dst_dimensions.x || gid.y >= dst_dimensions.y) {
        return;
    }
    
    let face_index = gid.z % 6u;
    let face = CUBEMAP_FACES[face_index];
    let roughness = ROUGHNESS_VALUES[roughness_index];
    let dst_dimensions_f = vec2f(dst_dimensions);
    let sa_texel = 4.0 * PI / (6.0 * dst_dimensions_f.x * dst_dimensions_f.y);
    let cube_uv = vec2f(gid.xy) / dst_dimensions_f * 2.0 - 1.0;
    let n = normalize(face.forward + face.right * cube_uv.x + face.up * cube_uv.y);
    let v = n;
    
    var color = vec3f(0.0);
    var total_weight = 0.0;

    for (var s = 0u; s < SAMPLE_COUNT; s++) {
        // Generates a sample vector that's biased towards 
        // the preferred alignment direction (importance sampling).
        let xi = hammersly(s, SAMPLE_COUNT);
        let h = importance_sample_ggx(xi, n, roughness);
        let l = normalize(2.0 * dot(v, h) * h - v);

        let n_dot_l = max(dot(n, l), 0.0);
        if n_dot_l > 0.0 {
            // Sample from the environment's mip level based on roughness/pdf
            let d = d(n, h, roughness);
            let n_dot_h = max(dot(n, h), 0.0);
            let n_dot_v = max(dot(n, v), 0.0);
            let pdf = d * n_dot_h / (4.0 * n_dot_v) + 0.0001;

            let sa_sample = 1.0 / (f32(SAMPLE_COUNT) * pdf + 0.0001);
            let mip_level = select(0.5 * log2(sa_sample / sa_texel), 0.0, roughness == 0.0);

            color += 
                clamp(
                    textureSampleLevel(src, src_sampler, l, mip_level).rgb * n_dot_l,
                    vec3f(0.0),
                    vec3f(10.0)
                );
            total_weight += n_dot_l;
        }
    }
    
    color /= total_weight;
    textureStore(dst[roughness_index], gid.xy, face_index, vec4f(color, 1.0));
}