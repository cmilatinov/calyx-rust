//! PROTOTYPE (poise_poc, #610): the prototype's HUD. Poise bars over every
//! tank with poise in place of health bars, the player's poise and ability
//! cooldowns, and the hook line.

use super::abilities::ComponentTankAbilities;
use super::logic::Stance;
use super::{ComponentDummyDriver, ComponentPoise};
use crate::hud::paint::{Painter, BLACK, CYAN, LIGHT, MUTED, PANEL, RED};
use crate::hud::{finish_hud, Projection};
use crate::tank::ComponentTankController;
use calyx_engine::component::{Component, ComponentHud, ReflectComponent, ReflectComponentHud};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{TextAnchor, UiArena, UiColor, UiNodeHandle, UiRect};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};

/// The prototype's signal colour: knockdowns, the hook and ready abilities.
const AMBER: UiColor = UiColor::rgba(255, 178, 48, 255);
/// The hook line's rope.
const ROPE: UiColor = UiColor::rgba(222, 196, 150, 255);
/// The spring boot's leather and its sole.
const LEATHER: UiColor = UiColor::rgba(150, 86, 42, 255);
const SOLE: UiColor = UiColor::rgba(52, 38, 30, 255);
/// The spring's steel.
const SPRING: UiColor = UiColor::rgba(205, 214, 222, 255);
/// Coils of the boot's spring per metre it is stretched.
const COILS_PER_METRE: f32 = 2.2;
/// How many times its metre-and-a-half outline the boot is drawn.
const BOOT_SCALE: f32 = 1.6;
/// Half the width of the spring's coils, in metres.
const COIL_RADIUS: f32 = 0.28;

/// Width of a poise bar over a tank, in HUD units.
const BAR_WIDTH: f32 = 72.0;
const BAR_HEIGHT: f32 = 8.0;

/// PROTOTYPE (#610). Draws poise, the boot and hook cooldowns and the hook
/// line for the player's tank. A tank driven by a dummy draws nothing.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "01421efa-8077-4d4a-b80e-6d5942597cd4"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentHud)]
#[reflect_attr(name = "Poise HUD (prototype)")]
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
        let abilities =
            scene.read_component::<ComponentTankAbilities, _, _>(player, |abilities| *abilities);
        if let Some(projection) = Projection::scene(scene, width, height) {
            if let Some(abilities) = &abilities {
                paint_hook_line(&mut p, scene, &projection, abilities);
                paint_boot(&mut p, &projection, abilities);
            }
            for tank in scene.objects_with::<ComponentPoise>() {
                paint_tank_bar(&mut p, scene, &projection, player, tank);
            }
        }
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

/// The hook line from the gun to the hook's head or the hooked tank: ink
/// outline, rope core and a head.
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
    let latched = abilities.hook_latched();
    let core = if latched { AMBER } else { ROPE };
    p.line(a, b, BLACK, if latched { 9.0 } else { 7.0 });
    p.line(a, b, core, if latched { 4.5 } else { 3.0 });
    p.circle(b.0, b.1, 8.0, BLACK);
    p.circle(b.0, b.1, 5.0, core);
}

/// The spring boot while it is out: a zigzag spring from the tank to a big
/// boot, toe first, both inked.
fn paint_boot(p: &mut Painter, projection: &Projection, abilities: &ComponentTankAbilities) {
    let Some((origin, head, direction)) = abilities.boot_spring() else {
        return;
    };
    let side = nalgebra_glm::vec3(-direction.z, 0.0, direction.x);
    let length = (head - origin).magnitude();
    let turns = ((length * COILS_PER_METRE).ceil() as usize).max(1);
    let mut points = Vec::with_capacity(turns * 2 + 2);
    points.push(origin);
    for i in 0..turns * 2 {
        let t = (i as f32 + 0.5) / (turns * 2) as f32;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        points.push(origin + (head - origin) * t + side * COIL_RADIUS * sign);
    }
    points.push(head);
    let screen: Vec<_> = points
        .iter()
        .filter_map(|&point| projection.point(point))
        .collect();
    if screen.len() != points.len() {
        return;
    }
    for pair in screen.windows(2) {
        p.line(pair[0], pair[1], BLACK, 7.0);
    }
    for pair in screen.windows(2) {
        p.line(pair[0], pair[1], SPRING, 3.0);
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

/// The poise bar over `tank`, with its state: a break, a knockdown counting
/// down to recovery, a hook or immunity.
fn paint_tank_bar(
    p: &mut Painter,
    scene: &Scene,
    projection: &Projection,
    player: GameObject,
    tank: GameObject,
) {
    let Some(poise) = scene.read_component::<ComponentPoise, _, _>(tank, Clone::clone) else {
        return;
    };
    if !scene.is_visible_in_hierarchy(tank) {
        return;
    }
    let Some(bounds) = projection.visual_bounds(scene, tank) else {
        return;
    };
    let x = (bounds.left + bounds.right) * 0.5;
    let y = bounds.top - 10.0;
    let w = BAR_WIDTH;
    let h = BAR_HEIGHT;
    let friendly = tank == player;
    let state = poise.state();
    let (ratio, color) = match state.stance {
        Stance::Upright => (
            ratio(state.poise, poise.max_poise),
            if friendly { CYAN } else { RED },
        ),
        Stance::KnockedDown { .. } | Stance::Righting { .. } => (poise.down_progress(), AMBER),
        Stance::Hooked => (0.0, AMBER),
    };
    let outline = if state.is_immune() { LIGHT } else { BLACK };
    p.fill(
        x - w * 0.5 - 3.0,
        y - h * 0.5 - 3.0,
        w + 6.0,
        h + 6.0,
        outline,
        6.0,
    );
    p.fill(x - w * 0.5, y - h * 0.5, w, h, PANEL, 4.0);
    if ratio > 0.0 {
        p.fill(x - w * 0.5, y - h * 0.5, w * ratio, h, color, 4.0);
    }
    let tag = match state.stance {
        Stance::KnockedDown { .. } => Some(("DOWN", AMBER)),
        Stance::Righting { .. } => Some(("RECOVERING", AMBER)),
        Stance::Hooked => Some(("HOOKED", AMBER)),
        Stance::Upright if state.is_immune() => Some(("IMMUNE", LIGHT)),
        Stance::Upright => None,
    };
    if let Some((text, color)) = tag {
        p.label(
            x,
            y - h - 8.0,
            text,
            17.0,
            color,
            TextAnchor::Center,
            None,
            true,
            Some(3.0),
        );
    }
    // A break pops a big word over the tank.
    if poise.since_break < 0.9 {
        let age = poise.since_break;
        let fade = ((0.9 - age) / 0.3).clamp(0.0, 1.0);
        let pop = 1.0 + 0.5 * (1.0 - age / 0.15).clamp(0.0, 1.0);
        p.label(
            x,
            y - h - 34.0 - age * 30.0,
            "BREAK!",
            34.0,
            AMBER.with_alpha(fade),
            TextAnchor::Center,
            None,
            true,
            Some(4.0),
        );
        if let Some(calyx_engine::ui::PaintCommand::Label(label)) = p.commands.last_mut() {
            label.scale = pop;
        }
    }
}

fn ratio(value: f32, max: f32) -> f32 {
    if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The bottom panel: the player's poise, and the boot and hook with their
/// keys and cooldowns.
fn paint_panel(
    p: &mut Painter,
    scene: &Scene,
    player: GameObject,
    hud: &ComponentPoiseHud,
    abilities: Option<&ComponentTankAbilities>,
    width: f32,
    height: f32,
) {
    let Some(poise) = scene.read_component::<ComponentPoise, _, _>(player, Clone::clone) else {
        return;
    };
    let panel_w = (width * 0.3).clamp(360.0, 560.0);
    let panel_h = 104.0;
    let x = (width - panel_w) * 0.5;
    let y = height - hud.screen_margin - panel_h - if hud.show_controls { 22.0 } else { 0.0 };
    p.panel(x, y, panel_w, panel_h, 10.0);

    // Poise.
    let state = poise.state();
    let bar_x = x + 18.0;
    let bar_w = panel_w - 36.0;
    let bar_y = y + 34.0;
    let (fill, color, word) = match state.stance {
        Stance::Upright if state.is_immune() => {
            (ratio(state.poise, poise.max_poise), LIGHT, "IMMUNE")
        }
        Stance::Upright => (ratio(state.poise, poise.max_poise), CYAN, "POISE"),
        Stance::KnockedDown { .. } | Stance::Righting { .. } => {
            (poise.down_progress(), AMBER, "KNOCKED DOWN")
        }
        Stance::Hooked => (0.0, AMBER, "HOOKED"),
    };
    p.label(
        bar_x,
        y + 20.0,
        word,
        16.0,
        color,
        TextAnchor::CenterLeft,
        None,
        true,
        None,
    );
    p.label(
        bar_x + bar_w,
        y + 20.0,
        format!("{:.0}", state.poise.max(0.0)),
        16.0,
        LIGHT,
        TextAnchor::CenterRight,
        None,
        true,
        None,
    );
    p.fill(bar_x, bar_y, bar_w, 12.0, BLACK, 4.0);
    if fill > 0.0 {
        p.fill(bar_x, bar_y, bar_w * fill, 12.0, color, 4.0);
    }

    // Abilities.
    let Some(abilities) = abilities else {
        return;
    };
    let slot_w = (bar_w - 12.0) * 0.5;
    let slot_y = y + 56.0;
    let slots = [
        (
            "E",
            "BOOT",
            abilities.boot_cooldown_remaining,
            abilities.boot_cooldown,
            abilities.since_boot < 0.2,
        ),
        (
            "Q",
            "HOOK",
            abilities.hook_cooldown_remaining,
            abilities.hook_cooldown,
            abilities.hook_latched(),
        ),
    ];
    for (i, (key, name, remaining, cooldown, active)) in slots.into_iter().enumerate() {
        let sx = bar_x + i as f32 * (slot_w + 12.0);
        let ready = remaining <= 0.0;
        p.fill(sx, slot_y, slot_w, 34.0, BLACK, 6.0);
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
        p.fill(sx, slot_y, slot_w * charged, 34.0, tint, 6.0);
        p.fill(sx + 4.0, slot_y + 4.0, 26.0, 26.0, LIGHT, 4.0);
        p.label(
            sx + 17.0,
            slot_y + 17.0,
            key,
            17.0,
            BLACK,
            TextAnchor::Center,
            None,
            false,
            None,
        );
        p.label(
            sx + 40.0,
            slot_y + 17.0,
            name,
            17.0,
            if ready { LIGHT } else { MUTED },
            TextAnchor::CenterLeft,
            None,
            true,
            None,
        );
        if !ready {
            p.label(
                sx + slot_w - 10.0,
                slot_y + 17.0,
                format!("{remaining:.1}"),
                15.0,
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
            "WASD drive  ·  LMB / Space shoot  ·  E spring boot at the cursor  ·  hold Q or RMB to hook, swing with the turret, release to throw",
            14.0,
            LIGHT,
            TextAnchor::Center,
            None,
            false,
            Some(3.0),
        );
    }
}
