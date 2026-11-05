# HUD Visual Direction

The supplied gameplay and scoreboard concepts are the visual reference for the
tank game's HUD. Match their shapes, colour hierarchy, typography, proportions
and placement. This direction replaces the earlier cream-panel HUD treatment;
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
  layered steel-grey borders. Reserve bright accents for information and state.
- Cyan identifies the friendly side; red identifies the opposing side. Text
  and symbols are crisp off-white, with quieter grey for secondary information.
- Use the concepts' tank, skull and assist symbols and strong sans-serif type
  hierarchy. Keep symbols legible at gameplay size and align numeric columns.
- Anchor the kill feed at the upper left, with the circular minimap below it.
  Place the compact bevelled timer at the upper right. Scale within the rendered
  game image and its safe margins, including letterboxed and compact layouts.
- Show rounded black HP tracks above combatants, filled with their side colour.
  The aim marker is a light ground ring with four separated ticks and a centre
  dot. A brief four-stroke hit marker confirms the local player's damage.
- Float damage values near their target: large red emphasis for high
  damage, light normal damage and smaller muted low damage, with dark edging.
- The scoreboard is a wide centred bevelled panel with an attached title plate,
  cyan and red team sections, aligned rank/player/kills/deaths/assists/score
  columns, alternating dark rows and a clearly highlighted leading row.
  Blur and darken the scene behind it, and hide the gameplay overlays while the
  table is open, so the table remains the visual focus.
- Keep health, ammo and reload readable and functional; use compact styling
  consistent with the reference where an existing meter is not pictured.

## Live behavior

Every depicted element is functional. Values, names, team membership, visibility,
combat feedback and time come from game state. The example players, counts and
time printed in the concepts illustrate the layout and are not fixed content.

Hit markers respond to the local player's confirmed damage. Damage numbers
follow actual hits, age and disappear. The scoreboard counts real kills,
deaths, assists and score. The timer advances with simulation time. The minimap
shows terrain and cover with a circular visibility area, occlusion and friendly
or detected hostile contacts. Keep these presentation rules separate from
configurable scoring, timing and damage rules.

The initial scoring defaults are 100 points per enemy kill and 50 per assist,
with a ten-second contribution window. A qualifying assist requires actual
damage followed by another combatant's enemy kill; self kills, friendly kills
and environmental deaths do not award assist points. Explicit spawn teams take
precedence over the sandbox's inferred local-player and sentry sides.

Damage labels show health actually removed, including the overkill clamp.
The red exclamation mark identifies a hit removing at least half the target's
maximum health, rather than inventing a critical-hit mechanic. Hits below 15%
use the quieter low-damage style. Feedback expires with simulation time.
Use the existing match clock when present; otherwise the configurable HUD
countdown defaults to five minutes and resets with a fresh play session.

The scoreboard backdrop uses a temporary camera-local depth-of-field volume.
It changes only that effect, preserves authored post-processing settings, and
is removed when the scoreboard closes or its owning player is removed.

## Verification

Compare live editor captures with both concepts at the same aspect ratio.
Exercise damage, hit confirmation, kill feed, timer, minimap occlusion, ammo and
reload, and a scoreboard with populated friendly and opposing rows. Confirm
compact layouts remain legible. Test attribution, timing, score and visibility
through their real game-state paths; synthetic setup for a capture must be
identified in its verification record.
