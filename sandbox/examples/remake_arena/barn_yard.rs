//! The northeast corner, the barn yard (#579): a red barn against the
//! platform's south ramp, its door gable facing the feed yard at the ramp's
//! foot; a smaller stable square to it in the lobe north of the platform;
//! the tall silo in the cove by the east border; and a fenced paddock under
//! the lobe's north wall, with more paddocks and bales framing the lobe
//! outside its wall. The feed yard in front of the barn's door is left
//! open, worn dirt (`layout::FOOTPRINTS`' `Barn`), a clearing objectives
//! can use.
//!
//! The buildings stand square to each other: the barn's ridge runs north to
//! south, the stable's east to west. The barn, the stable and the silo stop
//! tanks, rounds and sight like any cover. The paddock's rail fence stops
//! tanks only, like the bridges' rails: rounds and sight cross it, so the
//! lobe keeps its long sightlines; it leaves a gate wider than a tank.
//!
//! The troughs, bales and crate are loose furniture that may become
//! physical objects: each stands alone in open ground, at least a tank's
//! width clear of everything, so pushed about it jams no lane, and no lane
//! or cover depends on it. Positions are in the Arena's frame.

use calyx_engine::component::ComponentCollider;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use sandbox::collision::TANK_BLOCKER;

use super::{blocking_shells, border, farmland::Creek, Author};

/// The barn: its centre and yaw. Its ridge runs north to south along the
/// south ramp's west kerb, which its east wall meets, its door gable (the
/// model's +X) facing south over the feed yard. Tanks pass it on the
/// creek's side.
pub const BARN: (f32, f32, f32) = (14.9, 13.8, 90.0);
/// The barn's walls, the solid: length (its X), height and depth (its Z).
const BARN_WALLS: Vec3 = Vec3::new(6.6, 3.2, 4.6);

/// The stable: the barn's model at this scale, its centre and yaw, square
/// to the barn, its door gable facing east along the lobe.
const STABLE_SCALE: f32 = 0.62;
const STABLE: (f32, f32, f32) = (31.0, 34.0, 0.0);

/// The silo's centre, in the cove by the east border, and its solid: an
/// octagon this far from the centre to each side, round its tower. Its
/// back meets the cove's wall; it leaves a lane more than 6 m wide to the
/// south ramp's kerb, the way round the platform's east side to the lobe.
pub const SILO: Vec2 = Vec2::new(31.0, 15.0);
const SILO_APOTHEM: f32 = 1.8;

/// The loose furniture, `(model, name, x, z, yaw)`: a trough in the
/// paddock and a bale in the open lobe east of the stable, each alone in
/// open ground.
const PROPS: [(&str, &str, f32, f32, f32); 2] = [
    ("feed_trough", "Paddock Trough", 21.5, 38.4, 0.0),
    ("hay_bale", "Lobe Hay Bale", 36.0, 27.5, 40.0),
];

/// The paddock's rail fence inside the pen, runs of straight stretches
/// through their points: a V at 43 and 38 degrees under the lobe's north
/// wall, the wall closing the paddock behind it, open at the V's foot by a
/// gate wider than a tank. Each stretch is kit modules, stretched.
const PADDOCK_FENCES: [&[(f32, f32)]; 2] =
    [&[(18.8, 33.8), (12.9, 38.6)], &[(24.2, 33.6), (28.0, 36.5)]];

/// Outside the lobe's wall, decoration only: paddock fences, runs of
/// straight stretches through their points, and a stack of bales and loose
/// bales, `(x, z, yaw)`.
const OUTSIDE_FENCES: [&[(f32, f32)]; 2] = [
    &[(13.0, 46.5), (21.5, 47.5), (28.5, 44.5), (35.0, 44.0)],
    &[(38.0, 41.5), (42.0, 36.5), (46.0, 33.0), (45.5, 28.5)],
];
const OUTSIDE_BALES: [(f32, f32, f32); 5] = [
    (23.5, 50.0, 20.0),
    (24.9, 49.6, 20.0),
    (24.2, 51.0, 20.0),
    (36.5, 47.0, 75.0),
    (44.0, 31.0, 30.0),
];

impl Author {
    /// The barn yard: the barn, the stable, the silo, their furniture, the
    /// paddock and the framing outside the lobe.
    pub fn barn_yard(&mut self, creek: &Creek) {
        let walls = [(vec3(0.0, BARN_WALLS.y * 0.5, 0.0), BARN_WALLS, 0.0)];
        let (x, z, yaw) = BARN;
        self.landmark("barn", "Arena Prop Barn", vec2(x, z), yaw, &walls);
        // The stable is scaled; its solid, the scaled walls, stands in the
        // Arena's frame, unscaled.
        let (x, z, yaw) = STABLE;
        let stable = self.landmark("barn", "Arena Prop Stable", vec2(x, z), yaw, &[]);
        let mut pose = self.scene.transform(stable);
        pose.scale = Vec3::repeat(STABLE_SCALE);
        self.scene.set_transform(stable, &pose.matrix());
        let size = BARN_WALLS * STABLE_SCALE;
        let (center, size) = blocking_shells(vec3(x, size.y * 0.5, z), size);
        self.tilted_collider(
            self.arena,
            "Arena Prop Stable collision 0",
            center,
            size,
            UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
        );
        // An octagon of four crossed bars, each as long as the octagon is
        // across and as wide as one of its sides.
        let across = SILO_APOTHEM * 2.0;
        let side = across * (std::f32::consts::PI / 8.0).tan();
        let silo: Vec<_> = [0.0, 45.0, 90.0, 135.0]
            .into_iter()
            .map(|turn| (vec3(0.0, 1.5, 0.0), vec3(across, 3.0, side), turn))
            .collect();
        self.landmark("silo", "Arena Prop Silo", SILO, 0.0, &silo);
        for &(model, name, x, z, yaw) in &PROPS {
            let name = format!("Arena Cover {name}");
            match model {
                "feed_trough" => {
                    self.landmark(
                        model,
                        &name,
                        vec2(x, z),
                        yaw,
                        &[(vec3(0.0, 0.375, 0.0), vec3(2.4, 0.75, 1.1), 0.0)],
                    );
                }
                _ => self.prop(model, &name, x, z, yaw),
            }
        }
        let mut stretch = 0;
        for (i, points) in PADDOCK_FENCES.iter().enumerate() {
            for pair in points.windows(2) {
                let (a, b) = (vec2(pair[0].0, pair[0].1), vec2(pair[1].0, pair[1].1));
                assert!(
                    creek.beyond(a).min(creek.beyond(b)) > 3.0,
                    "paddock fence {i} stands by the creek"
                );
                self.fence_run(&format!("Paddock Fence {stretch}"), a, b, true);
                stretch += 1;
            }
        }
        let mut stretch = 0;
        for (i, points) in OUTSIDE_FENCES.iter().enumerate() {
            for pair in points.windows(2) {
                let (a, b) = (vec2(pair[0].0, pair[0].1), vec2(pair[1].0, pair[1].1));
                assert!(
                    border::beyond(a).min(border::beyond(b)) > 2.5,
                    "outside paddock fence {i} stands inside the border"
                );
                self.fence_run(&format!("Outside Paddock Fence {stretch}"), a, b, false);
                stretch += 1;
            }
        }
        for (i, &(x, z, yaw)) in OUTSIDE_BALES.iter().enumerate() {
            assert!(
                border::beyond(vec2(x, z)) > 2.5,
                "outside bale {i} stands inside the border"
            );
            self.kit(
                "hay_bale",
                &format!("Arena Prop Outside Hay Bale {i}"),
                vec3(x, 0.0, z),
                yaw,
            );
        }
    }

    /// A kit model standing on the ground at `at`, turned `yaw` degrees,
    /// with its solids, `(centre, size, yaw)` in its frame, each risen to
    /// the shell-blocking top.
    fn landmark(
        &mut self,
        model: &str,
        name: &str,
        at: Vec2,
        yaw: f32,
        solids: &[(Vec3, Vec3, f32)],
    ) -> calyx_engine::scene::GameObject {
        let root = self.kit(model, name, vec3(at.x, 0.0, at.y), yaw);
        for (i, &(center, size, turn)) in solids.iter().enumerate() {
            let (center, size) = blocking_shells(center, size);
            self.tilted_collider(
                root,
                &format!("{name} collision {i}"),
                center,
                size,
                UnitQuaternion::from_euler_angles(0.0, turn.to_radians(), 0.0),
            );
        }
        root
    }

    /// A stretch of fence from `a` to `b`, `Arena Prop {name} Piece ...`:
    /// the kit's 4 m fence modules stretched to fill it. Inside the pen
    /// (`solid`) it has one solid along it, `Arena {name}`, that stops
    /// tanks but lets rounds and sight through, as the bridges' rails do.
    fn fence_run(&mut self, name: &str, a: Vec2, b: Vec2, solid: bool) {
        let along = b - a;
        let length = along.norm();
        let yaw = (-along.y).atan2(along.x).to_degrees();
        let modules = (length / 4.0).round().max(1.0) as usize;
        let step = length / modules as f32;
        for k in 0..modules {
            let at = a + along * ((k as f32 + 0.5) / modules as f32);
            let piece = self.kit(
                "fence",
                &format!("Arena Prop {name} Piece {k}"),
                vec3(at.x, 0.0, at.y),
                yaw,
            );
            let mut pose = self.scene.transform(piece);
            pose.scale = vec3(step / 4.0, 1.0, 1.0);
            self.scene.set_transform(piece, &pose.matrix());
        }
        if !solid {
            return;
        }
        let middle = (a + b) * 0.5;
        let object = self.tilted_collider(
            self.arena,
            &format!("Arena {name}"),
            vec3(middle.x, 0.6, middle.y),
            vec3(length + 0.2, 1.2, 0.3),
            UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
        );
        self.scene
            .write_component::<ComponentCollider, _>(object, |collider| {
                collider.groups = TANK_BLOCKER;
            });
    }
}
