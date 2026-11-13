//! PROTOTYPE (poise_poc, #610): the prototype's HUD. Each tank's knockout
//! meter as a percentage over it, the knockouts, the player's abilities and
//! their cooldowns, and the tells: the hook line running out, the spring
//! boot winding up and kicking, and anchors.

use super::abilities::ComponentTankAbilities;
use super::logic::Stance;
use super::{ComponentDummyDriver, ComponentKnockout};
use crate::hud::paint::{Painter, BLACK, CYAN, LIGHT, MUTED, RED};
use crate::hud::{finish_hud, Projection};
use crate::tank::ComponentTankController;
use calyx_engine::component::{Component, ComponentHud, ReflectComponent, ReflectComponentHud};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{PaintCommand, TextAnchor, UiArena, UiColor, UiNodeHandle, UiRect};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::{vec3, Vec3};
use serde::{Deserialize, Serialize};

/// The prototype's signal colour: launches, the hook and ready abilities.
const AMBER: UiColor = UiColor::rgba(255, 178, 48, 255);
/// The hook line's rope.
const ROPE: UiColor = UiColor::rgba(222, 196, 150, 255);
/// The spring boot's leather and its sole.
const LEATHER: UiColor = UiColor::rgba(150, 86, 42, 255);
const SOLE: UiColor = UiColor::rgba(52, 38, 30, 255);
/// The spring's steel.
const SPRING: UiColor = UiColor::rgba(205, 214, 222, 255);
/// The anchor's tell.
const ANCHOR: UiColor = UiColor::rgba(120, 200, 255, 255);
/// Coils of the boot's spring per metre it is stretched.
const COILS_PER_METRE: f32 = 2.2;
/// How many times its metre-and-a-half outline the boot is drawn.
const BOOT_SCALE: f32 = 1.6;
/// Half the width of the spring's coils, in metres.
const COIL_RADIUS: f32 = 0.28;
/// The radius of the anchor's ring round a tank, in metres.
const ANCHOR_RADIUS: f32 = 2.2;

/// The meter's colours, from calm to danger, at 0, 50, 100 and 150%.
const METER_COLORS: [(f32, [u8; 3]); 4] = [
    (0.0, [235, 242, 247]),
    (50.0, [255, 214, 64]),
    (100.0, [255, 138, 30]),
    (150.0, [232, 40, 40]),
];

/// PROTOTYPE (#610). Draws the knockout meters, the knockouts, the
/// abilities and their tells for the player's tank. A tank driven by a dummy
/// draws nothing.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "01421efa-8077-4d4a-b80e-6d5942597cd4"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentHud)]
#[reflect_attr(name = "Knockout HUD (prototype)")]
#[serde(default)]
pub struct ComponentPoiseHud {
    /// Distance of the ability panel from the bottom of the screen.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub screen_margin: f32,
    /// Show the controls under the ability panel.
    pub show_controls: bool,
}

impl Default for ComponentPoiseHud {
    fn default() -> Self {
        Self {
            screen_margin: 30.0,
            show_controls: true,
        }
    }
}

impl Component for ComponentPoiseHud {}

impl ComponentHud for ComponentPoiseHud {
    fn build_hud(
        hud: &Self,
        scene: &Scene,
        player: GameObject,
        ui: &mut UiArena,
        viewport: UiRect,
    ) -> Option<UiNodeHandle> {
        scene.read_component::<ComponentTankController, _, _>(player, |_| ())?;
        if scene
            .read_component::<ComponentDummyDriver, _, _>(player, |_| ())
            .is_some()
        {
            return None;
        }
        let mut p = Painter::new(viewport);
        let width = viewport.width() / p.scale;
        let height = viewport.height() / p.scale;
        let tanks: Vec<_> = scene.objects_with::<ComponentKnockout>().collect();
        if let Some(projection) = Projection::scene(scene, width, height) {
            for &tank in &tanks {
                if let Some(abilities) =
                    scene.read_component::<ComponentTankAbilities, _, _>(tank, |a| *a)
                {
                    paint_hook_line(&mut p, scene, &projection, &abilities);
                    paint_boot(&mut p, &projection, &abilities);
                }
            }
            for &tank in &tanks {
                paint_tank_meter(&mut p, scene, &projection, player, tank);
            }
        }
        paint_knockouts(&mut p, scene, player, &tanks, width);
        let abilities =
            scene.read_component::<ComponentTankAbilities, _, _>(player, |abilities| *abilities);
        paint_panel(
            &mut p,
            scene,
            player,
            hud,
            abilities.as_ref(),
            width,
            height,
        );
        Some(finish_hud(ui, p, viewport))
    }
}

/// The meter's colour at `percent`, from calm to danger.
pub(crate) fn meter_color(percent: f32) -> UiColor {
    let percent = percent.max(0.0);
    let mut color = METER_COLORS[METER_COLORS.len() - 1].1;
    for pair in METER_COLORS.windows(2) {
        let (from, a) = pair[0];
        let (to, b) = pair[1];
        if percent <= to {
            let t = ((percent - from) / (to - from)).clamp(0.0, 1.0);
            let mix = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
            color = [mix(0), mix(1), mix(2)];
            break;
        }
    }
    UiColor::rgba(color[0], color[1], color[2], 255)
}

fn lerp_color(a: UiColor, b: UiColor, t: f32) -> UiColor {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    UiColor::rgba(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b), mix(a.a, b.a))
}

/// The hook line from the gun to the hook's head or the hooked tank: ink
/// outline, rope core and a head. While a tank is hooked the rope turns
/// from amber to red as the swing's time runs out, and frays at the end.
fn paint_hook_line(
    p: &mut Painter,
    scene: &Scene,
    projection: &Projection,
    abilities: &ComponentTankAbilities,
) {
    let Some(end) = abilities.hook_end(scene) else {
        return;
    };
    let (Some(a), Some(b)) = (
        projection.point(abilities.hook_origin),
        projection.point(end),
    ) else {
        return;
    };
    let Some(left) = abilities.hook_time_left() else {
        p.line(a, b, BLACK, 7.0);
        p.line(a, b, ROPE, 3.0);
        p.circle(b.0, b.1, 8.0, BLACK);
        p.circle(b.0, b.1, 5.0, ROPE);
        return;
    };
    let core = lerp_color(RED, AMBER, left);
    let width = 3.0 + 2.5 * left;
    if left > 0.3 {
        p.line(a, b, BLACK, width + 5.0);
        p.line(a, b, core, width);
    } else {
        // Fraying: the line breaks into strands that jitter apart.
        let strands = 3;
        let spread = (0.3 - left) * 30.0;
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (nx, ny) = (-dy / length, dx / length);
        for strand in 0..strands {
            let offset = (strand as f32 - 1.0) * spread * 0.5;
            let mid = (
                (a.0 + b.0) * 0.5 + nx * offset * 2.0,
                (a.1 + b.1) * 0.5 + ny * offset * 2.0,
            );
            p.line(a, mid, BLACK, 4.0);
            p.line(mid, b, BLACK, 4.0);
            p.line(a, mid, core, 2.0);
            p.line(mid, b, core, 2.0);
        }
    }
    p.circle(b.0, b.1, 9.0, BLACK);
    p.circle(b.0, b.1, 6.0, core);
}

/// The spring boot: a zigzag spring from the tank to a big boot, toe first,
/// both inked. Winding up, the spring compresses tight against the tank.
fn paint_boot(p: &mut Painter, projection: &Projection, abilities: &ComponentTankAbilities) {
    let Some(view) = abilities.boot_view() else {
        return;
    };
    let direction = view.direction;
    let side = vec3(-direction.z, 0.0, direction.x);
    let (head, turns, coil) = match view.winding {
        // Compressing: the boot draws back towards the tank, coils packed.
        Some(t) => (
            view.origin + direction * (0.9 - 0.6 * t),
            6,
            COIL_RADIUS * 1.4,
        ),
        None => {
            let length = (view.head - view.origin).magnitude();
            (
                view.head,
                ((length * COILS_PER_METRE).ceil() as usize).max(1),
                COIL_RADIUS,
            )
        }
    };
    let origin = view.origin;
    let mut points = Vec::with_capacity(turns * 2 + 2);
    points.push(origin);
    for i in 0..turns * 2 {
        let t = (i as f32 + 0.5) / (turns * 2) as f32;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        points.push(origin + (head - origin) * t + side * coil * sign);
    }
    points.push(head);
    let screen: Vec<_> = points
        .iter()
        .filter_map(|&point| projection.point(point))
        .collect();
    if screen.len() != points.len() {
        return;
    }
    let steel = if view.winding.is_some() {
        AMBER
    } else {
        SPRING
    };
    for pair in screen.windows(2) {
        p.line(pair[0], pair[1], BLACK, 7.0);
    }
    for pair in screen.windows(2) {
        p.line(pair[0], pair[1], steel, 3.0);
    }
    // The boot, seen from above: a sole with a rounded toe pointing out
    // along the spring, and the shaft over its heel.
    let at = |forward: f32, across: f32| head + (direction * forward + side * across) * BOOT_SCALE;
    let outline = [
        at(-0.35, -0.38),
        at(0.55, -0.42),
        at(0.95, -0.25),
        at(1.05, 0.0),
        at(0.95, 0.25),
        at(0.55, 0.42),
        at(-0.35, 0.38),
        at(-0.5, 0.0),
    ];
    let sole: Option<Vec<_>> = outline
        .iter()
        .map(|&point| projection.point(point))
        .collect();
    let shaft: Option<Vec<_>> = [
        at(-0.45, -0.32),
        at(0.2, -0.34),
        at(0.2, 0.34),
        at(-0.45, 0.32),
    ]
    .iter()
    .map(|&point| projection.point(point + Vec3::y() * 0.5 * BOOT_SCALE))
    .collect();
    let (Some(sole), Some(shaft)) = (sole, shaft) else {
        return;
    };
    let grow = |points: &[(f32, f32)], by: f32| -> Vec<(f32, f32)> {
        let n = points.len() as f32;
        let cx = points.iter().map(|q| q.0).sum::<f32>() / n;
        let cy = points.iter().map(|q| q.1).sum::<f32>() / n;
        points
            .iter()
            .map(|&(x, y)| {
                let (dx, dy) = (x - cx, y - cy);
                let d = (dx * dx + dy * dy).sqrt().max(1e-3);
                (x + dx / d * by, y + dy / d * by)
            })
            .collect()
    };
    p.polygon(&grow(&sole, 3.0), BLACK);
    p.polygon(&sole, SOLE);
    p.polygon(&grow(&sole, -3.0), LEATHER);
    p.polygon(&grow(&shaft, 3.0), BLACK);
    p.polygon(&shaft, LEATHER);
}

/// A label with a pop, scaled up for its first moments.
#[allow(clippy::too_many_arguments)]
fn pop_label(p: &mut Painter, x: f32, y: f32, text: &str, size: f32, color: UiColor, age: f32) {
    let fade = ((0.9 - age) / 0.3).clamp(0.0, 1.0);
    let pop = 1.0 + 0.5 * (1.0 - age / 0.15).clamp(0.0, 1.0);
    p.label(
        x,
        y - age * 30.0,
        text,
        size,
        color.with_alpha(fade),
        TextAnchor::Center,
        None,
        true,
        Some(4.0),
    );
    if let Some(PaintCommand::Label(label)) = p.commands.last_mut() {
        label.scale = pop;
    }
}

/// The meter over `tank`: its percentage, coloured by danger, and its
/// state; an anchor's ring round it.
fn paint_tank_meter(
    p: &mut Painter,
    scene: &Scene,
    projection: &Projection,
    player: GameObject,
    tank: GameObject,
) {
    let Some(knockout) = scene.read_component::<ComponentKnockout, _, _>(tank, Clone::clone) else {
        return;
    };
    let state = knockout.state();
    let position = scene.world_transform(tank).position;
    if state.is_knocked_out() {
        if let Some((x, y)) = projection.point(position.map(|v| v.max(-2.0))) {
            p.label(
                x,
                y,
                "KO!",
                40.0,
                RED,
                TextAnchor::Center,
                None,
                true,
                Some(4.0),
            );
        }
        return;
    }
    if state.is_anchored() {
        let points: Vec<_> = (0..=24)
            .filter_map(|i| {
                let angle = i as f32 / 24.0 * std::f32::consts::TAU;
                projection.point(position + vec3(angle.cos(), 0.15, angle.sin()) * ANCHOR_RADIUS)
            })
            .collect();
        for pair in points.windows(2) {
            p.line(pair[0], pair[1], BLACK, 8.0);
        }
        for pair in points.windows(2) {
            p.line(pair[0], pair[1], ANCHOR, 4.0);
        }
    }
    let Some(bounds) = projection.visual_bounds(scene, tank) else {
        return;
    };
    let x = (bounds.left + bounds.right) * 0.5;
    let y = bounds.top - 14.0;
    let friendly = tank == player;
    p.fill(x - 40.0, y - 15.0, 80.0, 30.0, BLACK.with_alpha(0.75), 8.0);
    p.fill(
        x - 40.0,
        y + 11.0,
        80.0,
        4.0,
        if friendly { CYAN } else { RED },
        2.0,
    );
    p.label(
        x,
        y - 1.0,
        format!("{:.0}%", state.percent),
        24.0,
        meter_color(state.percent),
        TextAnchor::Center,
        None,
        true,
        Some(3.0),
    );
    let tag = match state.stance {
        Stance::KnockedDown { .. } => Some(("DOWN", AMBER)),
        Stance::Righting { .. } => Some(("RECOVERING", AMBER)),
        Stance::Hooked => Some(("HOOKED", AMBER)),
        _ if state.is_invulnerable() => Some(("RESPAWNED", LIGHT)),
        _ if state.is_anchored() => Some(("ANCHORED", ANCHOR)),
        _ if state.is_immune() => Some(("IMMUNE", LIGHT)),
        _ => None,
    };
    if let Some((text, color)) = tag {
        p.label(
            x,
            y - 28.0,
            text,
            16.0,
            color,
            TextAnchor::Center,
            None,
            true,
            Some(3.0),
        );
    }
    if knockout.since_launch < 0.9 {
        pop_label(
            p,
            x,
            y - 52.0,
            "LAUNCH!",
            32.0,
            AMBER,
            knockout.since_launch,
        );
    }
}

/// The knockouts at the top of the screen: how many times each tank has
/// been knocked out.
fn paint_knockouts(
    p: &mut Painter,
    scene: &Scene,
    player: GameObject,
    tanks: &[GameObject],
    width: f32,
) {
    let count = |tank: GameObject| {
        scene
            .read_component::<ComponentKnockout, _, _>(tank, |knockout| knockout.state.knockouts)
            .unwrap_or(0)
    };
    let yours = count(player);
    let theirs: u32 = tanks
        .iter()
        .filter(|&&tank| tank != player)
        .map(|&tank| count(tank))
        .sum();
    let w = 420.0;
    p.panel((width - w) * 0.5, 18.0, w, 46.0, 8.0);
    p.label(
        width * 0.5,
        41.0,
        format!("KNOCKED OUT    YOU {yours}    ENEMY {theirs}"),
        20.0,
        LIGHT,
        TextAnchor::Center,
        None,
        true,
        None,
    );
}

/// The bottom panel: the player's meter, and its abilities with their keys
/// and cooldowns.
fn paint_panel(
    p: &mut Painter,
    scene: &Scene,
    player: GameObject,
    hud: &ComponentPoiseHud,
    abilities: Option<&ComponentTankAbilities>,
    width: f32,
    height: f32,
) {
    let Some(knockout) = scene.read_component::<ComponentKnockout, _, _>(player, Clone::clone)
    else {
        return;
    };
    let panel_w = (width * 0.4).clamp(520.0, 720.0);
    let panel_h = 136.0;
    let x = (width - panel_w) * 0.5;
    let y = height - hud.screen_margin - panel_h - if hud.show_controls { 22.0 } else { 0.0 };
    p.panel(x, y, panel_w, panel_h, 10.0);

    // The meter.
    let state = knockout.state();
    let inner_x = x + 18.0;
    let inner_w = panel_w - 36.0;
    p.label(
        inner_x,
        y + 34.0,
        format!("{:.0}%", state.percent),
        44.0,
        meter_color(state.percent),
        TextAnchor::CenterLeft,
        None,
        true,
        Some(3.0),
    );
    let word = match state.stance {
        Stance::KnockedDown { .. } => "TUMBLING  ·  WASD STEER  ·  SHIFT/Z BOOST",
        Stance::Righting { .. } => "RECOVERING",
        Stance::Hooked => "HOOKED",
        Stance::KnockedOut { .. } => "KNOCKED OUT",
        Stance::Upright if state.is_invulnerable() => "RESPAWNED  ·  INVULNERABLE",
        Stance::Upright if state.is_anchored() => "ANCHORED",
        Stance::Upright => "KNOCKOUT METER",
    };
    p.label(
        inner_x + inner_w,
        y + 34.0,
        word,
        16.0,
        LIGHT,
        TextAnchor::CenterRight,
        None,
        true,
        None,
    );

    // The abilities, three to a row.
    let Some(a) = abilities else {
        return;
    };
    let slots = [
        (
            "E",
            "BOOT",
            a.boot_cooldown_remaining,
            a.boot_cooldown,
            a.since_kick < 0.2,
        ),
        (
            "Q",
            "HOOK",
            a.hook_cooldown_remaining,
            a.hook_cooldown,
            a.hook_latched(),
        ),
        (
            "SHIFT/Z",
            "BOOST",
            a.boost_cooldown_remaining,
            a.boost_cooldown,
            false,
        ),
        (
            "CTRL/C",
            "ANCHOR",
            a.anchor_cooldown_remaining,
            a.anchor_cooldown,
            state.is_anchored(),
        ),
        (
            "R",
            "MINE",
            a.mine_cooldown_remaining,
            a.mine_cooldown,
            false,
        ),
        ("F", "OIL", a.oil_cooldown_remaining, a.oil_cooldown, false),
    ];
    let slot_w = (inner_w - 2.0 * 10.0) / 3.0;
    for (i, (key, name, remaining, cooldown, active)) in slots.into_iter().enumerate() {
        let sx = inner_x + (i % 3) as f32 * (slot_w + 10.0);
        let sy = y + 62.0 + (i / 3) as f32 * 36.0;
        let ready = remaining <= 0.0;
        p.fill(sx, sy, slot_w, 30.0, BLACK, 6.0);
        let charged = if ready || cooldown <= 0.0 {
            1.0
        } else {
            1.0 - (remaining / cooldown).clamp(0.0, 1.0)
        };
        let tint = if active {
            AMBER
        } else if ready {
            AMBER.with_alpha(0.35)
        } else {
            MUTED.with_alpha(0.35)
        };
        p.fill(sx, sy, slot_w * charged, 30.0, tint, 6.0);
        let key_w = 14.0 + 9.0 * key.len() as f32;
        p.fill(sx + 3.0, sy + 3.0, key_w, 24.0, LIGHT, 4.0);
        p.label(
            sx + 3.0 + key_w * 0.5,
            sy + 15.0,
            key,
            14.0,
            BLACK,
            TextAnchor::Center,
            None,
            false,
            None,
        );
        p.label(
            sx + key_w + 10.0,
            sy + 15.0,
            name,
            15.0,
            if ready { LIGHT } else { MUTED },
            TextAnchor::CenterLeft,
            None,
            true,
            None,
        );
        if !ready {
            p.label(
                sx + slot_w - 8.0,
                sy + 15.0,
                format!("{remaining:.1}"),
                14.0,
                LIGHT,
                TextAnchor::CenterRight,
                None,
                false,
                None,
            );
        }
    }
    if hud.show_controls {
        p.label(
            width * 0.5,
            y + panel_h + 14.0,
            "WASD drive  ·  LMB / Space shoot  ·  E spring boot at the cursor  ·  hold Q or RMB to hook, swing with the turret  ·  knock them into the water or off the cliff",
            14.0,
            LIGHT,
            TextAnchor::Center,
            None,
            false,
            Some(3.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::meter_color;

    #[test]
    fn the_meter_turns_from_calm_to_danger() {
        let calm = meter_color(0.0);
        let danger = meter_color(200.0);
        assert_eq!((calm.r, calm.g, calm.b), (235, 242, 247));
        assert_eq!((danger.r, danger.g, danger.b), (232, 40, 40));
        // Greener and bluer the lower the meter.
        assert!(meter_color(30.0).b > meter_color(120.0).b);
        assert!(meter_color(60.0).g > meter_color(140.0).g);
    }
}
