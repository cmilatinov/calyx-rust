# Concept-sheet arena kit

The 26 models below follow [the concept sheet](../../docs/design/art/concept-sheet.png).
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
| `sandbag_stack` | Three staggered rows of soft bags, 1.27 m total height, with restrained seam bands | Box around the stack |
| `pickup_yellow`, `pickup_blue` | Green and blue glowing orbs over octagonal pads; legacy yellow name retains stable IDs | Box around pad; collection trigger belongs to gameplay |
| `sentry` | Anchored circular base with broad mounting pads and exposed yaw bearing; red player-2 head with clipped armor cheeks, pitch hubs, recoil sleeve, open muzzle, roof cooling banks and shaded optic | Cylinder around base, radius 0.57 m and height 0.15 m, plus a box around the solid head if cover collision is desired; visual integration preserves existing gameplay |
| `explosive_barrel` | Red explosive drum with a centered yellow-and-black lid flame badge, opposing diamond flame plaques, narrow hazard hoops and rolled rims | Cylinder, radius 0.455 m and height 1.125 m, centered 0.5625 m above the ground pivot; explosion behavior belongs to cover gameplay |
| `aim_reticle` | Flat open ring and four ticks, light marks over a dark contrast border; 1.66 m overall width | Non-blocking, no collider |

Every opaque prop part material has `outline_style = 1`; pickup signal orbs
and both reticle materials use 0. The reticle's explicit dark border stays
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
