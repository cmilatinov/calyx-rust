struct VertexIn {
    @location(0) position: vec3f,
    @location(2) uv0: vec2f,
};

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

const SAMPLE_COUNT = 1024u;
const PI = 3.14159265359;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = vec4f(vertex.position, 1.0);
    out.uv = vertex.uv0;
    return out;
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
	var up: vec3f;
    if abs(n.z) < 0.999 { 
        up = vec3(0.0, 0.0, 1.0); 
    } else { 
        up = vec3(1.0, 0.0, 0.0); 
    };
	let tangent = normalize(cross(up, n));
	let bitangent = cross(n, tangent);

	return normalize(tangent * h.x + bitangent * h.y + n * h.z);
}

// Geometry Schlick-Beckmann (Schlick GGX)
fn g1(n: vec3f, x: vec3f, roughness: f32) -> f32 {
    // Note that we use a different k for IBL
    let r = roughness;
    let k = (r * r) / 2.0;
    let n_dot_x = max(dot(n, x), 0.0);

    let numerator = n_dot_x;
    var denominator = n_dot_x * (1.0 - k) + k;
    denominator = max(denominator, 0.000001);

    return numerator / denominator;
}

// Geometry Smith
fn g(n: vec3f, v: vec3f, l: vec3f, roughness: f32) -> f32 {
    return g1(n, v, roughness) * g1(n, l, roughness);
}

fn integrate_brdf(n_dot_v: f32, roughness: f32) -> vec2f {
    let v = vec3f(
        sqrt(1.0 - n_dot_v * n_dot_v), 
        0.0, 
        n_dot_v
    );

    var a = 0.0;
    var b = 0.0;

    let n = vec3f(0.0, 0.0, 1.0);
    for (var i = 0u; i < SAMPLE_COUNT; i++) {
        let xi = hammersly(i, SAMPLE_COUNT);
        let h = importance_sample_ggx(xi, n, roughness);
        let l = normalize(2.0 * dot(v, h) * h - v);

        let n_dot_l = max(l.z, 0.0);
        if n_dot_l > 0.0 {
            let n_dot_h = max(h.z, 0.0);
            let v_dot_h = max(dot(v, h), 0.0);

            let g = g(n, v, l, roughness);
            let g_vis = (g * v_dot_h) / (n_dot_h * n_dot_v);
            let fc = pow(1.0 - v_dot_h, 5.0);

            a += (1.0 - fc) * g_vis;
            b += fc * g_vis;
        }
    }

    a /= f32(SAMPLE_COUNT);
    b /= f32(SAMPLE_COUNT);
    return vec2f(a, b);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    return vec4f(integrate_brdf(in.uv.x, in.uv.y), 0.0, 1.0);
}