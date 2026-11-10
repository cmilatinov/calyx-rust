# Tank Game Visual Direction

The style guide for the sandbox tank game's tanks, arena and effects. The HUD
follows its [visual direction and concepts](../hud-ui/visual-direction.md).
The look-dev scene
`sandbox/assets/lookdev/tank_direction.cxscene` shows it: one group per map
palette, each with its rig (sun, sky, post-process volume), a Far camera, a
showcase corner (palette swatches, sample materials, the four player tanks)
and a match scenario.

The [concept sheet](concept-sheet.png) defines the modular props, trees and cover.
Palette A Green maps to Sunny Battlefield; Palette B Sand maps to Sunbaked.
Its tank drawings are references only: the existing Tank_002 model stays.

## The style

**Clay materials with ink outlines, solid team colours, read from a high
top-down camera.** Matte, softly rounded shapes with strong contact shadows
read as a handmade diorama; a black ink line around every tank and prop keeps
silhouettes crisp from far away, which is what lets players aim at each other
and see movement. The style has three map palettes, Sunny Battlefield,
Sunbaked and the night map Moonlit, that share everything except colour and
light.

Why this one: the clay materials were the developer's clear favourite across
the candidates, and the outlines gave the best tank readability at the
gameplay distance. Flat, untextured colour keeps every asset generatable.

Considered and dropped:

- **Other palettes and looks**: toybox plastic, candy arcade, comic paper,
  camo and a gritty military look. Camo was the busiest and weakest for
  colour-blind separation; gritty read grey and muddy from above.
- **Patterned tanks** (woodland, tiger, leopard and digital camo built from
  the team colour). Patterns in shades of one hue disappeared at the Far
  framing; high-contrast two-colour patterns read, but every tank's average
  colour collapsed towards mid grey (players about 1 L apart in greyscale
  against 16 L for solid colours), and they need per-player textures and an
  even tank UV. Solid colours won.

## Camera

The gameplay camera is the **Far** framing:

| Height | Back offset | Pitch | Horizontal fov | Ground width at the tank |
| --- | --- | --- | --- | --- |
| 34 m | 13.7 m | 68° | 64° | about 46 m (23 m deep) |

A tank (2.3 m wide at the game's 0.45 scale) is about 100 px wide in a
2062 px wide view. Judge every visual change at this framing first.

The tank's `Camera` object carries it as a Tank Camera component
(`sandbox/src/tank_camera.rs`), which also leads the view towards the aim
point and zooms in with the scroll wheel to 0.6 of the Far framing.

Kit prop scales stay the same as the showcase; the playable arena sits
inside an irregular border about 82 by 81 m across, a dry-stone wall that
turns every few metres, with a larger visible ground apron beyond it.

## Colour convention

- A material's solid `Color` is read as **linear**. Store each palette hex
  sRGB-decoded, so it renders as the hex. Writing hex/255 makes every colour
  render lighter than its hex.
- Colour textures (base colour, albedo, diffuse, emissive slots) are sampled
  through an sRGB view, so a colour PNG is stored as plain sRGB.
- Light, fog and vignette colours are `Color32` sRGB values the components
  convert themselves; enter the hex directly.
- With colours stored this way every map renders at **0 EV** exposure.
- Dark hexes render darker than their hex: the ACES toe pulls them down, and
  the less light a map has, the more of the scene sits in the toe. Moonlit's
  ground `#44525B` renders about `#324255`. Judge a Moonlit colour in the
  render, not only as a swatch.

## Map palettes

Hexes are the colours as they should appear; store them per the colour
convention.

| Role | Sunny Battlefield | Sunbaked | Moonlit |
| --- | --- | --- | --- |
| Ground | grass `#87BC6C` | sand `#B9AD97` | blue slate `#44525B` |
| Road / lane | sand `#D0C292` | packed earth `#B1A289` | moon dust `#657179` |
| Perimeter walls | warm tan `#D6B275` | grey stone `#AAA59D` | bluestone `#5E6F81` |
| Barriers | stone `#BFB9AE` | bone concrete `#C0BCB6` | pale concrete `#9FA7AF` |
| Crates | bark `#A2855E` | gunmetal `#66635F` | weathered steel `#6B8086` |
| Crate frame | timber `#986128` | timber `#79572E` | weathered steel `#465D65` |
| Crate inset panel | honey timber `#C28B3C` | muted timber `#B99863` | weathered steel `#6B8086` |
| Hazard yellow | yellow `#F3C84B` | ochre yellow `#E6B84C` | muted gold `#CDA559` |
| Hazard black | charcoal `#303038` | graphite `#303030` | charcoal `#30373F` |
| Explosive drums | danger red `#BE382B` | oxide red `#C43B27` | warm red `#E4513B` |
| Sentry armor | pale steel `#B9BBAE` | bone steel `#C4BAA5` | cool steel `#A5B9C5` |
| Sentry optic | pale cyan `#88DBE3` | turquoise `#79D0D8` | ice cyan `#9CDAE8` |
| Grass tufts | grass `#83AC38` | scrub green `#9AAB61` | night grass `#507165` |
| Tall grass (brush) | deep grass `#47802E` | dry olive `#6A7A3A` | night reeds `#2C4C45` |
| Tall grass, mid blades | grass `#5E8F3A` | sage `#868E50` | moonlit reeds `#3F6156` |
| Rocks | grey stone `#B4B5AA` | warm grey `#A29F96` | granite `#848C96` |
| Tree trunks | bark `#A2855E` | bark `#8F7960` | night bark `#4C4541` |
| Tree lower foliage | dark leaf `#5F8E2C` | dark sage `#7F8C57` | deep pine `#2E4D4C` |
| Tree middle foliage | leaf `#83B942` | sage `#A4B479` | pine `#3E6564` |
| Tree upper foliage | light leaf `#AACF58` | light sage `#C1C989` | moonlit pine `#62817B` |
| Tank tracks and guns | tread dark `#6C6C74` | gunmetal `#66635F` | dark steel `#3D4348` |
| Road wheels | cream `#D4D2CD` | bone concrete `#C0BCB6` | pale concrete `#9FA7AF` |
| Emissive signals | ember `#FFB000`, strength 30 | amber `#FFB547`, strength 40 | lamps: sodium `#FF9330`, strength 4; shells and muzzle flashes: amber `#FFA640`, strength 2 |
| Water, deep | teal `#2F7CA0` | oasis teal `#2E747A` | night water `#1B354E` |
| Water, shallow | lagoon `#5DB4BF` | jade `#6AA89C` | moonlit shallows `#2C5A73` |
| Water foam | sea foam `#E6F4EE` | cream foam `#ECE6D6` | pale foam `#9CBDCC` |
| Ink (outlines) | `#121212` | `#121212` | `#121212` |

The farmland ground and props add these roles (`calyx_model_gen::palette`);
ground roles draw no ink, the creek bank draws the terrain's:

| Role | Sunny Battlefield | Sunbaked | Moonlit |
| --- | --- | --- | --- |
| Darker grass patch | `#72A85B` | `#A99C83` | `#3B4952` |
| Lighter grass patch | `#9DCB7D` | `#C8BDA6` | `#505E67` |
| Gravel | `#B9B3A3` | `#A9A497` | `#5F6A72` |
| Tilled field | `#987550` | `#A08562` | `#3A4750` |
| Crop rows | `#6F9E36` | `#A3A766` | `#4F6D63` |
| Worn tank tracks | `#7D6B50` | `#8E7B62` | `#33414A` |
| Creek bank | as dirt `#A2855E` | `#968267` | `#4C5D62` |
| Roof tiles | `#B4553D` | `#AF5E42` | slate `#55687A` |
| Barn boards | barn red `#9C4A36` | faded red `#A2583F` | plum-grey `#6B4C4E` |
| Barn trim | cream `#E8DFC8` | bone `#DED3BC` | pale steel `#A9B4BC` |

Roads must stay clearly lighter or darker than the ground; Sunbaked's road is
one step darker than the sand, Sunny Battlefield's sand road is lighter than
the grass, and Moonlit's moon dust road is 13 L lighter than the slate.

Moonlit is blue and slate, never purple: every neutral leans slightly cyan
(CIELAB a\* between −3 and −1, b\* between −12 and −4), and so does the
render under the cool moon. Its night comes from the dark, cool palette and
the light; the signals are the only warm light in the scene, so they read at
a glance at a low strength (see Materials).

Water is a body of the Water component (see Water below), not a material:
its colours are the component's sRGB fields, entered as the hexes above.

## Tank colours

Tanks are solid team colours on the hull, turret and pivot; tracks, guns and
road wheels take the map's neutral roles above.

| Player | Sunny Battlefield | Sunbaked | Moonlit |
| --- | --- | --- | --- |
| 1 (blue) | sky blue `#6687BD` | cobalt `#255CB2` | ice blue `#63B4FF` |
| 2 (red) | coral red `#B23A3A` | vermilion `#F54530` | flare red `#FF4E45` |
| 3 (light) | cream `#FCF9F3` | snow `#F3F3F3` | frost `#F3F3F3` |
| 4 (dark) | slate `#243647` | graphite `#303030` | soot `#3D3630` |

Checks, against each other and against the ground and road (CIE76 ΔE under
Machado 2009 simulated protanopia / deuteranopia / tritanopia; greyscale as
CIELAB L):

| Check | Sunny Battlefield | Sunbaked | Moonlit |
| --- | --- | --- | --- |
| Closest player pair, ΔE (protan / deutan / tritan) | 31.6 / 39.0 / 36.3 | 46.1 / 57.5 / 35.2 | 36.9 / 54.9 / 46.5 |
| Closest player pair, greyscale | 13.5 L | 16.0 L | 12.1 L |
| Closest player to ground or road, ΔE | 19.6 | 26.3 | 16.2 |
| Closest player to ground or road, greyscale | 15.1 L | 11.3 L | 10.9 L |
| Dark player against the ink | 21 ΔE, 16 L | 14 ΔE, 14 L | 18 ΔE, 18 L |

Adding a player colour, or changing one:

- Give each player its own lightness band as well as its own hue: blue,
  red, a light and a dark player now sit in four bands (about L 40–56,
  L 42–56, L 96–98, L 20–22). A fifth player needs a band of its own.
- Moonlit's dark ground (L 34) and road (L 47) take the middle of the
  range, so its bands move: the dark player sits just below the ground
  (soot, L 23), and red and blue move above the road, red to L 59 and blue
  to L 71, lighter than on the day maps. The dark player stays dark, as a
  warm charcoal rather than a slate, so it parts from the cool ground by hue
  as well as by lightness.
- Keep every pair at least 20 ΔE apart under each simulated deficiency and
  at least 12 L apart in greyscale, and at least 15 ΔE and 10 L from the
  ground and the road.
- Keep the dark player at least 14 ΔE from the ink, so it never merges with
  its outline: a soft charcoal or slate, never black.
- Check at the Far framing, not only in swatches. In the Moonlit render the
  soot tank's hull sits about 13 ΔE from its outline (median of the hull at
  Far, worst simulated deficiency), as Sunny Battlefield's slate tank does,
  against 23 for Sunbaked's graphite; a dimmer moon pushed it under 10.

## Materials

- Clay, everywhere: metalness 0; roughness 0.95–1.0 for the ground, walls
  and props, 0.9 for tank bodies. Only signal emissives and the sample
  "glazed" sphere are glossy (roughness 0.4).
- Flat colour, no textures, except the ground's toon detail (see Ground).
  Ambient occlusion multiplier 1.
- Emissive only for gameplay signals: projectiles, muzzle flashes, lamps.
  Strength 30–40 on the day maps, the map's emissive colour.
- Moonlit's signals are much weaker: against the dark ground the day
  strengths blew shells out into white flares. Shells and muzzle flashes
  take strength 2, so a shell reads as a small amber point with a short
  trail, and lamps strength 4 in a deeper sodium orange. Their point lights
  drop too: shell 0.3 within 2 m, muzzle flash 1.0 within 2.5 m, impact 2.0
  within 4 m, against 1.5 within 3 m, 3.0 within 4 m and 3.0 within 5 m by
  day. The shells still bloom: Moonlit's bloom (see Lighting) gives them a
  soft warm halo about a tank's width across, without raising their
  strength.
- SSAO is part of the clay look: intensity 2.2, radius 0.9, bias 0.025,
  16 samples, giving the soft dark contact shadow under every tank and prop.

## Ink outlines

- Drawn by the Outline effect on each map's post-process volume: a
  screen-space pass that inks silhouettes and creases of opaque PBR meshes
  from the depth, normals and material in the G-buffer (see Outlines in
  `docs/design/post-processing/architecture.md`). No mesh needs an outline
  copy.
- A material picks its outline with `outline_style`: 0 draws none (the
  default), `N` draws the volume's style `N`, the `N`th entry of the
  effect's style list.
- Each kind of thing has its own style, an outline layer: terrain, tanks
  and props. A change of style is a silhouette, so a tank or a prop gets a
  full outline where it meets a wall, a ramp or another kind of thing, rather
  than merging into it as one inked shape. The three styles are the same
  ink; every map lists all three, identical. Don't style a layer differently
  without a direction change.

| Style | Ink | Silhouette | Inner lines | Used by |
| --- | --- | --- | --- | --- |
| 1, terrain | `#121212`, opacity 1 | 2 px | 2 px at 0.55 (the arena: 1 px) | walls, ramp and platform bodies and kerbs (`walls`, `kerbs`) |
| 2, tanks | as style 1 | as style 1 | as style 1 | tanks (hull, turret, pivot, tracks, guns, wheels): `game/materials/tanks` |
| 3, props | as style 1 | as style 1 | as style 1 | cover (barriers, sandbags), crates, barrels, rocks, trees, grass, flags, lamps, pickup stands, the sentry, sample materials |

- Ground, roads and the emissive signals (projectiles, muzzle flashes,
  lamps) use style 0: the ground draws no lines across its lanes, and the
  signals keep their glow. An object against the ground is still outlined,
  in its own style.
- Thresholds, every map: depth 0.01, silhouette 0.04, normal 40°.
  Silhouettes are a surface against the background or the ground, a change
  of style, or one surface well in front of another; inner lines are creases
  and small steps within a surface, such as the turret's hatch and rivets,
  thin and light so the silhouette reads first at the Far framing.
- The lines draw after tone mapping and grading, so the ink shows as its
  hex, and their width is in pixels, so a line is as wide around a tank at
  the Far framing as around a rock at the screen's edge.
- Particles and meshes with other shaders get no lines.
- Keep the dark player at least 14 ΔE from the ink (see Tank colours), so it
  never merges with its outline.

## Lighting and post-processing

| | Sunny Battlefield | Sunbaked | Moonlit |
| --- | --- | --- | --- |
| Sun / moon | `#FFE3C2`, intensity 4.8, pitched 50°, yaw 30°, shadows on | `#FFE0B0`, intensity 5.5, pitched 50°, yaw 35°, shadows on | moon `#C8D6EC`, intensity 7.0, pitched 55°, yaw −35°, shadows on |
| Sky | industrial sunset HDRI, intensity 1.0 | industrial sunset HDRI, intensity 1.0 | industrial sunset HDRI, intensity 1.0 |
| Haze | none | linear, `#E9D2A8`, intensity 1.6, 30–90 m, opacity 0.45 | none |
| Tone mapping | ACES, 0 EV | ACES, 0 EV | ACES, 0 EV |
| Grading | contrast 1.05, saturation 1.2, temperature +0.06 | contrast 1.08, saturation 1.15, temperature +0.08 | contrast 1.08, saturation 1.0, temperature −0.02 |
| Bloom | intensity 0.15, threshold 1.2 | intensity 0.2, threshold 1.2 | intensity 4.5, threshold 2.5, knee 0.1, radius 6 (day maps: knee 0.5) |
| Vignette | 0.12, black | 0.12, warm brown `#28180A` | 0.2, night blue `#060A14` |
| Anti-aliasing | FXAA | FXAA | FXAA |

The moon is brighter than either sun: the palette carries the night, and the
moon lifts the dark hexes out of the ACES toe so tanks and cover read as well
as by day. Below about 6 the soot tank merged with its outline and the ground
went navy. The sky keeps the sunset HDRI at full intensity because it is the
fill light in the shadows; at 0.25–0.4 the shadowed sides went black. The
cool moon and the slightly cool grade keep its warmth out of the picture.
Moonlit's bloom keeps the shells' halo at strength 2. The threshold
compares a pixel's brightest channel, and under the strong moon the lit
ice-blue tank comes close to a strength-2 shell: thresholds from 0.6 to 2.0
put a blue glow around that tank. A threshold of 2.5 with a hard knee (0.1)
passes only the signals, so tanks, barriers and swatches never bloom, and
the high intensity (4.5) turns the little light above it into a visible
halo.

Shadow distance 70 m covers the Far framing. Keep vignette at or below 0.2:
the game view is very wide and the vignette is round.

The concept sheet controls kit construction as well as colour: timber crates
have visible slats, diagonal braces and silver corner hardware on all three maps;
Jersey barriers have lifting loops, a plain concrete roof and diagonal paint
on both sloped side panels; trees have irregular scalloped dark/middle/light foliage
tiers and a broad organic root flare. Close-ups cannot substitute for the Far
camera check. The auxiliary generated roles (`crate_plank`, `crate_brace`,
`metal`, `metal_caps`, `kerbs`, `flag_red`, `berries`, `sandbags`, `sandbag_seams`,
`pickup_yellow`, `pickup_blue`) are defined alongside these roles in
`calyx_model_gen::palette`.

Combat props use the same broad shape language. Explosive barrels are red
drums with a centered yellow lid badge and a black flame silhouette, including
a yellow inner lick. The lid warning must read at the Far camera; side
diamonds repeat it without relying on text. Narrow hazard hoops and rolled
rims retain the drum silhouette; the lid has no offset bung or other dot.
Sentries retain the red
player-2 center, framed by pale armor cheeks, visible pitch/yaw bearings,
a thick recoil sleeve and open muzzle. Paired roof cooling banks, a hatch
handle, a shaded cyan optic and broad anchor pads explain the mechanism
from above. Their extra details are substantial forms, not tiny surface noise.
The `explosive_red`, `sentry_armor` and `sentry_optic` roles are matte clay
(roughness 0.95, metalness 0) with no emission; the optic is a colored lens,
not an additional gameplay signal.

## Water

Water is uncrossable terrain, opaque (the renderer has no transparent pass)
and read from the Far framing by shape, colour and motion: a river that
meanders between carved banks, its surface rolling with waves that run
downstream, shallow colour at the banks running to deep in the
middle, and hard-edged foam along the shores, around rocks and drifting with
the flow. Never a straight channel: a river bends, and its width changes. The arena's creek is one,
and it forks: a branch joins it as a second water whose shores open into
the creek's where they meet, with no bank or foam line across the mouth
(`sandbox/asset_sources/arena-kit.md`, Farmland front). The
creek in `sandbox/assets/lookdev/water_test.cxscene` (built by `cargo run -p
sandbox --example water_test`) shows it in each palette.

- Colours: the Water rows of the palette table, as the component's
  `deep_color`, `shallow_color` and `foam_color`. Deep is clearly darker than
  the ground and shallow differs from it by hue, so the river reads at a
  glance; foam is the lightest value on the map after the light player.
- Every palette's water leans to its own neutrals: Sunny's teal sits beside
  the grass, Sunbaked's is a muted oasis green-teal against the sand, and
  Moonlit's is a deep navy-slate, cool like every Moonlit neutral (CIELAB
  b\* below 0, a\* between −8 and −1), never purple.
- Shading: smooth, the developer's choice, is the default: normals from the
  waves' slope, interpolated, so the waves read round and calm under the
  flat-coloured foam. The component's `shading` can switch a river to
  faceted (each facet flat with one colour, a low-poly look) instead.
- Shape: 6–10 m wide, bends about 20 m apart, mesh cells about 0.8 m. Waves
  0.22 m high, 5 m long, flowing at 1.2 m/s, with a shorter crossing train,
  a ripple and drifting noise, so the surface visibly rolls and catches the
  light from the Far camera. Roughness 0.3: the only glossy ground.
- Foam: flat foam colour in hard-edged shapes, never a gradient: a broken
  band about 0.45 m wide along both shores whose inner edge swells and
  breaks, loose flecks past it, patches stretched along the flow, and a
  thin, broken ring around each rock standing in the water. All of it
  drifts downstream.
- Glints: where the waves turn most of the way towards mirroring the sun or
  moon into the camera, the light sparkles as small round points of HDR
  emission in the light's colour, twinkling as they drift (faceted: whole
  facets flash). Strength 1.5 by day, a light sparkle under the sun; Moonlit
  5.0, above its 2.5 bloom threshold, so the moon's sparkle halos under its
  bloom at Far. A glint is a stylised flash, not a reflection: from the high
  camera a flat surface almost never mirrors a light that is not behind the
  camera.
- Banks: the river sits 0.6 m below the ground. Its banks follow the same
  curve: an earth wall (`ground_dirt`) rising from under the water, 0.4 m of
  run, then a 0.5 m earth lip, inked on the terrain's outline layer (style
  1). The ink line falls where the bank meets the water on both shores,
  since the bank is the nearer surface there; the water itself uses style 0.
  The look-dev ground around the creek is inked on the same layer so the lip
  meets it without a line.

## Ground

The arena's ground is one surface, not tiles: a flat Terrain component over
the 200 by 200 m apron, dropped only under the creek, inside the banks, so
nothing lies coplanar with it and the floor never flickers. Its material,
the `ground` shader (`assets/shaders/mesh/ground*.wgsl`), draws through the
G-buffer like any clay surface, so SSAO, shadows and the light treat it as
the rest of the scene. It draws no ink outline (style 0).

- **Layers.** Grass, darker grass, lighter grass, dirt, the sand lanes,
  gravel, tilled fields with crop rows, and mud along the creek, each in
  the palette's ground role (the farmland roles above; the lanes take the
  road role, mud is the creek bank's colour at 0.6). One set of textures
  serves all three maps.
- **Masks.** `remake_arena` paints two masks at 4 px a metre: the ground
  kinds of the layout, grass broken into darker and lighter patches of mixed
  sizes (a few metres to twenty), worn dirt in the pen, the farmhouse yard,
  gravel under the water tower and around the spawn's paving, and mud
  fraying 1.6 to 3.2 m past the creek's shore. Edges are softened about
  0.6 m, then warped up to 0.9 m by world-space noise in the shader.
- **Toon detail.** Each layer is cel-shaded clay: two tones about 6 %
  either side of its colour, in patches of mixed sizes with crisp edges, and
  a few crisp accents in a third tone: leaf clumps on the grass, dark
  pebbles in the dirt, flat stones on the gravel, clods in the field soil
  and puddles in the mud. The detail also acts as each layer's height, so
  layers meet along its patch edges over a narrow band.
- **Not camouflage.** Broad two-tone patches repeated over open ground read
  as camo at the Far framing, which the style dropped for the tanks. So the
  tones stay close, each tile's patches mix a broad and a smaller noise
  scale, two samples of the 6 m tile at different sizes and turns are
  picked between by world-space noise, and the grass's darker and lighter
  patches in the masks vary from a few metres to twenty. Broad light and
  shade (22 m, 3.5 %) only lifts and dips them.
- **Fields.** Crisp: a field keeps its rectangle's edge, unwarped and
  unblurred, and its crop rows are half a metre wide, a metre apart, along
  the field's direction.
- **Marks.** The material multiplies its colour by a world-space marks
  texture over a world rectangle (`marks_texture`, `marks_rect`): white, the
  default, leaves it as is. Tank tracks darken the ground through it.
- **Tracks.** A moving tank prints two parallel tread bands, 36 cm wide with
  their centres 1.1 m apart, that follow its real path and curve with its
  turns; turning in place sweeps them into a round scuff. Grousers every
  half metre make a print 30 % lighter between them. A fresh print takes
  off about a third of the ground's colour, a little more of its green
  than its red, so grass reads trodden into the soil and the lanes read
  pressed, and Moonlit's darker ground takes a little more; then it fades
  evenly to nothing over 25 s. Prints are part of the ground, so they never
  flicker or show on bridges, water or props, and a tank fog of war hides
  prints nothing.
- **Paving.** The south spawn's paving keeps its kit tiles, raised 3 cm off
  the ground.

The look-dev round that picked the toon detail over clay, ink and painted
(CC0) sets is under
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-576/lookdev>;
the arena before and after, in play in all three palettes, under
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-576/final>.

## Silhouette and scale

- Chunky, softly rounded shapes; generous bevels (0.12–0.35 m), no thin
  spikes. Silhouettes must read as their kind (tank, rock, tree, crate,
  barrier) at the Far framing.
- Today's tank model and scale (0.45) stay. Cover is waist to hip high on a
  tank: barriers 2.4 × 0.9 × 1.0 m on a wider foot, crates 1.1 m, rocks
  about 1–1.3 m, so tanks behind cover stay visible from above.
- Lollipop or blob trees about 2.9 m tall; perimeter walls 1.2 m thick and
  1.6 m high with rounded tops.
- Lay arenas out with open lanes and a central cover line, as in the
  look-dev scenario.

## Sourcing

- Models are generated: low-poly meshes built in headless Blender by
  `calyx_model_gen` from one parameter file per model in
  `sandbox/asset_sources/models/`, into `sandbox/assets/generated/`. Smooth
  shading for the clay look; `shading = "flat"` for a faceted prop.
- The generator writes each palette role's material for all three maps from
  the table above, sRGB-decoded, so props take their colour from their
  role, never from a hand-typed colour.
- Bought or downloaded assets must be CC0 or CC-BY (credited in
  `sandbox/THIRD_PARTY_ASSETS.md`); Unity Asset Store packs cannot be
  committed. They come in as `import` models, restyled in a palette role
  like a generated prop; the generator refuses one the credits file does
  not list.
- Outline hulls are generated with `outline = <thickness>`. Once the
  screen-space outline pass replaces the hulls, drop `outline` from the
  parameter files and regenerate: the hulls and the ink material go.
- Multi-part models use named `[[parts]]` tables in the parameter file. Each
  part has its own `kind`, shape keys, `role`, optional `shading`, `offset`
  (XYZ metres) and `rotation` (XYZ Euler degrees). Omitted transforms are zero.
  Transforms are local to the model root, or to another named part supplied
  with `parent = "head"`, as on the Sentry. Missing parents and cycles fail
  before generation. A part can set `cast_shadows = false`, as the reticle does.
  A `prism` extrudes a closed XZ footprint: clipped stripe polygons, Jersey
  cross-sections and wall junctions. A cylinder may set `top_radius` for a
  tapered trunk. A blob may set `fuse` (voxel size) to join overlapping lobes
  into one organic surface rather than intersecting spheres.
  A `ring` makes an annular band with `radius`, `inner_radius`, `height` and
  `vertices`, leaving its center open for a reticle or muzzle.
  Model-level `shading`, `smooth_angle` and optional `outline` apply to all
  parts; a part may override shading. The six generated shapes are supported;
  sourced models remain in the single-shape format.
- Each part becomes `generated/models/<model>__<part>.obj`. The generator
  assembles them under one root in `generated/prefabs/<map>/<model>.cxprefab`,
  with the map's material per part. Mesh, object and prefab ids derive from
  names, so reordering the parts keeps the ids. Instantiate the prefab to
  place the complete model. Existing single-shape files and ids stay the same.
- Generated opaque materials set `outline_style` by role
  (`Role::outline_style`): 1 for `walls` and `kerbs`, 3 for every prop role;
  ground, road, signals, reticle marks and legacy hull ink use 0. The ramp and
  platform kerbs use `kerbs`, the barriers' colours on the terrain's layer. New parts need no inverted hull.
- The complete 23-piece concept-sheet kit is shown on all three maps in the
  look-dev scene: six tileable wall modules, two barriers, two crates, three
  tree sizes, three rock sizes and seven props. The Scenario camera shows a
  pen with repeated wall joins, cover clusters and the existing player tanks.
  [Arena kit layout and collider handoff](../../../sandbox/asset_sources/arena-kit.md)
  records the one-metre root placement grid and collider choices.
- Tall-grass brush (`tall_grass`) is generated like the kit, and reads as a
  field of tall grass, not one plant: seven thin blades to a clump, straps
  0.28 to 0.38 m wide at the foot tapering gently to a rounded end, 3.2 to
  3.8 m long so the grass stands well above a tank's turret, each leaning
  10 to 30 degrees in its own direction with a short sideways arc. The grass
  shader droops them down and outward, so each bends in a smooth arc and
  ends in a soft curve, never a spike. Blades are dark (`tall_grass`) or
  mid (`tall_grass_light`) green, inked as props, with no pale band.
  Patches stand one clump, turned at random, anywhere in each cell of an
  even 1.1 m grid, so the blades fill the patch evenly without showing the
  grid or a centre, over darker grass painted into the ground. Both roles
  use the engine's grass shader (`shaders/mesh/grass`): the grass sways in
  the wind, bends away from tanks, and, around the player's tank in brush
  and the tanks the player sees in the same patch, fades in a soft circle
  to 35 % opacity, truly alpha blended, with no ink and no dither, so the
  tank reads clearly through it and anti-aliasing has nothing to flicker.
- No textures are needed for the style itself. The ground's toon detail
  textures are ours, drawn by `sandbox/asset_sources/ground/ground_detail.py`
  (see Ground); no third-party texture is used.
- Combat rounds reuse the showcase sphere and signal material: a 0.4 m
  round head and a 0.18 m wide, 1.6 m long ellipsoid tail, centred 0.8 m
  behind the head. They have no shadow or outline. Their warm point light
  uses intensity 1.5 and radius 3 m by day, or 0.3 within 2 m for Moonlit.
  Visual dimensions are independent of the collision radius.
- The muzzle flash is a single directional HDR particle: 0.64 m wide and
  1 m long initially, centred 0.25 m ahead of the muzzle, lasting 0.1 s.
  It uses the map's linear signal colour and emission strength, with a
  short point light (intensity 3, radius 4 m by day; 1 within 2.5 m for
  Moonlit). Smoke is a quiet alpha-blended wake behind the luminous tail.
  Tank impacts burst 24 HDR spark streaks lasting up to 0.5 s, with size
  and opacity held briefly before fading. Their intensity is 30 by day and
  4 for Moonlit, independently of the weaker night shell and muzzle signal.
  Cover impacts use expanding dust.
- Sunny Battlefield is the default combat palette. Add one **Combat Palette**
  component to a scene and select **Sunny**, **Sunbaked** or **Moonlit**.
  Each choice uses its actual look-dev signal material, colour and emission
  strength; Moonlit uses the shell amber, separately from its sodium lamps.
  Changes to the approved signal material also update gameplay.

## Evidence

Final look-dev captures (Far and the match scenario at Far):
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-492/final>, with
inverted-hull outlines; the outline pass against them (`v2_*` for the
styled pass): <https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-509>.
The complete concept-sheet arena kit, Far gallery, repeated-join pen and all
23 isolated model close-ups on the day maps:
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-517/v2>.
The initial asymmetric tree revision and complete Moonlit kit, with Far frames
for all three maps, are under
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-517/v3>.
Compare their shapes and material breaks with the [concept sheet](concept-sheet.png).

Earlier rounds, including the dropped candidates and pattern experiments,
are beside them under `et-492/`. Moonlit against the two day maps, at Far,
the match scenario at Far and a close-up of the shells:
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-524>.

The final kit refinement restores the approved small and medium tree crowns;
the large tree has a broad unequal overlapping crown and visible tapered branch
forks. Sandbags stand 1.27 m high. Pickup signals are blue and green (the green
pickup retains the legacy `pickup_yellow` asset/role name for stable IDs), with
palette-specific emission chosen to bloom visibly at Far. Final 2 px inner-ink
views of every model and all three Far galleries/pens are under
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-517/v4>.

The generated Sentry, explosive barrel and ground reticle, including Far views
against both terrain colours in each palette and arena firing/aim captures,
are under <https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-526>.
The revised explosive drum and detailed Sentry, including the same Far-pitch
views, lower three-quarter views and live arena checks for all three palettes,
are under <https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-526/v2>.
The centered barrel lid warning without a bung is shown in all three palettes
under <https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-526/v3>.
