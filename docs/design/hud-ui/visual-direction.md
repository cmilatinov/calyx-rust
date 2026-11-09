# HUD Visual Direction

The supplied gameplay and scoreboard concepts are the visual reference for the
tank game's HUD. Match their shapes, colour hierarchy and typography. The
placement and single-edge treatment below incorporate the subsequent approved
iteration. This direction replaces the earlier cream-panel HUD treatment;
the world keeps its [tank-game art direction](../art/tank-game-direction.md).

## References

The original images are preserved without alteration:

- [Gameplay HUD concept](concepts/gameplay-hud.png): combat overlays, kill feed,
  circular minimap and timer. Its bottom strip is a reference legend explaining
  the elements, not a toolbar to display during play.
- [Scoreboard concept](concepts/scoreboard.png): panel shape, title, columns,
  team grouping, row hierarchy and backdrop treatment.

![Gameplay HUD concept](concepts/gameplay-hud.png)

![Scoreboard concept](concepts/scoreboard.png)

## Visual rules

- Near-black, subtly blue-grey surfaces with angular, clipped corners and
  one clean steel-grey edge. Reserve bright accents for information and state.
- Cyan identifies the friendly side; red identifies the opposing side. Text
  and symbols are crisp off-white, with quieter grey for secondary information.
- Use the concepts' tank, skull and assist symbols and strong sans-serif type
  hierarchy. Keep symbols legible at gameplay size and align numeric columns.
- Anchor the smaller circular minimap at the upper left and the compact kill
  feed at the upper right. Centre the timer at the top: a broad flat top, short
  upper shoulders and long sides tapering to a narrower flat bottom; reduce
  the plate and digits together by 18% from the previous iteration. Use one
  clean edge throughout the HUD and scoreboard. Size elements against the rendered playfield, not the concept image pixels. The radar
  diameter is about 13.6% of the view width, including compact layouts.
- Show rounded black HP tracks above combatants, filled with their side colour.
  Every bar, on tanks, sentries and targets alike, has one fixed size: a
  64-unit fill, 7 units thick, so it never changes with the entity's size,
  heading or distance. Only its position follows the projected visible mesh
  bounds: centre the pill over them, 8px above the projected top so it sits
  close to the entity. Use slightly less rounded corners and
  expand the coloured fill into the removed inner border. Keep enemy arrows
  between it and the tank. The existing scene crosshair stays visible until #526 supplies
  its ground model; the HUD must not draw a substitute ground reticle. Four
  filled, tapered blades confirm damage at the aim point for 0.25 seconds.
- Float damage values near their target: large red emphasis for high
  damage, light normal damage and smaller muted low damage. Use real bold
  italic type with a coverage-mask outline, a short pop, and a 1.4-second life.
  New hits stack above older hits above the HP bar, slightly to one side.
- The scoreboard is a wide centred bevelled panel with an attached title plate,
  cyan and red team sections, aligned rank/player/kills/deaths/assists/score
  columns, alternating dark rows and a clearly highlighted leading row.
  Blur and darken the scene behind it, and hide the gameplay overlays while the
  table is open, so the table remains the visual focus.
- When the match ends (`sandbox/src/match_rules.rs`), the scoreboard opens by
  itself and stays open, with a small bevelled result plate under the panel:
  VICTORY in cyan, DEFEAT in red or DRAW in off-white, over what ended the
  match. Free for all ranks every tank in one list by kills, each name in its
  side's colour; team deathmatch keeps the friendly and opposing sections.
- The match flow's screens (`sandbox/src/hud/menu.rs`) share the scoreboard's
  panel, title plate, blur and darkened backdrop, and hide the gameplay
  overlays. The main menu shows the arena's title and match rules over PLAY
  and QUIT; the lobby lists each side's tanks with their ready state over
  MAIN MENU and READY; the end screen puts MAIN MENU and REMATCH under the
  final scoreboard. Buttons are bevelled plates with the one steel edge; the
  screen's main choice takes a cyan edge, hover lights the fill, and Enter
  picks the main choice.
- Omit the bottom health and ammo panels entirely. Health remains visible in
  world HP bars; weapon, reload and respawn gameplay remains unchanged.
- Use the bundled OFL Barlow Bold and Bold Italic faces, with
  backend glyph metrics for centring and ellipsis. Outlined text is a single
  cached coverage image, never several offset draws. Font source and license
  are recorded alongside the bundled files in the engine UI fonts directory.

## Live behavior

Every depicted element is functional. Values, names, team membership, visibility,
combat feedback and time come from game state. The example players, counts and
time printed in the concepts illustrate the layout and are not fixed content.

Hit markers respond to the local player's confirmed damage. Damage numbers
follow actual hits, age and disappear. The scoreboard counts real kills,
deaths, assists and score. The timer advances with simulation time. The minimap
shows continuous playable-area and obstacle contours, without filled tiles or
shared tile seams, with a circular visibility area, occlusion and friendly
or detected hostile contacts. The map follows the player, with vision covering
70% of its radius. Near-black fog surrounds a cyan light clipped by live
cover, lighting the same sight the main view's fog shows and fading with
distance as it does; visible contours are brighter. Compact contact dots keep a dark
outline (6.5px outer radius, 5px fill); preserve the larger player chevron. Keep
the cached contours current when floor or cover geometry changes.

The main view follows the same vision (`sandbox/src/vision.rs`): a side sees
what any of its live tanks sees within that tank's vision range, unless cover
is in the way. Enemy tanks, sentries and targets outside it are not drawn, nor
their HP bars and damage numbers, and enemy rounds and muzzle effects follow
the same rule except a round on course to hit the player. What the side
does not see is darkened with near-black ink by the renderer's fog of war
(`ComponentFogOfWar` on the arena's global post-process volume, fed by
`sandbox/src/vision/sight.rs`), under the HUD and health bars: each pixel by
the ground position of the surface it shows, so cover a tank sees is lit up
to its top. Only walls and boxes (crates and supply crates) block sight;
low cover such as barriers, sandbags and rocks does not. Sight fades out with
distance, fully lit to 8 m inside a tank's vision range and fading along an
exponential to dark 12 m past it, and cover shadows have hard, radial sides,
traced per pixel so they stay crisp and antialiased while tanks move. Fog
does not pop when the side changes: a tank that joins it fades its sight in
over 0.3 s and one that dies fades it out, and an enemy that leaves sight
stays drawn
while its HP bar and minimap blip fade out over the same time, then hides.
An enemy that comes into sight shows at once and its HP bar and blip fade
in. Tall-grass brush (`sandbox/src/brush.rs`) hides an enemy tank inside a
patch from a side with no live tank in the same patch, wherever it is in
sight: no model, HP bar, minimap blip or damage numbers, fading out as fog
does. Firing from a patch reveals the shooter to everyone for the patch's
reveal time (1.5 s by default), and sentries obey the same rule. Brush is
not cover: it blocks no sight, shot or tank. The player's own tank shows
translucent in brush, a half screen-door pattern without ink, and the grass
thins in a soft circle around it, and around the tanks it sees in the same
patch, so they show through; the grass sways and bends away from the tanks
the player sees, never from one fog hides. Fog is current vision only and changes
nothing but what is drawn: hidden enemies still collide, shoot and take
damage.

The arena's Game State (`sandbox/src/game_state.rs`) runs the match flow:
the main menu, the lobby, the match and the end screen, then a rematch from
the lobby or back to the menu. Combat runs only while the match is playing:
in the menu, the lobby and the end screen, tanks, sentries, rounds and
respawns hold still and no damage or kill counts. Leaving a match for the
lobby or the menu removes the rounds still in flight and puts every tank back
at its spawn point at full health and ammo; Match Rules clear the totals. The
local player readying starts the match, since bots are always ready. Quit
sends the host `HostCommand::Quit`: the editor stops play, as its Stop button
does, and the standalone player closes.

Keep these presentation rules separate from
configurable scoring, timing and damage rules.

The initial scoring defaults are 100 points per enemy kill and 50 per assist,
with a ten-second contribution window. A qualifying assist requires actual
damage followed by another combatant's enemy kill; self kills, friendly kills
and environmental deaths do not award assist points. The scoreboard contains
only tanks (the local player and bots), never Sentry or Target A/B. Sentry is
a fixed hazard: its kill increments the tank victim's death but gives nobody
a kill, score or assist, even when another tank contributed damage. Its name
remains in the kill feed in neutral grey. Explicit spawn teams take precedence
over a tank's configured or inferred team; hazards have no tank team.

A scene's Match Rules component sets the mode and limits; the arena plays
team deathmatch to 20 kills or five minutes. In free for all every tank is on
its own side, for damage credit, assists, colours and sight alike. A kill of
one's own side counts only the death. The first tank (free for all) or team
(team deathmatch) to the kill limit wins; at the time limit the kill leader
wins, and a tie is a draw. The decided match freezes: tanks, sentries, rounds
and respawns stop, and no further kill counts. The match clock drives the HUD
timer.

Each health bar owns a white recent-damage segment, including incoming damage
to the local tank. Accepted damage immediately exposes the removed segment; it
holds for 0.35 seconds then drains to current health over 0.60 seconds. Further
hits restart from the current white edge. Invalid or nonpositive damage does
not restart it. Healing, maximum-health changes, death and respawn snap the
segment to current health. This transient state is not serialized.

Damage labels show health actually removed, including the overkill clamp.
The red exclamation mark identifies a hit removing at least half the target's
maximum health, rather than inventing a critical-hit mechanic. Hits below 15%
use the quieter low-damage style. Feedback expires with simulation time.
Use the existing match clock when present; otherwise the configurable HUD
countdown defaults to five minutes and resets with a fresh play session.

The scoreboard backdrop uses a temporary camera-local depth-of-field volume.
It temporarily overrides depth of field and fades late-composited ink outlines
to zero, so sharp silhouettes cannot redraw over the blur. Authored effects
remain intact and return when the scoreboard closes or its owner is removed.

## Verification

Compare live editor captures with both concepts at the same aspect ratio.
Exercise damage, hit confirmation, kill feed, timer, minimap occlusion, ammo and
reload, and a scoreboard with populated friendly and opposing rows. Confirm
compact layouts remain legible. Test attribution, timing, score and visibility
through their real game-state paths; synthetic setup for a capture must be
identified in its verification record.
