//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/postprocess/camera_planes.wgsl"

// Fog of war on display-linear color, after tone mapping. Each pixel is fogged by the ground
// position (x, z) of the surface it shows, rebuilt from the scene depth. A viewer sees a ground
// point when the line from the viewer to it leaves no blocker into open ground before reaching
// it, a point on a blocker's edge counting as on the blocker, so sight carries on through the run
// of touching blockers it meets first and a blocker in sight is lit, faces and all, while the
// ground behind it is not; then it sees as much as its distance fade and weight leave, and the
// side sees the point as well as its best viewer does. Lines of sight are traced per sample, so
// shadow sides are exact wherever the viewers stand, and each pixel averages a few samples across
// its own footprint on the ground to antialias them without softening them. What the side does
// not see mixes towards the fog colour in sRGB-encoded colour, as a translucent overlay drawn over
// the image would. `FogOfWarSight` in
// `crates/calyx_engine/src/component/post_process/fog_of_war.rs` holds the same rules on the CPU.
// Alpha passes through.

struct FogOfWarUniform {
    // sRGB-encoded fog colour in rgb, opacity in a.
    color: vec4f,
    // The camera, to turn depth into view-space positions.
    camera: CameraPlanes,
    // From the camera's view space to world space.
    view_to_world: mat4x4f,
    viewer_count: u32,
    blocker_count: u32,
    // How many angular sectors each viewer's blockers are sorted into, as `SIGHT_SECTORS`.
    sector_count: u32,
    _pad: u32,
};

// One viewer, as `GpuViewer` in `crates/calyx_engine/src/render/postprocess/fog_of_war.rs`.
struct Viewer {
    // Ground position (x, z).
    position: vec2f,
    fade_start: f32,
    fade_end: f32,
    fade_curve: f32,
    // How much of its sight the viewer lends, 0 to 1.
    weight: f32,
    // The viewer's first entry in `sectors`.
    first_sector: u32,
    _pad: u32,
};

// One blocker, as `GpuBlocker` in `crates/calyx_engine/src/render/postprocess/fog_of_war.rs`.
struct Blocker {
    // Ground position (x, z) of its centre.
    center: vec2f,
    // The rows of the matrix taking a ground offset from the centre to the blocker's local
    // space, where it spans -1 to 1 on both axes.
    local_x: vec2f,
    local_y: vec2f,
    // The lengths of `local_x` and `local_y`: how far the local coordinates move per world unit
    // at most.
    scale: vec2f,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var depth_texture: texture_depth_2d;

@group(0) @binding(2)
var<uniform> fog: FogOfWarUniform;

@group(0) @binding(3)
var<storage, read> viewers: array<Viewer>;

@group(0) @binding(4)
var<storage, read> blockers: array<Blocker>;

// Each viewer's `fog.sector_count` equal angular sectors around it, counted from +x towards +y, as
// the first index into `sector_blockers` and how many follow: the blockers whose angular span seen
// from the viewer overlaps the sector. A line of sight from a viewer meets only blockers of the
// sector its direction lies in.
@group(0) @binding(5)
var<storage, read> sectors: array<vec2u>;

@group(0) @binding(6)
var<storage, read> sector_blockers: array<u32>;

// How much a pixel's window of directions is widened, in radians, as `SECTOR_SLACK`.
const SECTOR_SLACK: f32 = 1e-3;

const HALF_TURN: f32 = 3.14159265;

// The sectors a viewer's lines of sight to a pixel lie in: the viewer's first entry in `sectors`,
// the first of the pixel's sectors around the viewer, and how many follow it, wrapping around.
struct SectorWindow {
    base: u32,
    first: u32,
    count: u32,
};

// The sectors of `viewer` holding every direction within `half_width` radians of `axis`, a unit
// direction; half a turn or more holds them all.
fn sector_window(viewer: Viewer, axis: vec2f, half_width: f32) -> SectorWindow {
    var window: SectorWindow;
    window.base = viewer.first_sector;
    window.first = 0u;
    window.count = fog.sector_count;
    if half_width >= HALF_TURN {
        return window;
    }
    let sector_angle = 2.0 * HALF_TURN / f32(fog.sector_count);
    let angle = atan2(axis.y, axis.x);
    let first = i32(floor((angle - half_width) / sector_angle));
    let last = i32(floor((angle + half_width) / sector_angle));
    let count = i32(fog.sector_count);
    window.first = u32(((first % count) + count) % count);
    window.count = u32(min(last - first + 1, count));
    return window;
}

// The entry in `sectors` of the `index`th sector of `window`.
fn window_sector(window: SectorWindow, index: u32) -> vec2u {
    return sectors[window.base + (window.first + index) % fog.sector_count];
}

// How far apart two blockers along a line of sight may be and still count as touching, as
// `FogOfWarSight::TOUCHING`.
const TOUCHING: f32 = 0.05;

// How far past the side of a blocker a line of sight leaves by a point may lie and still count as
// on the blocker, as `FogOfWarSight::EDGE`. Every pixel of a wall's face rebuilds to a point on the
// edge of the wall's footprint, give or take the depth's rounding, so without it the face of a
// wall in sight turned from the viewer would be lit or fogged row by row, and flicker as the
// camera or the viewer moves.
const EDGE: f32 = 0.005;

// Where each pixel samples sight, in pixels from its centre, as `FogOfWarSight::PIXEL_SAMPLES`:
// across in `SAMPLES_X` and down in `SAMPLES_Y`.
const SAMPLES_X = vec4f(0.125, 0.375, -0.125, -0.375);
const SAMPLES_Y = vec4f(0.375, -0.125, -0.375, 0.125);

// How much `viewer` sees at each of four `distance`s from it, cover aside.
fn range_fade(viewer: Viewer, distance: vec4f) -> vec4f {
    var t: vec4f;
    if viewer.fade_end <= viewer.fade_start {
        t = select(vec4f(0.0), vec4f(1.0), distance >= vec4f(viewer.fade_start));
    } else {
        t = saturate((distance - viewer.fade_start) / (viewer.fade_end - viewer.fade_start));
    }
    let k = viewer.fade_curve;
    if abs(k) < 1e-4 {
        return 1.0 - t;
    }
    return 1.0 - (exp(k * t) - 1.0) / (exp(k) - 1.0);
}

// Where the line from `start` along the unit `direction` is inside `blocker` within `limit` of
// `start`, as `FogOfWarBlocker::span`: (entry, exit) distances along it in xy, and in z whether
// it meets the blocker at all.
fn span(blocker: Blocker, start: vec2f, direction: vec2f, limit: f32) -> vec3f {
    let relative = start - blocker.center;
    let origin = vec2f(dot(blocker.local_x, relative), dot(blocker.local_y, relative));
    let heading = vec2f(dot(blocker.local_x, direction), dot(blocker.local_y, direction));
    var near = -3.0e38;
    var far = 3.0e38;
    for (var axis = 0; axis < 2; axis++) {
        if abs(heading[axis]) < 1e-6 {
            if abs(origin[axis]) > 1.0 {
                return vec3f(0.0);
            }
        } else {
            let a = (-1.0 - origin[axis]) / heading[axis];
            let b = (1.0 - origin[axis]) / heading[axis];
            near = max(near, min(a, b));
            far = min(far, max(a, b));
        }
    }
    if near <= far && far >= 0.0 && near <= limit {
        return vec3f(max(near, 0.0), min(far, limit), 1.0);
    }
    return vec3f(0.0);
}

// Whether the line from `start` along the unit `direction`, `reach` long, passes within `margin`
// of `blocker`: whether it crosses the blocker's footprint widened by `margin` in its local space.
// A cheap test that spares most blockers the full one.
fn near_line(blocker: Blocker, start: vec2f, direction: vec2f, reach: f32, margin: f32) -> bool {
    let relative = start - blocker.center;
    let origin = vec2f(dot(blocker.local_x, relative), dot(blocker.local_y, relative));
    let heading = vec2f(dot(blocker.local_x, direction), dot(blocker.local_y, direction));
    let extent = 1.0 + margin * blocker.scale;
    var near = 0.0;
    var far = reach;
    if abs(heading.x) < 1e-6 {
        if abs(origin.x) > extent.x {
            return false;
        }
    } else {
        let a = (-extent.x - origin.x) / heading.x;
        let b = (extent.x - origin.x) / heading.x;
        near = max(near, min(a, b));
        far = min(far, max(a, b));
    }
    if abs(heading.y) < 1e-6 {
        if abs(origin.y) > extent.y {
            return false;
        }
    } else {
        let a = (-extent.y - origin.y) / heading.y;
        let b = (extent.y - origin.y) / heading.y;
        near = max(near, min(a, b));
        far = min(far, max(a, b));
    }
    return near <= far;
}

// Whether the line from `start` along the unit `direction`, which leaves a blocker at `leaves_at`,
// carries on into another blocker that overlaps or touches it there, within `limit` of `start`.
// `window` holds the sectors the line lies in.
fn carried_on(
    window: SectorWindow,
    start: vec2f,
    direction: vec2f,
    limit: f32,
    leaves_at: f32,
) -> bool {
    for (var k = 0u; k < window.count; k++) {
        let sector = window_sector(window, k);
        for (var index = sector.x; index < sector.x + sector.y; index++) {
            let into = span(blockers[sector_blockers[index]], start, direction, limit);
            if into.z != 0.0 && into.x <= leaves_at + TOUCHING && into.y > leaves_at {
                return true;
            }
        }
    }
    return false;
}

// Where the lines from `start` along the four unit directions `(dir_x, dir_y)` are inside
// `blocker`, each within its `limit` of `start`, as `span` does for one line: entry distances in
// `near`, exit distances in `far`, and whether each meets the blocker at all in `hit`.
struct Spans {
    near: vec4f,
    far: vec4f,
    hit: vec4<bool>,
};

fn spans(blocker: Blocker, start: vec2f, dir_x: vec4f, dir_y: vec4f, limit: vec4f) -> Spans {
    let relative = start - blocker.center;
    let origin = vec2f(dot(blocker.local_x, relative), dot(blocker.local_y, relative));
    let step_x = blocker.local_x.x * dir_x + blocker.local_x.y * dir_y;
    let step_y = blocker.local_y.x * dir_x + blocker.local_y.y * dir_y;
    let flat_x = abs(step_x) < vec4f(1e-6);
    let flat_y = abs(step_y) < vec4f(1e-6);
    let ax = (-1.0 - origin.x) / step_x;
    let bx = (1.0 - origin.x) / step_x;
    let ay = (-1.0 - origin.y) / step_y;
    let by = (1.0 - origin.y) / step_y;
    let near = max(
        select(min(ax, bx), vec4f(-3.0e38), flat_x),
        select(min(ay, by), vec4f(-3.0e38), flat_y),
    );
    let far = min(
        select(max(ax, bx), vec4f(3.0e38), flat_x),
        select(max(ay, by), vec4f(3.0e38), flat_y),
    );
    let outside = (flat_x & vec4<bool>(abs(origin.x) > 1.0))
        | (flat_y & vec4<bool>(abs(origin.y) > 1.0));
    let hit = !outside & near <= far & far >= vec4f(0.0) & near <= limit;
    return Spans(max(near, vec4f(0.0)), min(far, limit), hit);
}

// Which of the four sample points `(points_x, points_y)` sight reaches from `viewer`, 1 or 0 each:
// those whose line from `start` leaves no blocker into open ground more than `EDGE` before
// reaching them, as `FogOfWarSight::clear_line`. The four lines are traced together, so each
// blocker is read once for all of them, and one that comes nowhere near the line to the pixel's
// centre `center`, widened by how far the samples lie from it, `spread`, is skipped for all of
// them. Only the blockers of the viewer's sectors the four lines lie in are read.
fn clear_lines(
    viewer: Viewer,
    points_x: vec4f,
    points_y: vec4f,
    center: vec2f,
    spread: f32,
) -> vec4f {
    let start = viewer.position;
    let offset_x = points_x - start.x;
    let offset_y = points_y - start.y;
    let distance = sqrt(offset_x * offset_x + offset_y * offset_y);
    let here = distance < vec4f(1e-6);
    let dir_x = select(offset_x / distance, vec4f(1.0), here);
    let dir_y = select(offset_y / distance, vec4f(0.0), here);
    let limit = distance + TOUCHING;
    let towards = center - start;
    let reach = length(towards);
    let axis = select(towards / reach, vec2f(1.0, 0.0), reach < 1e-6);
    // Every sample's line lies within `margin` of the centre's, out to `along`.
    let along = reach + spread + TOUCHING;
    let margin = spread * along / max(reach - spread, 1e-4) + 1e-3;
    // Every sample lies within `spread` of the centre, so its line turns from the centre's by
    // less than asin(spread / reach); close to the viewer the lines can point anywhere.
    var half_width = HALF_TURN;
    if spread * 2.0 < reach {
        half_width = asin(spread / reach) + SECTOR_SLACK;
    }
    let window = sector_window(viewer, axis, half_width);
    var open = !here;
    var blocked = vec4<bool>(false);
    for (var k = 0u; k < window.count && any(open); k++) {
        let sector = window_sector(window, k);
        for (var entry = sector.x; entry < sector.x + sector.y; entry++) {
            let blocker = blockers[sector_blockers[entry]];
            if !near_line(blocker, start, axis, along, margin) {
                continue;
            }
            let inside = spans(blocker, start, dir_x, dir_y, limit);
            // Lines that leave this blocker before their sample, by more than `EDGE`: into open
            // ground, unless another blocker carries them on.
            let leaving = open & inside.hit & inside.far < distance - EDGE;
            if !any(leaving) {
                continue;
            }
            let carried = vec4<bool>(
                leaving.x
                    && carried_on(window, start, vec2f(dir_x.x, dir_y.x), limit.x, inside.far.x),
                leaving.y
                    && carried_on(window, start, vec2f(dir_x.y, dir_y.y), limit.y, inside.far.y),
                leaving.z
                    && carried_on(window, start, vec2f(dir_x.z, dir_y.z), limit.z, inside.far.z),
                leaving.w
                    && carried_on(window, start, vec2f(dir_x.w, dir_y.w), limit.w, inside.far.w),
            );
            blocked = blocked | (leaving & !carried);
            open = open & !blocked;
            if !any(open) {
                break;
            }
        }
    }
    return select(vec4f(1.0), vec4f(0.0), blocked);
}

// How much of the pixel showing the ground point `center` the side sees, a pixel to the right on
// screen lying `dx` further on the ground and a pixel down `dy`: the mean over its samples of the
// most any viewer sees of each, as `FogOfWarSight::pixel_visibility`.
fn pixel_visibility(center: vec2f, dx: vec2f, dy: vec2f) -> f32 {
    let points_x = center.x + dx.x * SAMPLES_X + dy.x * SAMPLES_Y;
    let points_y = center.y + dx.y * SAMPLES_X + dy.y * SAMPLES_Y;
    let spread = 0.375 * (length(dx) + length(dy)) + 1e-4;
    var seen = vec4f(0.0);
    for (var index = 0u; index < fog.viewer_count; index++) {
        let viewer = viewers[index];
        let offset_x = points_x - viewer.position.x;
        let offset_y = points_y - viewer.position.y;
        let sight = range_fade(viewer, sqrt(offset_x * offset_x + offset_y * offset_y))
            * saturate(viewer.weight);
        if all(sight <= seen) {
            continue;
        }
        seen = max(seen, sight * clear_lines(viewer, points_x, points_y, center, spread));
    }
    return (seen.x + seen.y + seen.z + seen.w) * 0.25;
}

// The world position the pixel `pixel` shows, rebuilt from the scene depth.
fn world_at(pixel: vec2i, size: vec2f) -> vec3f {
    let depth = textureLoad(depth_texture, pixel, 0);
    let ndc = camera_ndc((vec2f(pixel) + 0.5) / size);
    let view = camera_view_position(fog.camera, ndc, camera_linear_depth(fog.camera, depth));
    return (fog.view_to_world * vec4f(view, 1.0)).xyz;
}

// The ground step to a neighbouring pixel: whichever of the steps `ahead` and `behind` is
// shorter, so a pixel on a silhouette steps across its own surface rather than onto whatever lies
// behind it. A step to a neighbour off the image is zero and is not taken.
fn ground_step(ahead: vec2f, behind: vec2f) -> vec2f {
    let a = dot(ahead, ahead);
    let b = dot(behind, behind);
    if a == 0.0 {
        return behind;
    }
    if b == 0.0 || a <= b {
        return ahead;
    }
    return behind;
}

fn linear_to_srgb(value: vec3f) -> vec3f {
    let low = value * 12.92;
    let high = 1.055 * pow(value, vec3f(1.0 / 2.4)) - 0.055;
    return select(high, low, value <= vec3f(0.0031308));
}

fn srgb_to_linear(value: vec3f) -> vec3f {
    let low = value / 12.92;
    let high = pow((value + 0.055) / 1.055, vec3f(2.4));
    return select(high, low, value <= vec3f(0.04045));
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let texel = textureLoad(input_texture, pixel, 0);
    let dimensions = vec2i(textureDimensions(depth_texture));
    let size = vec2f(dimensions);
    let last = dimensions - 1;
    let world = world_at(pixel, size).xz;
    let right = world_at(min(pixel + vec2i(1, 0), last), size).xz - world;
    let left = world - world_at(max(pixel - vec2i(1, 0), vec2i(0)), size).xz;
    let down = world_at(min(pixel + vec2i(0, 1), last), size).xz - world;
    let up = world - world_at(max(pixel - vec2i(0, 1), vec2i(0)), size).xz;
    let dx = ground_step(right, left);
    let dy = ground_step(down, up);
    let amount = saturate(fog.color.a) * (1.0 - pixel_visibility(world, dx, dy));
    if amount <= 0.0 {
        return texel;
    }
    let encoded = linear_to_srgb(saturate(texel.rgb));
    let fogged = srgb_to_linear(mix(encoded, fog.color.rgb, amount));
    return vec4f(fogged, texel.a);
}
