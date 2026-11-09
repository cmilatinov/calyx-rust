# Concept-sheet arena kit

The 28 models below follow [the concept sheet](../../docs/design/art/concept-sheet.png).
Each has one named-part parameter file in `models/`, shared generated part meshes,
and one prefab for each map under `assets/generated/prefabs/{sunny,sunbaked,moonlit}/`.
Regenerate meshes, materials and prefabs from the repository root with
`cargo run -p calyx_model_gen --` (Blender 4.0 at its default path).

The look-dev `Showcase` lays out the original 23 kit models on all three maps. The `Scenario`
contains a U-shaped wall pen, repeated straight joins, cover clusters, trees,
rocks, props and the existing four player tanks. All retain the Far camera
framing and the map's original lighting/post-processing rig.

## Tileable wall connections

The root placement grid is **1 metre**, with 2 m and 4 m straight modules. All wall roots have ground-level pivots,
height 1.6 m and thickness 1.2 m. Open connection faces use the same rounded
profile (0.12 m bevel); their exported contact vertices are checked by
`every_tileable_wall_open_end_has_the_same_exported_contact_profile`.
The junctions have longer arms so their L/T/cross silhouettes remain clear
from the Far camera. Their port coordinates are whole metres, so all adjoining module roots
remain on the one-metre placement grid.
Pieces meet at their connection faces without a through-gap. The bevel leaves
a narrow intentional seam, as in the concept sheet's individual rounded blocks.

Coordinates below are `(x,z)` relative to the root, before yaw rotation.

| Model | Open connection centres | Collider handoff |
| --- | --- | --- |
| `wall_short` | `(-1,0)`, `(1,0)` | Box, 2 x 1.6 x 1.2 m |
| `wall_long` | `(-2,0)`, `(2,0)` | Box, 4 x 1.6 x 1.2 m |
| `wall_corner` | `(3,0)`, `(0,3)` | Two arm boxes; keep the inner corner open |
| `wall_t` | `(-3,0)`, `(3,0)`, `(0,3)` | Arm boxes; keep the two inner corners open |
| `wall_cross` | `(-3,0)`, `(3,0)`, `(0,-3)`, `(0,3)` | Two crossing arm boxes |
| `wall_end` | `(1,0)` | Box; rounded closed end on negative X |

Rotate a module by multiples of 90 degrees and place the next module so their
connection centres coincide. The pen uses nine short pieces along the back
and six along each side, two corner modules and two end caps. The long module
replaces two short modules without moving either outer connection: short roots
at X=0 and X=2 have outer ports -1 and 3; a long root at X=1 has those same ports.

## Ramp and raised platform

Both stand on the 1 m root grid with ground-level pivots. Coordinates are
`(x,z)` relative to the root, before yaw rotation.

| Model | Construction | Ports | Collider handoff |
| --- | --- | --- | --- |
| `ramp` | 4 x 8 m wedge (`kind = "wedge"`) rising along +Z from the ground at Z = -4 to 1.6 m at Z = +4, a 1:5 slope, between 0.5 m kerbs that stand 0.5 m above it | crest edge `(0,4)`, 4 m wide | Tilted 0.6 m slab under the slope, marked as a drive surface; each kerb a tilted block from its sloped top 2.3 m down into the ground |
| `platform` | 10 x 8 m block, 1.6 m high like the walls, with a paved deck and a 0.5 m kerb round its rim | rim openings 4 m wide at `(0,-4)` (south) and `(-5,1)` (west) | Deck slab (top 0.4 m) marked as a drive surface over a blocking body; the rim kerbs as drawn |

A ramp whose crest meets a port continues its kerbs into the rim kerbs either
side of the opening. The wedge's sharp foot is rounded by a small bevel only.

## Remaining models and collider handoff

The shapes below are the choices for arena authoring in #495; the look-dev
proof scene does not add gameplay physics. `Convex` refers to the solid part,
excluding grass, paint plates, lifting loops and other decorative appendages.

| Models | Construction | Collider handoff |
| --- | --- | --- |
| `barrier_hazard`, `barrier_plain` | Rounded Jersey body, wide foot, two lifting loops; hazardous version has clipped diagonal stripes across both sloping side panels; the roof stays plain concrete | Convex concrete body and foot |
| `crate_framed`, `crate_large` | 1.1 m and 1.65 m slatted timber boxes; inset seams, diagonal braces, silver corner wraps and pins | Box around the solid crate |
| `rock_small`, `rock_medium`, `rock_large` | Distinct faceted silhouettes, 0.85/1.45/2.05 m wide, with grass at the base | Convex rock body |
| `tree_small`, `tree`, `tree_large` | Approved small/medium layered crowns; large broad unequal crown with visible organic branch forks and roots | Box around trunk/root base; canopy remains decorative |
| `flag` | Red folded-outline banner, pole and weighted circular base | Box around pole/base |
| `lamp` | Post and framed warm lantern with pyramidal cap | Box around pole/base |
| `berry_bush` | Low fused leaf clusters with visible red berries | Convex bush body |
| `grass_tuft` | Five tapered leaves in a fan | Normally non-blocking; a bounds box is available if an interaction needs one |
| `tall_grass` | Brush clump: five big plain blades, tapered straps 0.6 to 0.75 m wide and 3.4 to 3.9 m long leaning out 18 to 42 degrees, dark or mid bodies with light tips (`tall_grass`, `tall_grass_light`, `tall_grass_tip`), drawn with the grass shader | Non-blocking: the patch, not the clump, has volumes, `BRUSH`-group boxes that stop nothing (see Brush patches) |
| `sandbag_stack` | Three staggered rows of soft bags, 1.27 m total height, with restrained seam bands | Box around the stack |
| `pickup_yellow`, `pickup_blue` | Green and blue glowing orbs over octagonal pads; legacy yellow name retains stable IDs | Box around pad; collection trigger belongs to gameplay |
| `sentry` | Anchored circular base with broad mounting pads and exposed yaw bearing; red player-2 head with clipped armor cheeks, pitch hubs, recoil sleeve, open muzzle, roof cooling banks and shaded optic | Cylinder around base, radius 0.57 m and height 0.15 m, plus a box around the solid head if cover collision is desired; visual integration preserves existing gameplay |
| `explosive_barrel` | Red explosive drum with a centered yellow-and-black lid flame badge, opposing diamond flame plaques, narrow hazard hoops and rolled rims | Cylinder, radius 0.455 m and height 1.125 m, centered 0.5625 m above the ground pivot; explosion behavior belongs to cover gameplay |
| `aim_reticle` | Flat open ring and four ticks, light marks over a dark contrast border; 1.66 m overall width | Non-blocking, no collider |

Every opaque prop part material has `outline_style = 3`, the props' outline
layer; walls and the ramp and platform bodies and kerbs use 1, the terrain's.
Pickup signal orbs and both reticle materials use 0. The reticle's explicit dark border stays
readable on light ground without the screen-space ink thickening its marks.
Palette roles and construction rules are in
[the direction guide](../../docs/design/art/tank-game-direction.md).

The Sentry faces +Z. Its ground pivot is at the foot of the base, the `head`
part starts at `(0, 0.3, 0)`, and its muzzle ends at `(0, 0.5, 1.2)` relative
to that pivot. The gameplay Sentry root stays at its original height of
0.5 m; its generated visual root sits 0.5 m below it, and `ComponentSentry.head`
references the head child. Thus `muzzle_offset = 1.2` still fires exactly at
the visible muzzle, with the same projectile height, range and shooter identity.
The barrel, muzzle, mantlet, hatch, armor, pitch hubs, cooling banks and optic
name `parent = "head"` in the source; their offsets are local to that part.
The pedestal, yaw bearing and mounting pads stay fixed. Part names, not source order, identify
parents, meshes and prefab objects. A missing parent or cycle is rejected.

The explosive drum puts its largest warning on the lid, which dominates at
the Far camera's 68-degree pitch. The black flame and yellow inner lick are
closed relief meshes, with a yellow field around them; opposing side plaques
repeat the symbol for lower views. The red body separates this prop from a
generic utility drum. The badge and flame share the lid's center, with no
offset bung. Rolled rims retain its familiar drum construction.
The Sentry's red center remains visible between pale cheek armor and paired
dark cooling banks. The recoil sleeve, lateral pitch hubs and anchor pads
describe how the machine turns, fires and stays planted. These are broad
mechanical forms rather than a field of small bolts. Its cyan optic is matte
and non-emissive, so it cannot compete with combat signals on Moonlit.

The arena contains a linked Sunny Sentry prefab under its gameplay root and
the Sunny reticle geometry. The player prefab carries the same reticle under
its Player root, so prefab instances
remap the controller's crosshair reference. `spawn.rs` only moves the existing
player rig during respawn and creates no crosshair visual. All three map
prefabs are available for arena authoring. The barrel is intentionally absent
from the live arena until cover gameplay places it.

Moonlit retains the authored night lighting and bloom (moon strength 7, sky 1,
bloom 4.5 / threshold 2.5 / knee 0.1 / radius 6). Its primary kit roles match
the direction reference; additional timber/foliage details stay in cool steel
and pine shades. The lantern uses sodium `#FF9330` at strength 4. Pickup
emission (Sunny/Sunbaked/Moonlit) is 12/12/6 for green and 24/24/16 for blue,
with distinct colored halos at Far. The combat props add `explosive_red`,
`sentry_armor` and `sentry_optic` roles; existing kit material colors remain unchanged.
All three look-dev rigs use 2 px inner ink; outer ink and thresholds are unchanged.

## Authored gameplay arena

The gameplay arena uses linked Sunny kit instances inside a closed 60 by 64 m
pen. The existing Arena object remains at world `(0, 0, 22)` and supplies the
minimap bounds. Its `generate_on_start` is off: saved kit children and their
colliders are the layout. Empty test arenas keep the simple grid generator by
default; adding a decorative child does not disable it.

One-metre grass, sand, dirt, paving and road tiles use flat clay materials
without ink. Uniform surfaces share scaled tile instances on whole-metre
rectangles; paving joints and marked road dashes stay on individual tiles.
The tiles share one uninterrupted collision floor at y = 0, so seams and
paving joints do not catch tank tracks. Ground tiles also cover a 200 by
200 m apron around the pen, to fill the camera view at the walls even with
maximum zoom-out and aim lead; the foundation, a backstop the camera never
sees, lies 1.3 m under them so it clears the creek's water. Neither adds
collision outside the closed playable pen. The ground materials and meshes are
generated for all three palettes alongside the prop kit.

The two sand flank lanes stay clear for tanks. Staggered barriers, crates,
sandbags and rocks break shots across the middle while leaving the player's
initial firing lane open. Gameplay actor ids and model scales stay
independent of the kit layout. The player spawns at the origin; targets are
at (-6, 0.5, 24) and (6, 0.5, 32), and the sentry is at (16, 0.5, 32),
beside the foot of the south ramp. Spawn-side cover begins beyond the first six
metres, while the broad sand lanes run near X = +/-20.

The raised area fills the northeast pocket. The platform's root is at local
`(20, 23)` (world Z 45), its deck 1.6 m up, 3.4 m clear of the north and east
walls so tanks can circle it. The south ramp, root `(20, 15)`, continues the
east sand lane up to the deck; the west ramp, root `(11, 24)` turned 90
degrees, climbs from the middle of the arena, north of the rear crates. A
sandbag stack on the deck at `(17, 21)` gives cover overlooking the middle.

Interior walls, each an `Arena Wall ...` object holding its kit modules,
divide the pen into lanes and corners. A corner shields the spawn from the
east sand lane (`Southeast Corner`, root at `(16, -25)`; the farmland's
farmhouse, hedgerow and water tower do so on the west), a run across the west middle at z = 11 splits the west lane
from the centre (`West Middle`), a corner at `(-6, 25)` faces the raised area
across the north (`Northwest Corner`), and a run at x = 10 screens the west
ramp's approach from the centre (`East Middle`). Their modules join at their
connection ports, and each straight run has one continuous solid, as the
perimeter does, rising to the shell-blocking top. Every wall leaves the sand
lanes, the ramps' feet and the spawn's first six metres clear, with gaps wider
than a tank (at least 2.8 m) to other obstacles and 4.8 m to the perimeter.
Like every wall they block sight and cast fog shadows; the minimap outlines
their solids as it does cover, while the perimeter strips, the Arena's own
children, bound its floor.

Tanks ride drive surfaces (`ComponentDriveSurface`: the floor, ramp slabs and
the deck): the hull never collides with them, settles on the surface under
its centre, nose and tail, and pitches with the slope. Kerbs, the platform
body and everything else stop the hull, so tanks can only leave the deck by
a ramp. The cursor aims at the drive surface under it. A round leaves the
gun at its height above the shooter's ground and flies straight to the same
height above the aimed ground: level on the flat, diving from the deck,
climbing towards it. Every cover solid rises 2 m above the ground it stands
on, so cover on the deck stops rounds fired across the deck as cover on the
floor does. Ramps and kerbs are slopes and edges, not cover: a level round
passes over a ramp's low end, and a shot from the deck clears its rim.

The straight perimeter uses four continuous box strips matching the wall
modules, without internal collider seams. Crates use their solid box, and
barriers and rocks use
stacked boxes for their changing silhouette. Rock slabs are derived from the
exported triangles, with narrower height bands at the crown (at most sixteen
per rock); they conservatively approximate contours rather than exact convex
geometry. Sandbags use three thin rows matching their stepped pile. Tree
trunks, flag poles and lamp posts have collision; foliage, lifting loops and paint are decorative.
This uses the engine's existing primitive colliders. The minimap reads the
authored cover colliders' current transforms, including scene edits.

`cargo run -p sandbox --example remake_arena` rebuilds only the Arena object's
authored children with deterministic ids and engine-created prefab links.
It assigns the explicit actor placements and initial Far camera above,
preserving other component data and all actor ids/prefab links. Run
the model generator first after changing a tile or kit parameter file.

## Farmland front

The southwest quadrant (x < 0, z < 0 in the Arena's frame) is a farmland
front, Sunny Battlefield's setting, built by `remake_arena`'s `farmland`
module. It reads by detail rather than height:

- **Creek.** One Water component, `Arena Creek`, 0.6 m below the ground,
  meanders in under the west wall, bends past the farm and leaves under the
  south wall, 5.5 to 8 m wide, continuing outside both walls. Its banks are
  the component's, in the generated `creek_bank` material (the dirt's
  colours on the terrain's ink layer), with their lip 2 cm above the tiles.
  The ground tiles leave out every cell that reaches into the trench, and
  rotated grass strips along both banks cover the cells' steps under the
  lip. Stones stand in the stream, with foam rings, and reeds on its banks.
- **Collision.** `Arena Creek Blocker` boxes, 1.6 m pieces along the
  centreline as wide as the water plus 0.5 m a side and overlapping the
  next, are in the sandbox's `TANK_BLOCKER` collision groups: they stop
  tanks short of the water but let rounds, sight and the aim across.
  Colliders are primitives, so the blockers follow the bends in pieces;
  either side of the bridge a block closes the creek up to the rails.
- **Crossings.** A timber bridge (`bridge`) carries the west sand lane over
  the creek, its deck 5 cm above the floor, its rails `TANK_BLOCKER` boxes.
  It is the only crossing; the spawn side reaches the rest of the pen north
  of the creek. There is no ford.
- **Landmarks.** The ruined farmhouse (`Arena Wall Farmhouse`) is two kit
  wall corners with doorways wider than 2.8 m, a 3.8 m `chimney` and a
  broken `roof_ruin`, with hay bales; the 9.7 m `water_tower` stands in the
  pen's southwest corner, roof and all clear of both walls, its solid
  leaving a gap too narrow for a tank. Two `hedgerow`s are cover that stops rounds and, like crates,
  sight.
- **Ground.** Two tilled fields with crop rows. The ground surface, track
  marks and the creek's route are left to later work; the
  `ground_grass_dark`, `ground_grass_light`, `ground_gravel` and
  `ground_track` tiles are in the kit but not placed.
- **Outside.** Fields with fences and copses of kit trees beyond the west
  and south walls, around the creek's continuation.

New models are generated in all three palettes like the kit: the ground
tiles `ground_grass_dark`, `ground_grass_light`, `ground_gravel`,
`ground_field`, `ground_crop`, `ground_track` and `creek_bank`, `hedgerow`, `water_tower`, `bridge`, `chimney`,
`roof_ruin`, `hay_bale` and `fence`, with the new roles `ground_grass_dark`,
`ground_grass_light`, `ground_gravel`, `ground_field`, `ground_crop`,
`ground_track`, `creek_bank` and `roof_tiles`. No third-party assets are
used.

## Brush patches

Tall-grass brush (`sandbox/src/brush.rs`) is placed as patches: an object with
the **Brush** component and a child box volume in the `BRUSH` collision
groups (memberships 8, filter 0) that stops no tank, round or sight, 2 m high
on the ground. Its grass stands one `tall_grass` clump to each cell of an
even grid of about 1.6 m over the volume, each cell a child with a
**Foliage** of exactly one clump placed at random within the middle of the
cell, with random yaw and 0.9 to 1.1 scale, casting no shadow (the grass
would shade mostly itself, at the cost of every blade in each shadow
cascade). The grid keeps the patch full to its edges without bunching, and
the clumps are instances, not game objects, so a patch draws in a few
instanced draws. Under the grass a `ground_grass_dark` tile, 1 m inside the
volume's edge, darkens the ground the blades lean out over. Any of the
primitive collider shapes works as a volume, and a patch may hold several.
Brush stands on the open floor, clear of cover, so a tank can drive into it
from any side.

The Arena carries the scene's **Grass Field**, whose wind and bend the grass
shader reads; the brush hands it the tanks moving through the grass every
frame.

`remake_arena` places three test patches until the layout work places brush
along the creek and the landmarks: `Arena Brush Centre` (7 x 5 m at local
`(3, -10)`, ahead of the spawn), `Arena Brush East` (5 x 6 m at `(13.5, 4)`) and
`Arena Brush North` (5 x 5 m at `(-4, 6)`), each clear of cover and of the
sentry's firing lane.
