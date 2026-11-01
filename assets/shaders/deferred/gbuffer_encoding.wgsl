// How the G-buffer stores a normal, shared by the geometry pass that writes it,
// `shaders/mesh/gbuffer.wgsl`, and every pass that reads it. The layout of each target is in
// `docs/design/rendering/gbuffer.md`.
//
// A unit normal is stored as its octahedral encoding: the normal projected onto the octahedron
// |x| + |y| + |z| = 1, the lower half folded over the upper, which leaves two values in
// [-1, 1]. In a 16-bit float channel the round trip stays within about 0.1 degree.

fn gbuffer_octahedral_wrap(v: vec2f) -> vec2f {
    return (1.0 - abs(v.yx)) * select(vec2f(-1.0), vec2f(1.0), v >= vec2f(0.0));
}

// The octahedral encoding of the unit vector `n`.
fn gbuffer_encode_normal(n: vec3f) -> vec2f {
    let projected = n.xy / (abs(n.x) + abs(n.y) + abs(n.z));
    if n.z < 0.0 {
        return gbuffer_octahedral_wrap(projected);
    }
    return projected;
}

// The unit vector whose octahedral encoding is `encoded`.
fn gbuffer_decode_normal(encoded: vec2f) -> vec3f {
    var n = vec3f(encoded, 1.0 - abs(encoded.x) - abs(encoded.y));
    let fold = clamp(-n.z, 0.0, 1.0);
    n.x += select(fold, -fold, n.x >= 0.0);
    n.y += select(fold, -fold, n.y >= 0.0);
    return normalize(n);
}
