# Sandbox Gameplay

The sandbox is the tank game: tanks, sentries, rounds, fog of war, brush and a
HUD, built as project components on the engine. Its look is in
[Tank Game Visual Direction](../art/tank-game-direction.md) and its HUD in
[HUD Visual Direction](../hud-ui/visual-direction.md). Each module's rustdoc
holds its own rules; this doc keeps the ones that span modules.

Code: `sandbox/src/`.

## Input

`sandbox/assets/input.cxinput` is the sandbox's action map: `shoot` (primary
button, Space), `jump` (Space), `scoreboard` (Tab), `menu_confirm` (Enter),
`menu_select` (primary button), and the `move_forward` (W/S) and `move_right`
(D/A) axes.

## Combat

`combat.rs` records kill events. A projectile records its shooter, and a
lethal hit becomes a `KillEvent` passed to `record_kill`, which counts kills
and deaths on `ComponentCombatStats` and adds the kill to every
`ComponentKillFeed`. Networked kills are meant to enter through the same
`record_kill`. `match_rules.rs` reads those kills to end a match.

## Combat effects

`combat_vfx.rs`:

- The tank and sentry share a signal-amber shell, a muzzle burst and
  world-space smoke. Hits spawn additive sparks on targets or alpha-blended
  dust on static cover.
- A `ComponentCombatEffectLifetime` follows each round with its trail emitter,
  stops emission when the round disappears, and deletes the emitter after the
  last particles drain. One-shot emitters also expire; the particle
  simulation drops their GPU state.
- Rounds reuse the approved look-dev sphere and emissive materials, with a
  round head and a narrow luminous tail; visual children compensate the
  physics root scale.
- `ComponentCombatPalette` selects Sunny (default), Sunbaked or Moonlit
  signals for the scene. Moonlit uses its weaker shell amber and reduced
  shell/muzzle lights, independently of sodium lamps. Damaging hits use a
  brighter, briefly sustained streak burst in every palette.

## Collision groups

`collision.rs` defines the sandbox's groups: `HULL_GROUP` (1), `ROUNDS_GROUP`
(2), `SIGHT_GROUP` (3) and `BRUSH_GROUP` (4).

- The hull's sweep and turn, a round's flight and arena sight each ask with
  their own groups (`HULL`, `ROUNDS`, `SIGHT`), so a static collider stops
  only what it is a member of; a default collider is a member of all of them.
- `TANK_BLOCKER` stops the hull alone, for water: rounds, the cursor aim
  (drive surfaces only), sentry rounds, fog of war and the minimap's sight
  cross it.
- `BRUSH` is a member of `BRUSH_GROUP` only and interacts with nothing, for
  the volumes of brush patches.

The engine side of groups is in [Physics](../engine/physics.md#collision-groups).

## Brush

`ComponentBrush` (`brush.rs`) is a tall-grass patch whose shape is the
primitive colliders on it and its descendants, in the `BRUSH` collision
group, which stops no tank, round or sight.

- A tank in a patch is hidden from every side and sentry outside it,
  `TeamVision::concealed` and `sees_unit` carrying the rule to fog, health
  bars, damage numbers and minimap blips. Tanks in the same patch see each
  other.
- Firing from a patch reveals the shooter for the patch's `reveal_time`
  (`ComponentBrushReveal`).
- The patch's grass is a `ComponentFoliage` of `tall_grass` clumps drawn with
  the engine's [grass](../rendering/grass.md) shader. Every frame the first
  patch hands the scene's `ComponentGrassField` the tanks the local player
  sees as benders (never a tank fog hides), which bend the grass, and around
  the player in brush and the tanks it sees in the same patch fade it, truly
  transparent.

## Track marks

`ComponentTrackMarks` (`track_marks.rs`): tanks give the scene's terrain the
component when play starts.

- Each frame it stamps the two treads of every tank that moved on the terrain
  (not on a bridge or deck, and not while fog hides it) into a byte-per-texel
  field over the terrain's rectangle, 8 texels a metre by default.
- The prints keep the grousers still on the ground as a tread rolls, and
  turning in place sweeps them round into scuffs.
- Every 0.25 s each tile of 32 by 32 texels holding marks fades by what the
  time is worth, so a print is gone after `fade_time`. Only the tiles stamped
  or faded that frame are written to the texture (`Texture::writable_rgba8`
  and `write_rgba8`), and a faded tile drops out, so the cost follows the
  fresh marks, not the distance driven.
- The terrain draws a copy of its material (`Material::duplicate`) whose
  ground `marks_texture` is that texture, tinted per map palette, so the
  material asset itself never changes ([Terrain](../rendering/terrain.md#ground-shader)).

## Vision

`vision.rs` decides what a side sees and fogs the rest; it writes the fog of
war sight every frame (`vision/sight.rs`), whose rendering is in
[Post-Processing Architecture](../post-processing/architecture.md#fog-of-war).

## HUD

`ComponentTankHud` (`hud.rs` and `hud/`) draws health, weapon, a minimap drawn
from world positions, the kill feed, the menu, and a scoreboard while the
`scoreboard` action is held.
