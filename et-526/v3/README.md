# Centered barrel warning

Source revision: [020b77f](https://github.com/cmilatinov/calyx-rust/commit/020b77f). Captured 2026-11-05 in the PR 529 editor through `calyx_ctl`, using the pinned nightly and Blender 4.0 generator.

All four lid badge/flame layers now share XZ center (0, 0). The black bung dot and its slot are removed entirely, including their obsolete meshes and prefab objects. Side warning plaques and drum construction stay as before. The highest surface is now the centered flame relief at 1.125 m. The Sentry and reticle are unchanged from revision 660939d.

Each gallery uses its map's authored lighting/post-processing rig. Far and the close-ups use the same 68-degree pitch and 64-degree horizontal FOV as the previous evidence. The lower views check the clean lid and side plaques. Temporary proof scenes were archived outside the checkout after capture.

| Palette | Far | Lid close-up | Three-quarter |
| --- | --- | --- | --- |
| Sunny Battlefield | [Far](sunny_far.png) | [Lid](sunny_barrel.png) | [Three-quarter](sunny_barrel_threequarter.png) |
| Sunbaked | [Far](sunbaked_far.png) | [Lid](sunbaked_barrel.png) | [Three-quarter](sunbaked_barrel_threequarter.png) |
| Moonlit | [Far](moonlit_far.png) | [Lid](moonlit_barrel.png) | [Three-quarter](moonlit_barrel_threequarter.png) |

Blender generation/mesh validation and all six combat-prop contract tests passed. The obsolete cap-only contact assertion was removed while retaining the lid/drum/rim separation checks. Full workspace validation is recorded on the PR. The Sentry gameplay and aiming evidence remains in [v2](../v2).

`manifest.json` records SHA-256 hashes of these captures.
