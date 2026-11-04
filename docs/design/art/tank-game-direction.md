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
and see movement. The style has two map palettes, Sunny Battlefield and
Sunbaked, that share everything except colour and light.

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
- With colours stored this way both maps render at **0 EV** exposure.

## Map palettes

Hexes are the colours as they should appear; store them per the colour
convention.

| Role | Sunny Battlefield | Sunbaked |
| --- | --- | --- |
| Ground | grass `#87BC6C` | sand `#B9AD97` |
| Road / lane | sand `#D0C292` | packed earth `#B1A289` |
| Perimeter walls | cliff `#C9AA7A` | terracotta `#B18665` |
| Barriers | stone `#BFB9AE` | bone concrete `#C0BCB6` |
| Crates | bark `#A2855E` | gunmetal `#66635F` |
| Rocks | stone `#BFB9AE` | dune shadow `#A9957A` |
| Tree trunks | bark `#A2855E` | bark `#8F7960` |
| Tree canopies | leaf `#62B07D` | scrub green `#92A374` |
| Tank tracks and guns | tread dark `#6C6C74` | gunmetal `#66635F` |
| Road wheels | cream `#D4D2CD` | bone concrete `#C0BCB6` |
| Emissive signals | ember `#FFB000`, strength 30 | amber `#FFB547`, strength 40 |
| Ink (outlines) | `#121212` | `#121212` |

Roads must stay clearly lighter or darker than the ground; Sunbaked's road is
one step darker than the sand, Sunny Battlefield's sand road is lighter than
the grass.

## Tank colours

Tanks are solid team colours on the hull, turret and pivot; tracks, guns and
road wheels take the map's neutral roles above.

| Player | Sunny Battlefield | Sunbaked |
| --- | --- | --- |
| 1 (blue) | sky blue `#6687BD` | cobalt `#255CB2` |
| 2 (red) | coral red `#B23A3A` | vermilion `#F54530` |
| 3 (light) | cream `#FCF9F3` | snow `#F3F3F3` |
| 4 (dark) | slate `#243647` | graphite `#303030` |

Checks, against each other and against the ground and road (CIE76 ΔE under
Machado 2009 simulated protanopia / deuteranopia / tritanopia; greyscale as
CIELAB L):

| Check | Sunny Battlefield | Sunbaked |
| --- | --- | --- |
| Closest player pair, ΔE (protan / deutan / tritan) | 31.6 / 39.0 / 36.3 | 46.1 / 57.5 / 35.2 |
| Closest player pair, greyscale | 13.5 L | 16.0 L |
| Closest player to ground or road, ΔE | 19.6 | 26.3 |
| Closest player to ground or road, greyscale | 15.1 L | 11.3 L |
| Dark player against the ink | 21 ΔE, 16 L | 14 ΔE, 14 L |

Adding a player colour, or changing one:

- Give each player its own lightness band as well as its own hue: blue,
  red, a light and a dark player now sit in four bands (about L 40–56,
  L 42–56, L 96–98, L 20–22). A fifth player needs a band of its own.
- Keep every pair at least 20 ΔE apart under each simulated deficiency and
  at least 12 L apart in greyscale, and at least 15 ΔE and 10 L from the
  ground and the road.
- Keep the dark player at least 14 ΔE from the ink, so it never merges with
  its outline: a soft charcoal or slate, never black.
- Check at the Far framing, not only in swatches.

## Materials

- Clay, everywhere: metalness 0; roughness 0.95–1.0 for the ground, walls
  and props, 0.9 for tank bodies. Only signal emissives and the sample
  "glazed" sphere are glossy (roughness 0.4).
- Flat colour, no textures. Ambient occlusion multiplier 1.
- Emissive only for gameplay signals: projectiles, muzzle flashes, lamps.
  Strength 30–40, the map's emissive colour.
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
  effect's style list. Both maps define one style today; more can be added
  to the list later, for example a heavier ink for the tanks.

| Style | Ink | Silhouette | Inner lines | Used by |
| --- | --- | --- | --- | --- |
| 1, ink | `#121212`, opacity 1 | 2 px | 1 px at 0.55 | tanks (hull, turret, pivot, tracks, guns, wheels), walls, barriers, crates, rocks, trees, sample materials |

- Ground, roads and the emissive signals (projectiles, muzzle flashes,
  lamps) use style 0: the ground draws no lines across its lanes, and the
  signals keep their glow. An object against the ground is still outlined,
  in its own style.
- Thresholds, both maps: depth 0.01, silhouette 0.04, normal 40°.
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

| | Sunny Battlefield | Sunbaked |
| --- | --- | --- |
| Sun | `#FFE3C2`, intensity 4.8, pitched 50°, yaw 30°, shadows on | `#FFE0B0`, intensity 5.5, pitched 50°, yaw 35°, shadows on |
| Sky | industrial sunset HDRI, intensity 1.0 | industrial sunset HDRI, intensity 1.0 |
| Haze | none | linear, `#E9D2A8`, intensity 1.6, 30–90 m, opacity 0.45 |
| Tone mapping | ACES, 0 EV | ACES, 0 EV |
| Grading | contrast 1.05, saturation 1.2, temperature +0.06 | contrast 1.08, saturation 1.15, temperature +0.08 |
| Bloom | intensity 0.15, threshold 1.2 | intensity 0.2, threshold 1.2 |
| Vignette | 0.12, black | 0.12, warm brown `#28180A` |
| Anti-aliasing | FXAA | FXAA |

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
- The generator writes each palette role's material for both maps from the
  table above, sRGB-decoded, so props take their colour from their role,
  never from a hand-typed colour.
- Bought or downloaded assets must be CC0 or CC-BY (credited in
  `sandbox/THIRD_PARTY_ASSETS.md`); Unity Asset Store packs cannot be
  committed. They come in as `import` models, restyled in a palette role
  like a generated prop; the generator refuses one the credits file does
  not list.
- Outline hulls are generated with `outline = <thickness>`. Once the
  screen-space outline pass replaces the hulls, drop `outline` from the
  parameter files and regenerate: the hulls and the ink material go.
- No textures are needed for the style itself.
- The tiny combat shell is an original eight-sided mesh generated directly by
  `python sandbox/asset_sources/combat/shell.py` into
  `sandbox/assets/game/meshes/shell.obj`. Its signal-amber material uses the
  Sunny Battlefield signal colour in linear RGB, emission 30 and outline
  style 0. Muzzle flashes and tank hits use warm additive particles; smoke
  and cover dust use soft alpha-blended puffs from the existing shared texture.

## Evidence

Final look-dev captures (Far and the match scenario at Far):
<https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-492/final>, with
inverted-hull outlines; the outline pass against them (`v2_*` for the
styled pass): <https://github.com/cmilatinov/calyx-rust/tree/pr-assets/et-509>.
Earlier rounds, including the dropped candidates and pattern experiments,
are beside them under `et-492/`.
