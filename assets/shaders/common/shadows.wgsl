//#include "shaders/common/lights.wgsl"

// Shadow lookups for the lights in `light_storage`, as `ShadowRenderer` renders and uploads
// them. A lookup needs only the world position and normal of the shaded point, so the forward
// PBR pass and a deferred lighting pass include this file unchanged; neither passes the camera,
// since `ShadowData` carries the one the cascades were fitted to.
//
// - The first shadowed directional light has `shadow_index` 0 and reads `SHADOW_CASCADE_COUNT`
//   cascades, one layer each of `shadow_cascades`, chosen by view depth.
// - A shadowed spot light has `shadow_index` 0 to `SHADOW_MAX_SPOTS - 1`, its tile of
//   `shadow_atlas`.
// - Every other light has `shadow_index` -1 and is never occluded.

const SHADOW_CASCADE_COUNT = 4u;
const SHADOW_MAX_SPOTS = 8u;

// One cascade of the directional light.
struct ShadowCascade {
    // World to the cascade's clip space; depth runs 0 at the light to 1 away from it.
    view_projection: mat4x4f,
    // View depth from the camera at which the cascade ends.
    split_far: f32,
    // World size of one shadow texel, which scales the normal offset.
    texel_size: f32,
    _pad: vec2f,
};

// The shadow of one spot light, in its atlas tile.
struct SpotShadow {
    // World to the light's clip space; depth runs 0 at the near plane to 1 at its range.
    view_projection: mat4x4f,
    // Atlas UV of the tile's top left corner.
    tile_offset: vec2f,
    // World size of one shadow texel per unit of distance from the light.
    texel_scale: f32,
    // The light's normal offset, in shadow texels.
    normal_bias: f32,
};

struct ShadowData {
    cascades: array<ShadowCascade, SHADOW_CASCADE_COUNT>,
    spots: array<SpotShadow, SHADOW_MAX_SPOTS>,
    // The camera the cascades were fitted to: its position and unit forward axis.
    camera_position: vec3f,
    // The fraction of each cascade's depth range, at its far end, over which it blends into the
    // next one.
    cascade_blend: f32,
    camera_forward: vec3f,
    // The directional light's normal offset, in shadow texels.
    directional_normal_bias: f32,
    // The atlas UV size of one tile, and of one atlas texel.
    atlas_tile_size: f32,
    atlas_texel: f32,
    // The UV size of one cascade texel.
    cascade_texel: f32,
    _pad: f32,
};

@group(2) @binding(1)
var<uniform> shadow_data: ShadowData;

@group(2) @binding(2)
var shadow_cascades: texture_depth_2d_array;

@group(2) @binding(3)
var shadow_atlas: texture_depth_2d;

// A `LessEqual` comparison sampler with linear filtering, so each tap is a hardware 2x2 PCF.
@group(2) @binding(4)
var shadow_sampler: sampler_comparison;

// Where `world_position` lands in the clip space of `view_projection`: XY as texture UV, Z as
// depth.
fn shadow_coords(view_projection: mat4x4f, world_position: vec3f) -> vec3f {
    let clip = view_projection * vec4f(world_position, 1.0);
    let ndc = clip.xyz / clip.w;
    return vec3f(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}

// The fraction of the 3x3 grid of 2x2 comparison taps around `uv` that sees past `depth`, one
// texel of `texel` UV apart: a 4x4 texel tent filter.
fn shadow_pcf_cascade(layer: u32, uv: vec2f, depth: f32, texel: f32) -> f32 {
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let offset = vec2f(f32(x), f32(y)) * texel;
            lit += textureSampleCompareLevel(
                shadow_cascades, shadow_sampler, uv + offset, layer, depth
            );
        }
    }
    return lit / 9.0;
}

// As `shadow_pcf_cascade`, in the atlas, with every tap kept inside the tile whose UV bounds
// are `tile_min` to `tile_max`, so a lookup never reads a neighbouring light's tile.
fn shadow_pcf_atlas(uv: vec2f, depth: f32, tile_min: vec2f, tile_max: vec2f) -> f32 {
    let texel = shadow_data.atlas_texel;
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let tap = clamp(uv + vec2f(f32(x), f32(y)) * texel, tile_min, tile_max);
            lit += textureSampleCompareLevel(shadow_atlas, shadow_sampler, tap, depth);
        }
    }
    return lit / 9.0;
}

// How lit `world_position` is in cascade `layer`, after moving it `normal_bias` of the
// cascade's texels along `normal`. A point outside the cascade is lit.
fn cascade_lit(layer: u32, world_position: vec3f, normal: vec3f) -> f32 {
    let cascade = shadow_data.cascades[layer];
    let offset_position =
        world_position + normal * shadow_data.directional_normal_bias * cascade.texel_size;
    let coords = shadow_coords(cascade.view_projection, offset_position);
    if any(coords.xy < vec2f(0.0)) || any(coords.xy > vec2f(1.0)) || coords.z > 1.0 {
        return 1.0;
    }
    return shadow_pcf_cascade(layer, coords.xy, coords.z, shadow_data.cascade_texel);
}

// How lit `world_position` is by the shadowed directional light: 1 fully lit, 0 fully in
// shadow. The cascade is the first whose far split is past the point's view depth; over the
// last `cascade_blend` of its range the result blends into the next cascade, and the last
// cascade fades out to lit so the shadow ends without a seam.
fn directional_shadow(light: Light, world_position: vec3f, normal: vec3f) -> f32 {
    if light.shadow_index < 0 {
        return 1.0;
    }
    let depth = dot(world_position - shadow_data.camera_position, shadow_data.camera_forward);
    var near = 0.0;
    for (var i = 0u; i < SHADOW_CASCADE_COUNT; i++) {
        let far = shadow_data.cascades[i].split_far;
        if depth <= far {
            let lit = cascade_lit(i, world_position, normal);
            let blend_start = far - (far - near) * shadow_data.cascade_blend;
            if depth <= blend_start {
                return lit;
            }
            var next = 1.0;
            if i + 1u < SHADOW_CASCADE_COUNT {
                next = cascade_lit(i + 1u, world_position, normal);
            }
            return mix(lit, next, smoothstep(blend_start, far, depth));
        }
        near = far;
    }
    return 1.0;
}

// How lit `world_position` is by spot light `light`: 1 fully lit, 0 fully in shadow. The
// normal offset grows with distance from the light, as its texels do.
fn spot_shadow(light: Light, world_position: vec3f, normal: vec3f) -> f32 {
    if light.shadow_index < 0 {
        return 1.0;
    }
    let shadow = shadow_data.spots[u32(light.shadow_index)];
    let distance = max(dot(world_position - light.position, light.direction), 0.0);
    let offset_position =
        world_position + normal * shadow.normal_bias * shadow.texel_scale * distance;
    let coords = shadow_coords(shadow.view_projection, offset_position);
    if any(coords.xy < vec2f(0.0)) || any(coords.xy > vec2f(1.0)) || coords.z > 1.0 {
        return 1.0;
    }
    let tile_size = shadow_data.atlas_tile_size;
    let half_texel = vec2f(0.5 * shadow_data.atlas_texel);
    let tile_min = shadow.tile_offset + half_texel;
    let tile_max = shadow.tile_offset + vec2f(tile_size) - half_texel;
    return shadow_pcf_atlas(shadow.tile_offset + coords.xy * tile_size, coords.z, tile_min, tile_max);
}
