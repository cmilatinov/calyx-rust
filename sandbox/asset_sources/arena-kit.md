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
| `tall_grass` | Brush clump: seven thin blades, straps 0.28 to 0.38 m wide tapering gently to a rounded end, 3.2 to 3.8 m long, rooted a little apart and each leaning 10 to 30 degrees its own way with a short arc, drooped by the grass shader, dark or mid green (`tall_grass`, `tall_grass_light`) | Non-blocking: the patch, not the clump, has volumes, `BRUSH`-group boxes that stop nothing (see Brush patches) |
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

The gameplay arena uses linked Sunny kit instances inside a closed pen whose
irregular border spans about 67 by 69 m (see the border below). The existing
Arena object remains at world `(0, 0, 22)`; its 68 by 70 m extent, the
border's bounding rectangle, supplies the minimap bounds. Its `generate_on_start` is off: saved kit children and their
colliders are the layout. Empty test arenas keep the simple grid generator by
default; adding a decorative child does not disable it.

The ground is one surface, `Arena Ground`: a flat Terrain over a 200 by
200 m apron around the pen, to fill the camera view at the walls even with
maximum zoom-out and aim lead, dropped 1.3 m only under the creek. Its
material blends grass, its darker and lighter patches, dirt, the sand lanes,
gravel, fields and mud from two masks `remake_arena` paints, with ink detail
textures (see Ground in `docs/design/art/tank-game-direction.md`). Only the
south spawn's paving keeps one-metre kit tiles, raised 3 cm off the ground,
its joints on individual tiles. Tanks drive on one
uninterrupted collision floor at y = 0, the `Arena Floor` drive surface,
which adds no collision outside the closed playable pen. The ground's masks,
heightmap and per-palette materials are written to
`assets/generated/ground/`; its detail textures there are drawn by
`asset_sources/ground/ground_detail.py`.

The map is asymmetric farmland, built by `remake_arena`'s `layout` module:
nothing on it mirrors. A creek comes in through the west border and forks in
the middle of the pen (see Farmland front), splitting it into three
sections: the south team's bank in the southwest, the north team's in the
northwest, and the contested east between the branches. The player's side
spawns at the `Player Spawn`, at the origin (local `(0, -22)`); team 1 at
the `North Spawn`, at local `(-10, 18.5)` facing south, on a gravel pad. Each
corner has its own character:

- **Southwest, the farm**: the ruined farmhouse, the water tower, a field,
  hedgerows, and the creek's deep bend into the south half.
- **Southeast, the supply depot**: a tractor yard in a lobe of the border
  past the south branch, behind the depot's wall, with a shed, a raised
  fuel tank, a parked tractor and stacked crates round an open gravel yard (footprint `Depot`,
  local x 21..38, z -37..-20; see Supply depot).
- **Northwest, the orchard**: rows of kit trees at 37 degrees broken by
  hedgerows, a wall and brush (footprint `Orchard`, x -27..-13.5,
  z 12..29), with the windmill's footprint in the corner (`Windmill`,
  x -27..-22, z 24.5..29.5), dressed by #580.
- **Northeast, the barn yard**: the raised platform, a red barn against
  its south ramp, a tall silo in the east cove, an open feed yard at the
  ramp's foot (`Barn`, x 12.5..21.5, z 6..12.5), and a stable and a fenced
  paddock in the wide lobe north of the platform, past the north branch
  (see Barn yard).

The footprints are `layout::FOOTPRINTS`; the ground paints the barn's feed
yard and the windmill's ground worn dirt and the silo's pad (`Silo`) and
the depot's gravel.
Sand lanes, painted on the ground and smoothed into curves, run from the
south spawn over the west bridge to the north spawn, from the south spawn
over the south bridge through the east to the south ramp, and from the
north spawn over the north bridge to the west ramp. Targets are at world
(-8, 0.5, 34.5) and (28, 0.5, 23), and the sentry at (20, 0.5, 23.5), in
the east between the barn yard wall and the east field's wall.

The raised area fills the northeast pocket. The platform's root is at local
`(20, 23)` (world Z 45), its deck 1.6 m up, 3.4 m clear of the north and east
walls so tanks can circle it. The south ramp, root `(20, 15)`, continues the
east sand lane up to the deck; the west ramp, root `(11, 24)` turned 90
degrees, climbs from the middle of the arena, north of the rear crates. A
sandbag stack on the deck at `(17, 21)` gives cover overlooking the middle.

Interior walls, each an `Arena Wall ...` object holding its kit modules,
run at 35 to 45 degrees to the arena's axes, each at its own angle, as do
the hedgerows and the lines of barriers, sandbags and crates: `East Field`
breaks the long sightline across the east between the south bridge and the
barn yard, `Depot` closes the depot's yard, `Farm` shields the farm's field and
water tower, `Orchard` edges the orchard towards the creek, and `Barn Yard`
screens the barn yard from the fork. Each is a straight run, an end piece,
a long module and an end piece joined at their connection ports, in an
object turned to the wall's angle, with one continuous solid along the run,
as the border has, rising to the shell-blocking top. Every wall leaves
the ramps' feet and the spawns' first six metres clear, with gaps wider than
a tank (at least 2.8 m) to other obstacles and 4.8 m to the border. Like
every wall they block sight and cast fog shadows; the minimap outlines
their solids as it does cover.

The border (`remake_arena`'s `border` module) is an irregular outline of
59 short stretches, no two alike and none longer than about 11 m, turning
every which way: lobes push out round the orchard in the northwest, the
barn yard in the northeast, the middle of the east, the supply depot in
the southeast and the farm in the southwest; peninsulas reach north
behind the north spawn and south behind the south spawn; coves bite in
between them, by the silo, above the depot and either side of the creek's
west gap. The creek comes in through a gap
on the west and its branches leave through inlets on the north and the
south. It is a field boundary: a dry-stone wall, the kit's long modules
stretched to fit each stretch, with a boulder set at every bend
(`Arena Prop Border Bend ...`, decoration within the overlapping solids),
and a hedgerow round the orchard's lobe. Each stretch is an object turned
along it holding its pieces and one solid along its whole length, reaching
half its thickness past each bend so neighbouring solids overlap, rising
to the shell-blocking top. Stone stretches are `Arena Wall Border ...`, on
the terrain's outline layer; the hedgerows `Arena Cover Border Hedgerow
...`, props, which block sight as the walls do. Where the creek leaves, the
border's solids stop just past its shore, in the creek's blockers, and its
pieces stop on the bank. Outside the border the grass grows rank, a band
of darker verge along the wall, then fields, fences and copses of trees, so
nothing outside reads as playable. The minimap outlines the border's
solids, since no rectangle bounds the floor; the Arena's extent, which the
minimap fits and the camera checks cover, is the border's bounding
rectangle, as is the `Arena Floor`.

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

Each border segment has one continuous box, without internal collider
seams. Crates use their solid box, and
barriers and rocks use
stacked boxes for their changing silhouette. Rock slabs are derived from the
exported triangles, with narrower height bands at the crown (at most sixteen
per rock); they conservatively approximate contours rather than exact convex
geometry. Sandbags use three thin rows matching their stepped pile. Tree
trunks, flag poles and lamp posts have collision; foliage, lifting loops and paint are decorative.
This uses the engine's existing primitive colliders. The minimap reads the
authored cover colliders' current transforms, including scene edits.

`cargo run --release -p sandbox --example remake_arena` rebuilds only the Arena object's
authored children with deterministic ids and engine-created prefab links.
It assigns the explicit actor placements and initial Far camera above,
preserving other component data and all actor ids/prefab links. Run
the model generator first after changing a tile or kit parameter file, and
`python sandbox/asset_sources/ground/ground_detail.py sandbox/assets/generated/ground`
after changing the ground's detail textures. Run it in release: it builds
in seconds there, where a debug build takes minutes painting the ground's
masks.

## Farmland front

The farmland, Sunny Battlefield's setting, is built by `remake_arena`'s
`farmland` module. It reads by detail rather than height:

- **Creek.** Two Water components, 0.6 m below the ground. `Arena Creek`
  meanders in under the west border, bends deep into the south half past
  the farm, and at the fork in the middle of the pen runs on as the north
  branch, winding north to leave through the north border's inlet;
  `Arena Creek South Branch` starts inside it at the fork and runs
  south-east, bending east, to leave through the south border's inlet. Both
  are 5.5 to 8 m wide and keep meandering outside the border to the apron's
  edge. Where they meet each opens into the other (the water's `openings`):
  neither draws a bank or a shore along a shore lying in the other's water,
  so the fork reads as one water. Their banks are the component's, in the
  generated `creek_bank` material (the dirt's colours on the terrain's ink
  layer), with their lip 2 cm above the ground. The ground drops under the
  water inside the bank wall, so the lip covers the step, and its mud
  frays out from the banks. Stones stand in the stream, with foam rings,
  and reeds on its banks.
- **Collision.** `Arena Creek Blocker` boxes, 1.6 m pieces along each
  water's centreline as wide as the water plus 0.5 m a side and
  overlapping the next, are in the sandbox's `TANK_BLOCKER` collision
  groups: they stop tanks short of the water but let rounds, sight and the
  aim across. Colliders are primitives, so the blockers follow the bends in
  pieces; either side of each bridge, in the bridge's frame, a block closes
  the creek up to the rails.
- **Crossings.** Three timber bridges (`bridge`), one over each stretch, at
  uneven positions far apart: `Arena Prop Bridge West` over the creek's
  diagonal run before the fork, joining the two western sections;
  `Arena Prop Bridge North` high up the north branch, by the platform's west
  ramp; `Arena Prop Bridge South` over the south branch on the east lane,
  by the south spawn. Each stands square across its water where the
  centreline passes nearest its point, its deck 5 cm above the floor, its
  rails `TANK_BLOCKER` boxes. There is no ford: with the decks closed no
  drive joins any two sections, and each bridge alone joins exactly the two
  either side of it.
- **Fairness.** Each team's drive from its spawn to its nearest bridge, and
  to the ground nearest the map's centre on its own bank, takes within 15 %
  as long as the other team's (`tests/feature_regressions.rs`).
- **Landmarks.** The ruined farmhouse (`Arena Wall Farmhouse`) is two kit
  wall corners with doorways wider than 2.8 m, a 3.8 m `chimney` and a
  broken `roof_ruin`, with hay bales; the 9.7 m `water_tower` stands well
  inside the farm's lobe, east of the field and south of the farm's wall,
  about 11 m from the border. The `hedgerow`s are cover that stops
  rounds and, like crates, sight.
- **Ground.** A tilled field with crop rows by the water tower, the
  farmhouse's dirt yard, gravel under the water tower and mud along the
  creek, all drawn by the ground's material.
- **Outside.** A verge of darker grass along the border, then fields with
  fences and copses of kit trees all round, around the creek's and its
  branch's continuations.

New models are generated in all three palettes like the kit: the ground
tiles `ground_grass_dark`, `ground_grass_light`, `ground_gravel`,
`ground_field`, `ground_crop`, `ground_track` and `creek_bank`, `hedgerow`, `water_tower`, `bridge`, `chimney`,
`roof_ruin`, `hay_bale` and `fence`, with the new roles `ground_grass_dark`,
`ground_grass_light`, `ground_gravel`, `ground_field`, `ground_crop`,
`ground_track`, `creek_bank` and `roof_tiles`; and for the barn yard
`barn`, `silo` and `feed_trough`, with the roles `barn_boards`,
`barn_trim` and `barn_roof`. No third-party assets are used.

## Barn yard

The northeast corner is dressed by `remake_arena`'s `barn_yard` module as
a barn and silo, a fairly open yard with long sightlines, distinct from the
other corners. Its border is pushed out round it: the lobe north of the
platform reaches about 16 m past the platform's north rim, and the wall
east of the platform about 15 m past its east rim, so the way round the
platform and the paddock have room. Positions are local, in the Arena's
frame.

- **Barn** (`barn`, `Arena Prop Barn`, at `(14.9, 13.8)` turned 90
  degrees): red board walls 6.6 by 4.6 m and 3.2 m to the eaves under a
  pitched stained timber roof (`barn_roof`, with shingle battens in the crates' timber,
  `crate_frame`), 6.5 m to its cupola, cream corner boards and window
  frames (`barn_trim`), a braced sliding door under a hayloft door in the
  gable that faces south over the feed yard, another hayloft door in the
  back gable. Its ridge runs north to south; its east wall meets the south
  ramp's kerb, so tanks pass it on the creek's side, a lane about 5 m wide
  from the fork to the north bridge's foot. Its solid is the walls' box,
  risen to the shell-blocking top like any cover; the roof is decoration.
- **Stable** (the `barn` model at 0.62 scale, `Arena Prop Stable`, at
  `(31, 34)`, not turned): in the lobe, its ridge running east to west,
  square to the barn's, as buildings in a corner stand at 90 degrees to
  each other. Its solid is its scaled walls' box.
- **Silo** (`silo`, `Arena Prop Silo`, at `(31, 15)`): a concrete stave
  tower (`barriers`) 3.4 m across and 9.6 m high on a stone footing, five
  steel hoops, a galvanised dome and vent cap, a ladder up the yard side;
  11.8 m high, the corner's tall landmark, taller than the farm's water
  tower. Its back meets the cove's wall, on a gravel pad; it leaves a lane
  over 6 m wide to the south ramp's kerb. With the lobe's west end closed
  by the creek and the west ramp, that lane and the way round the
  platform's east side are how tanks reach the lobe. Its solid is an
  octagon of four crossed boxes, 1.8 m from its centre to each side.
- **Feed yard**: worn dirt in front of the barn's door at the ramp's foot,
  left open as a clearing.
- **Paddock**: under the lobe's north wall, two runs of rail fence (the
  kit's `fence`, stretched) at 39 and 38 degrees, the wall closing the
  paddock behind them, with a gate between them about 5 m wide, and a
  `feed_trough` inside. The fence's solids, `Arena Paddock Fence ...`, are
  in the `TANK_BLOCKER` groups like the bridges' rails: they stop tanks,
  but rounds and sight cross them, so the lobe keeps its sightlines.
- **Loose furniture**: the paddock's trough, the `Lobe Hay Bale` east of
  the stable and the `Barn Crate` north of the platform are expected to
  become physical objects. Each stands alone in open ground, at least a
  tank's width clear of everything, so no lane or cover depends on it and
  pushing it about jams nothing. The `Barn Barrier`, east of the platform,
  is fixed cover.
- **Outside**: beyond the lobe's wall, paddock fences run along it, with a
  stack of three bales and two loose bales, among the copses and the field
  to the east, so the corner reads as a farm beyond its wall too.

## Supply depot

The southeast corner, past the south branch, is the supply depot, built by
`remake_arena`'s `depot` module: a roomy tractor yard in the border's
south-east lobe, framed by a shed, a raised fuel tank, a parked tractor and
the depot's wall, unlike the farm's open field, the orchard's rows or the
barn yard. Positions are local to the Arena.

- **Border.** The lobe pushes the border out to about x 42 and z -41
  round the yard, its wall still turning every few metres: a bay bites in
  between the shed and the fuel tank and another along its south side, and
  the south branch leaves through the inlet at its west end.
- **Shed.** `Arena Cover Depot Supply Shed`, a closed timber `depot_shed`
  5.6 by 3.8 m on a concrete plinth, its ribbed galvanised roof about
  4.2 m up, at `(33.6, -21.8)` turned 40 degrees, its sliding door and
  window facing the yard and a lane behind it to the east border. One
  solid round its body, which stops tanks, rounds and sight, as supply
  cover does; a prop, it draws on the props' outline layer.
- **Fuel tank.** `Arena Cover Depot Fuel Tank`, the depot's tall landmark:
  a horizontal `fuel_tank` raised on a braced steel stand, its top about
  6.3 m up, in the lobe's south-east corner at `(38, -33)` turned -50
  degrees, square to the shed and along the border behind it. Its solid is
  a box round the stand; rounds stop at it, sight crosses it.
- **Tractor.** `Arena Cover Depot Tractor`, a yellow `tractor` parked along
  the depot wall at `(22.6, -18.3)` turned 38 degrees, nose to the shed.
  Low cover: one solid round it, which stops rounds but not sight.
- **Crates.** Two large crates on the yard's north-west edge
  (`Arena Cover Depot Stack Crate 0` and `1`), a pair by the south inlet
  (`Arena Cover Depot Inlet Crate 0` and `1`) and a framed crate between
  the shed and the yard (`... Stack Crate 2`), each its own object
  with the kit crate's solid, so they block sight. Three framed crates
  stacked on them (`Arena Prop Depot Stacked Crate ...`) stand above the
  shell-blocking top. The crates are expected to become loose physics
  objects that tanks shove and rounds knock over, so none of them shapes a
  lane or the yard: each stands in the open with room round it to tumble,
  away from the walls, the buildings and the south bridge's exit.
- **Yard.** The middle, a circle 9 m across round `(31, -30)`
  (`depot::YARD`), stays open gravel, room for two tanks to fight and
  ground an objective can use. Tanks come in from the north along the
  creek, from the cove by the depot wall's end, round behind the shed and
  between the shed and the fuel tank.
- **Outside.** Beyond the border the depot carries on: a second shed
  square to the first, a parked tractor in the east field, stacked crates
  and a fence line along the south side (`Arena Prop Depot Outside ...`),
  without collision.

The new models are generated like the kit, in all three palettes, from
`depot_shed.toml`, `fuel_tank.toml` and `tractor.toml` in `models/`. They
use the kit's existing roles: timber in `crate_plank`, `crate_frame`,
`crate_panel` and `crate_brace`, concrete in `rocks`, steel in `metal`, the
galvanised roof and the tank in `metal_caps`, the tank's hoops in
`explosive_red`, the tractor and the hazard placard in `hazard_yellow` and
`hazard_black`, and tyres in `tracks`. No third-party assets are used.

## Brush patches

Tall-grass brush (`sandbox/src/brush.rs`) is placed as patches: an object with
the **Brush** component and child volumes in the `BRUSH` collision groups
(memberships 8, filter 0) that stop no tank, round or sight, 2 m high on the
ground. A patch is the union of its volumes, so its outline need not be a
rectangle: the arena's are turned boxes and upright capsules (circles on
the ground), overlapping into kidneys, blobs, strips, an L, a teardrop, a
peanut and a crescent. Its grass stands one `tall_grass` clump to each cell
of an even 0.9 m grid whose clump area, 0.72 m square, lies wholly inside
the outline; a cell whose full area leaves it gets a 0.36 m area and a
0.6 to 0.8 scale if that fits, and a third of those are left out, so the
patch thins out raggedly to its edge rather than ending in a cut. Each cell
is a child with a **Foliage** of exactly one clump placed at random in its
area, with random yaw and 0.9 to 1.1 scale, casting no shadow (the grass
would shade mostly itself, at the cost of every blade in each shadow
cascade). The clumps are instances, not game objects, so a patch draws in a
few instanced draws. The grass and the ground a tank hides on are one
shape: no clump stands outside the volumes (`tests/feature_regressions.rs`).
The ground paints darker grass under each patch, to just inside its
outline, where the blades lean out over it (`ground.rs`, from
`BRUSH_PATCHES`). Any of the primitive collider shapes works as a volume.
Brush stands on the open floor, clear of cover, so a tank can drive into it
from any side.

The Arena carries the scene's **Grass Field**, whose wind and bend the grass
shader reads; the brush hands it the tanks moving through the grass every
frame.

`remake_arena` places seven patches unevenly along the creek's banks, clear
of the water, and by the landmarks, each its own shape, size and angle
(`layout::BRUSH_PATCHES`): a kidney, `Creek West`, on the north bank of the
deep bend; a blob, `Bridge North`, past the west bridge; a thin strip with a
lobe, `Creek East`, along the south bank between the bridges; an L,
`Farmyard`, wrapping the farmhouse's south-east corner; a teardrop,
`Orchard`, in the orchard; a peanut, `Barn`, by the barn yard; and a
crescent, `Depot`, round the depot wall's end; each clear of cover and of
the sentry's firing lane. They hold 86 clumps, 602 blades.
