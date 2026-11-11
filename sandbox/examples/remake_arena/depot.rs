//! The supply depot, the arena's southeast corner past the south branch: a
//! tractor yard in the border's south-east lobe, behind the depot's wall.
//! A timber shed stands off the east border with a lane behind it, the fuel
//! tank stands raised on its stand in the lobe's south-east corner, square
//! to the shed, and the tractor is parked along the depot wall. The middle,
//! [`YARD`], stays open gravel, room for two tanks to fight, with ways in
//! from the north by the creek, from the cove by the wall's end, round
//! behind the shed and between the shed and the fuel tank. Beyond the
//! border a second shed, square to the first, a parked tractor, stacked
//! crates and fences carry the depot on outside the walls.
//!
//! The crates are expected to become loose physics objects that tanks shove
//! and rounds knock over, so none is load-bearing: the shed, the fuel tank,
//! the tractor and the walls shape the yard and its lanes, and each crate
//! or stack stands in the open with room round it to tumble without
//! jamming a lane or the south bridge's exit. Every crate is its own object
//! with its own solid.
//!
//! The buildings stand square to each other, each at 40 degrees to the
//! arena's axes, as the other cover is turned 35 to 45 degrees. Positions
//! are in the Arena's frame.

use nalgebra_glm::{vec2, vec3, Vec2, Vec3};

use super::{blocking_shells, border, farmland::Creek, Author};

/// The open yard, `(centre, radius)`: ground no depot piece stands on,
/// left readable for objectives.
pub const YARD: (Vec2, f32) = (Vec2::new(31.0, -30.0), 4.5);

/// The depot's pieces inside the border, `(model, name, x, z, yaw)`, each
/// `Arena Cover Depot {name}`: the shed, the fuel tank and the tractor,
/// each with its solid in [`solids`].
const PIECES: [(&str, &str, f32, f32, f32); 3] = [
    // Its door facing the yard, a lane behind it to the east border.
    ("depot_shed", "Supply Shed", 33.6, -21.8, 40.0),
    // Square to the shed, along the lobe's south-east stretch, its ladder
    // towards the shed and its hose towards the yard.
    ("fuel_tank", "Fuel Tank", 38.0, -33.0, -50.0),
    // Parked along the depot wall, nose to the shed.
    ("tractor", "Tractor", 22.6, -18.3, 38.0),
];

/// The crates by the south inlet, `(model, name, x, z, yaw)`, as cover.
const INLET_CRATES: [(&str, &str, f32, f32, f32); 2] = [
    ("crate_large", "Depot Inlet Crate 0", 23.5, -33.0, 42.0),
    ("crate_framed", "Depot Inlet Crate 1", 25.0, -34.0, 42.0),
];

/// Crates stacked on top of the yard's crates, `(model, x, y, z, yaw)`:
/// above the shell-blocking top, so decoration only, until crates topple.
const STACKED: [(&str, f32, f32, f32, f32); 3] = [
    ("crate_framed", 24.8, 1.65, -24.3, 49.0),
    ("crate_framed", 23.5, 1.65, -33.0, 33.0),
    ("crate_framed", 29.5, 1.1, -24.5, 36.0),
];

/// Beyond the border, `(model, name, x, z, yaw)`: no collision, as the rest
/// of the outside. The shed stands square to the one inside.
const OUTSIDE: [(&str, &str, f32, f32, f32); 7] = [
    ("depot_shed", "Shed", 47.5, -27.0, -50.0),
    ("tractor", "Tractor", 49.0, -12.0, 80.0),
    ("crate_large", "Crate 0", 45.5, -32.5, 20.0),
    ("crate_framed", "Crate 1", 46.0, -34.2, 35.0),
    ("crate_large", "Crate 2", 51.0, -30.5, 10.0),
    ("fence", "Fence 0", 31.0, -41.5, -8.0),
    ("fence", "Fence 1", 35.0, -42.0, -8.0),
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
