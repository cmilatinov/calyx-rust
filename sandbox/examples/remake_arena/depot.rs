//! The supply depot, the arena's southeast corner past the south branch: a
//! tractor yard behind the depot's wall. A timber shed backs onto the east
//! border, the fuel tank stands raised on its stand in the corner past it,
//! the tractor is parked along the depot wall, and stacks of crates line the
//! yard by the creek and the south inlet, leaving lanes about a tank wide
//! between them. The middle, [`YARD`], stays open gravel. Beyond the border
//! a second shed, a parked tractor, stacked crates and fences carry the depot
//! on outside the walls.
//!
//! Every piece is turned 35 to 45 degrees to the arena's axes, as the other
//! cover is. Positions are in the Arena's frame.

use nalgebra_glm::{vec2, vec3, Vec2, Vec3};

use super::{blocking_shells, border, farmland::Creek, Author};

/// The open yard, `(centre, radius)`: ground no depot piece stands on,
/// left readable for objectives.
pub const YARD: (Vec2, f32) = (Vec2::new(26.0, -27.6), 2.0);

/// The depot's pieces inside the border, `(model, name, x, z, yaw)`, each
/// `Arena Cover Depot {name}`: the shed, the fuel tank and the tractor,
/// each with its solid in [`solids`].
const PIECES: [(&str, &str, f32, f32, f32); 3] = [
    // Its back to the east border, its door facing the yard.
    ("depot_shed", "Supply Shed", 28.9, -24.6, 38.0),
    // In the corner past the shed, its ladder towards the shed and its
    // hose towards the inlet.
    ("fuel_tank", "Fuel Tank", 29.4, -31.8, -40.0),
    // Parked along the depot wall, nose to the shed.
    ("tractor", "Tractor", 22.6, -18.3, 38.0),
];

/// The crates stacked by the south inlet, `(model, name, x, z, yaw)`, as
/// cover.
const INLET_CRATES: [(&str, &str, f32, f32, f32); 2] = [
    ("crate_large", "Depot Inlet Crate 0", 22.3, -28.4, 42.0),
    ("crate_framed", "Depot Inlet Crate 1", 23.6, -29.5, 42.0),
];

/// Crates stacked on top of the yard's crates, `(model, x, y, z, yaw)`:
/// above the shell-blocking top, so decoration only.
const STACKED: [(&str, f32, f32, f32, f32); 3] = [
    ("crate_framed", 23.0, 1.65, -23.4, 49.0),
    ("crate_framed", 22.3, 1.65, -28.4, 33.0),
    ("crate_framed", 31.6, 1.1, -28.4, 36.0),
];

/// Beyond the border, `(model, name, x, z, yaw)`: no collision, as the rest
/// of the outside.
const OUTSIDE: [(&str, &str, f32, f32, f32); 7] = [
    ("depot_shed", "Shed", 40.0, -25.5, 40.0),
    ("tractor", "Tractor", 43.5, -14.0, 80.0),
    ("crate_large", "Crate 0", 38.6, -30.4, 20.0),
    ("crate_framed", "Crate 1", 38.9, -32.0, 35.0),
    ("crate_large", "Crate 2", 44.0, -27.5, 10.0),
    ("fence", "Fence 0", 31.5, -40.0, -40.0),
    ("fence", "Fence 1", 34.6, -37.4, -40.0),
];

/// A piece's solids in its own frame, `(centre, size)`, risen to the
/// shell-blocking top: the shed's body, the fuel tank's stand and the
/// tractor from its rear wheels to its front weights.
fn solids(model: &str) -> (Vec3, Vec3) {
    match model {
        "depot_shed" => (vec3(0.0, 1.3, 0.0), vec3(5.6, 2.6, 3.8)),
        "fuel_tank" => (vec3(0.0, 1.0, 0.0), vec3(3.9, 2.0, 2.1)),
        "tractor" => (vec3(0.25, 0.85, 0.0), vec3(3.9, 1.7, 1.95)),
        _ => panic!("no depot solid for {model}"),
    }
}

impl Author {
    /// The supply depot: its shed, fuel tank, tractor and crates, and the
    /// depot carried on beyond the border.
    pub fn depot(&mut self, creek: &Creek) {
        // Every corner of a piece's footprint, `size` across its local X
        // and Z at `p` turned `yaw` degrees, stays on dry ground inside the
        // border's solids and out of the yard.
        let clear = |p: Vec2, size: Vec2, yaw: f32, what: &str| {
            let (sin, cos) = yaw.to_radians().sin_cos();
            let (along, across) = (vec2(cos, -sin), vec2(sin, cos));
            for (sx, sz) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                let corner = p + along * size.x * sx + across * size.y * sz;
                assert!(
                    creek.beyond(corner) > 1.0,
                    "the depot's {what} stands in the creek at {corner:?}"
                );
                assert!(
                    border::beyond(corner) < -0.6,
                    "the depot's {what} stands in the border at {corner:?}"
                );
                assert!(
                    (corner - YARD.0).norm() > YARD.1,
                    "the depot's {what} stands in the yard at {corner:?}"
                );
            }
        };
        for &(model, name, x, z, yaw) in &PIECES {
            let (center, size) = solids(model);
            clear(vec2(x, z), size.xz(), yaw, name);
            let name = format!("Arena Cover Depot {name}");
            let root = self.kit(model, &name, vec3(x, 0.0, z), yaw);
            let (center, size) = blocking_shells(center, size);
            self.collider(root, &format!("{name} collision 0"), center, size);
        }
        for &(model, name, x, z, yaw) in &INLET_CRATES {
            clear(vec2(x, z), vec2(1.65, 1.65), yaw, name);
            self.prop(model, &format!("Arena Cover {name}"), x, z, yaw);
        }
        for (i, &(model, x, y, z, yaw)) in STACKED.iter().enumerate() {
            self.kit(
                model,
                &format!("Arena Prop Depot Stacked Crate {i}"),
                vec3(x, y, z),
                yaw,
            );
        }
        for &(model, name, x, z, yaw) in &OUTSIDE {
            assert!(
                border::beyond(vec2(x, z)) > 2.5 && creek.beyond(vec2(x, z)) > 2.0,
                "the depot's outside {name} stands in the pen or the creek"
            );
            self.kit(
                model,
                &format!("Arena Prop Depot Outside {name}"),
                vec3(x, 0.0, z),
                yaw,
            );
        }
    }
}
