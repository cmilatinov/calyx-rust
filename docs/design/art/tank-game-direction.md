# Tank Game Visual Direction

The style guide for the sandbox tank game. Every visual change to the tanks,
the arena, effects and the HUD follows it. The look-dev scene
`sandbox/assets/lookdev/tank_direction.cxscene` shows it: one group per map
palette, each with its rig (sun, sky, post-process volume), a Far camera, a
showcase corner (palette swatches, sample materials, the four player tanks)
and a match scenario.

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
| Perimeter walls | cliff `#C9AA7A` | terracotta `#B18665` | bluestone `#5E6F81` |
| Barriers | stone `#BFB9AE` | bone concrete `#C0BCB6` | pale concrete `#9FA7AF` |
| Crates | bark `#A2855E` | gunmetal `#66635F` | weathered steel `#6B8086` |
| Rocks | stone `#BFB9AE` | dune shadow `#A9957A` | granite `#848C96` |
| Tree trunks | bark `#A2855E` | bark `#8F7960` | night bark `#4C4541` |
| Tree canopies | leaf `#62B07D` | scrub green `#92A374` | pine `#3E6564` |
| Tank tracks and guns | tread dark `#6C6C74` | gunmetal `#66635F` | dark steel `#3D4348` |
| Road wheels | cream `#D4D2CD` | bone concrete `#C0BCB6` | pale concrete `#9FA7AF` |
| Emissive signals | ember `#FFB000`, strength 30 | amber `#FFB547`, strength 40 | lamps: sodium `#FF9330`, strength 4; shells and muzzle flashes: amber `#FFA640`, strength 2 |
| Ink (outlines) | `#121212` | `#121212` | `#121212` |

Roads must stay clearly lighter or darker than the ground; Sunbaked's road is
one step darker than the sand, Sunny Battlefield's sand road is lighter than
the grass, and Moonlit's moon dust road is 13 L lighter than the slate.

Moonlit is blue and slate, never purple: every neutral leans slightly cyan
(CIELAB a\* between −3 and −1, b\* between −12 and −4), and so does the
render under the cool moon. Its night comes from the dark, cool palette and
the light; the signals are the only warm light in the scene, so they read at
a glance at a low strength (see Materials).

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
- Flat colour, no textures. Ambient occlusion multiplier 1.
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
  effect's style list. Every map defines one style today; more can be added
  to the list later, for example a heavier ink for the tanks.

| Style | Ink | Silhouette | Inner lines | Used by |
| --- | --- | --- | --- | --- |
| 1, ink | `#121212`, opacity 1 | 2 px | 1 px at 0.55 | tanks (hull, turret, pivot, tracks, guns, wheels), walls, barriers, crates, rocks, trees, sample materials |

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
- The generator writes each palette role's material for the day maps from
  the table above, sRGB-decoded, so props take their colour from their
  role, never from a hand-typed colour. Moonlit's roles join it in #518;
  until then Moonlit's look-dev group has no generated-crate sample.
- Bought or downloaded assets must be CC0 or CC-BY (credited in
  `sandbox/THIRD_PARTY_ASSETS.md`); Unity Asset Store packs cannot be
  committed. They come in as `import` models, restyled in a palette role
  like a generated prop; the generator refuses one the credits file does
  not list.
- Outline hulls are generated with `outline = <thickness>`. Once the
  screen-space outline pass replaces the hulls, drop `outline` from the
  parameter files and regenerate: the hulls and the ink material go.
- No textures are needed for the style itself.
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
Earlier rounds, including the dropped candidates and pattern experiments,
are beside them under `et-492/`. Moonlit against the two day maps, at Far,
the match scenario at Far and a close-up of the shells:
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-524>.
