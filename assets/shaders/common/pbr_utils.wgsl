// Distribution GGX/Throwbridge-Reitz
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

// Geometry Schlick-Beckmann (Schlick GGX)
fn g1(n: vec3f, x: vec3f, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
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

// Fresnel-Schlick
fn f(f0: vec3f, v: vec3f, h: vec3f, roughness: f32) -> vec3f {
    return f0 + (max(vec3f(1.0 - roughness), f0) - f0) * pow(1.0 - clamp(dot(v, h), 0.0, 1.0), 5.0);
}

fn pbr(
    n: vec3f,
    v: vec3f,
    l: vec3f,
    albedo: vec3f,
    radiance: vec3f,
    material: MaterialProperties
) -> vec3f {
    let h = normalize(v + l);
    let f0 = mix(vec3f(0.04), albedo, material.metallic);

    let ndf = d(n, h, material.roughness);
    let g = g(n, v, l, material.roughness);
    let f = f(f0, v, h, 0.0);

    let ks = f;
    let kd = (vec3f(1.0) - ks) * (1.0 - material.metallic);

    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_v = max(dot(n, v), 0.0);

    let numerator = ndf * g * f;
    var denominator = 4.0 * n_dot_v * n_dot_l;
    denominator = max(denominator, 0.000001);
    let specular = numerator / denominator;

    return (kd * albedo / PI + specular) * radiance * n_dot_l;
}
