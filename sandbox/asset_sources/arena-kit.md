# Concept-sheet arena kit

The 23 models below follow [the concept sheet](../../docs/design/art/concept-sheet.png).
Each has one named-part parameter file in `models/`, shared generated part meshes,
and one prefab for each map under `assets/generated/prefabs/{sunny,sunbaked,moonlit}/`.
Regenerate meshes, materials and prefabs from the repository root with
`cargo run -p calyx_model_gen --` (Blender 4.0 at its default path).

The look-dev `Showcase` lays out every model on all three maps. The `Scenario`
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
| `barrier_hazard`, `barrier_plain` | Rounded Jersey body, wide foot, two lifting loops; hazardous version has clipped diagonal stripes across both sloping shoulders and the top | Convex concrete body and foot |
| `crate_framed`, `crate_large` | 1.1 m and 1.65 m slatted timber boxes; inset seams, diagonal braces, silver corner wraps and pins | Box around the solid crate |
| `rock_small`, `rock_medium`, `rock_large` | Distinct faceted silhouettes, 0.85/1.45/2.05 m wide, with grass at the base | Convex rock body |
| `tree_small`, `tree`, `tree_large` | Three sizes; tapered/forked trunk, organic roots and unequal offset crown clusters in dark/middle/light foliage, leaning forked trunks and irregular roots | Box around trunk/root base; canopy remains decorative |
| `flag` | Red folded-outline banner, pole and weighted circular base | Box around pole/base |
| `lamp` | Post and framed warm lantern with pyramidal cap | Box around pole/base |
| `berry_bush` | Low fused leaf clusters with visible red berries | Convex bush body |
| `grass_tuft` | Five tapered leaves in a fan | Normally non-blocking; a bounds box is available if an interaction needs one |
| `sandbag_stack` | Three staggered rows of soft bags with restrained seam bands | Box around the stack |
| `pickup_yellow`, `pickup_blue` | Emissive orb over an octagonal pad | Box around pad; collection trigger belongs to gameplay |

Every opaque part material has `outline_style = 1`; pickup signal orbs use 0
to retain their glow. Palette roles and construction rules are in
[the direction guide](../../docs/design/art/tank-game-direction.md).

Moonlit uses the merged authored night rig unchanged (moon strength 7, sky 1,
bloom 4.5 / threshold 2.5 / knee 0.1 / radius 6). Its primary kit roles match
the direction reference; additional timber/foliage details stay in cool steel
and pine shades. The lantern uses sodium `#FF9330` at strength 4. Pickup
emission is 1.2 at night so the strong night bloom retains a bounded halo.
Approved Sunny and Sunbaked materials remain unchanged.
