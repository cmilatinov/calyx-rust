//! The northeast corner, the barn yard (#579): a red barn beside the
//! platform's south ramp, its door gable facing the feed yard at the ramp's
//! foot; the tall silo in the cove by the east border; and a fenced paddock
//! in the lobe north of the platform, with more paddocks, bales and a hay
//! stack framing the lobe outside its wall. The feed yard in front of the
//! barn's door is left open, worn dirt (`layout::FOOTPRINTS`' `Barn`), a
//! clearing objectives can use; a feed trough lines its edge along the
//! barn yard wall.
//!
//! The barn, the silo and the trough stop tanks, rounds and sight like any
//! cover. The paddock's rail fence stops tanks only, like the bridges'
//! rails: rounds and sight cross it, so the lobe keeps its long sightlines.
//! The paddock leaves a gate wider than a tank. Positions are in the
//! Arena's frame.

use calyx_engine::component::ComponentCollider;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use sandbox::collision::TANK_BLOCKER;

use super::{blocking_shells, border, farmland::Creek, Author};

/// The barn: its centre and yaw. Its long axis runs at 40 degrees to the
/// arena's Z, along the creek's bank, its door gable (the model's +X)
/// facing the feed yard, its east corner against the south ramp's kerb.
/// Tanks pass it on the creek's side.
pub const BARN: (f32, f32, f32) = (13.2, 14.5, 50.0);
/// The barn's walls, the solid: length (its X), height and depth (its Z).
const BARN_WALLS: Vec3 = Vec3::new(6.6, 3.2, 4.6);

/// The silo's centre, in the cove by the east border, and its solid: an
/// octagon this far from the centre to each side, round its tower. It
/// leaves a lane wider than a tank to the south ramp's kerb, the way round
/// the platform's east side to the lobe.
pub const SILO: Vec2 = Vec2::new(27.4, 13.8);
const SILO_APOTHEM: f32 = 1.8;

/// The yard's furniture, `(model, name, x, z, yaw)`: bales behind the barn
/// against the ramp, filling the closed gap between them; a trough along
/// the barn yard wall at the feed yard's edge; and one in the paddock along
/// its fence. Each stands flush with what it lines, leaving no strip a tank
/// could be stranded in.
const PROPS: [(&str, &str, f32, f32, f32); 4] = [
    ("hay_bale", "Barn Hay Bale 0", 16.3, 16.9, 35.0),
    ("hay_bale", "Barn Hay Bale 1", 15.9, 18.0, 44.0),
    ("feed_trough", "Feed Yard Trough", 15.31, 6.34, -43.0),
    ("feed_trough", "Paddock Trough", 17.13, 32.64, -38.0),
];

/// The paddock's rail fence inside the pen, a run of straight stretches
/// through its points: a V at 38 and 43 degrees under the lobe's north
/// wall, the wall closing the paddock behind it, open at its east end by a
/// gate wider than a tank. Each stretch is one kit module, stretched.
const PADDOCK_FENCES: [&[(f32, f32)]; 1] = [&[(13.56, 33.71), (16.5, 31.0), (18.86, 32.85)]];

/// Outside the lobe's wall: paddock fences, `(x, z, yaw)`, kit modules 4 m
/// long, and a hay stack of bales, `(x, z, yaw)`.
const OUTSIDE_FENCES: [(f32, f32, f32); 8] = [
    (13.0, 43.5, 20.0),
    (16.8, 42.1, 20.0),
    (20.6, 40.7, 20.0),
    (24.0, 38.9, 38.0),
    (27.1, 36.4, 38.0),
    (30.6, 34.0, 30.0),
    (34.1, 32.0, 30.0),
    (35.8, 30.0, 70.0),
];
const OUTSIDE_BALES: [(f32, f32, f32); 5] = [
    (21.5, 44.5, 20.0),
    (22.9, 44.0, 20.0),
    (22.2, 45.4, 20.0),
    (31.5, 38.2, 75.0),
    (37.8, 33.5, 30.0),
];

impl Author {
    /// The barn yard: the barn, the silo, their furniture, the paddocks
    /// and the framing outside the lobe.
    pub fn barn_yard(&mut self, creek: &Creek) {
        let (x, z, yaw) = BARN;
        self.landmark(
            "barn",
            "Arena Prop Barn",
            vec2(x, z),
            yaw,
            &[(vec3(0.0, BARN_WALLS.y * 0.5, 0.0), BARN_WALLS, 0.0)],
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
                "feed_trough" => self.landmark(
                    model,
                    &name,
                    vec2(x, z),
                    yaw,
                    &[(vec3(0.0, 0.375, 0.0), vec3(2.4, 0.75, 1.1), 0.0)],
                ),
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
                self.paddock_fence(stretch, a, b);
                stretch += 1;
            }
        }
        for (i, &(x, z, yaw)) in OUTSIDE_FENCES.iter().enumerate() {
            assert!(
                border::beyond(vec2(x, z)) > 2.5,
                "outside paddock fence {i} stands inside the border"
            );
            self.kit(
                "fence",
                &format!("Arena Prop Outside Paddock Fence {i}"),
                vec3(x, 0.0, z),
                yaw,
            );
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
    ) {
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
    }

    /// A stretch of paddock fence from `a` to `b`: the kit's 4 m fence
    /// modules stretched to fill it, and one solid along it, `Arena
    /// Paddock Fence {index}`, that stops tanks but lets rounds and sight
    /// through, as the bridges' rails do.
    fn paddock_fence(&mut self, index: usize, a: Vec2, b: Vec2) {
        let along = b - a;
        let length = along.norm();
        let yaw = (-along.y).atan2(along.x).to_degrees();
        let modules = (length / 4.0).round().max(1.0) as usize;
        let step = length / modules as f32;
        for k in 0..modules {
            let at = a + along * ((k as f32 + 0.5) / modules as f32);
            let piece = self.kit(
                "fence",
                &format!("Arena Prop Paddock Fence {index} Piece {k}"),
                vec3(at.x, 0.0, at.y),
                yaw,
            );
            let mut pose = self.scene.transform(piece);
            pose.scale = vec3(step / 4.0, 1.0, 1.0);
            self.scene.set_transform(piece, &pose.matrix());
        }
        let middle = (a + b) * 0.5;
        let solid = self.tilted_collider(
            self.arena,
            &format!("Arena Paddock Fence {index}"),
            vec3(middle.x, 0.6, middle.y),
            vec3(length + 0.2, 1.2, 0.3),
            UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
        );
        self.scene
            .write_component::<ComponentCollider, _>(solid, |collider| {
                collider.groups = TANK_BLOCKER;
            });
    }
}
