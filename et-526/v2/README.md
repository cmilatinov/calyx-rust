# Revised Sentry and explosive barrel

Captured across 2026-11-04/05 (America/Toronto) through `calyx_ctl`, using the PR 529 worktree, its editor and sandbox assembly, pinned nightly 2026-10-22 and Blender 4.0. Source revision: [660939d](https://github.com/cmilatinov/calyx-rust/commit/660939d6babf54b75feec5bcee1b697511f22900).

The explosive drum now has a red body and lid, a large yellow lid badge with a black flame and yellow inner lick, opposing side warning plaques, narrow hazard hoops, rolled rims and a slotted bung. The warning is on the surface most visible to the steep gameplay camera.

The Sentry now has broad armor cheeks, exposed pitch hubs and a yaw bearing, a recoil sleeve and collar, an open muzzle shoulder, paired roof cooling banks, a hatch handle, a shaded cyan optic and chunky anchor pads. The roof retains its red team center. New colors are palette roles with matte clay materials and no emission.

The reticle source, meshes, prefabs, materials and player prefab are unchanged. The arena's linked Sentry was updated through the editor; all existing object IDs and its gameplay head reference were preserved. Only that visual subtree changed in the shipped scene. The barrel is still a visual asset for future cover gameplay and is not placed in the shipped arena.

Each gallery uses the corresponding authored look-dev lighting/post-processing rig. Far and the high-angle close-ups retain the original 68-degree pitch and horizontal 64-degree FOV. Far is 34 m above the ground. Close-ups move the camera to 4 m above each model's center. Additional three-quarter views check the side plaques, optic, open muzzle and mechanism. These views are temporary verification scenes, archived outside the checkout after capture.

| Palette | Far | Barrel, high angle | Sentry, high angle | Barrel, three-quarter | Sentry, three-quarter | Arena firing and aim |
| --- | --- | --- | --- | --- | --- | --- |
| Sunny Battlefield | [Far](sunny_far.png) | [Barrel](sunny_barrel.png) | [Sentry](sunny_sentry.png) | [Barrel](sunny_barrel_threequarter.png) | [Sentry](sunny_sentry_threequarter.png) | [Arena](sunny_reticle_aim.png) |
| Sunbaked | [Far](sunbaked_far.png) | [Barrel](sunbaked_barrel.png) | [Sentry](sunbaked_sentry.png) | [Barrel](sunbaked_barrel_threequarter.png) | [Sentry](sunbaked_sentry_threequarter.png) | [Arena](sunbaked_reticle_aim.png) |
| Moonlit | [Far](moonlit_far.png) | [Barrel](moonlit_barrel.png) | [Sentry](moonlit_sentry.png) | [Barrel](moonlit_barrel_threequarter.png) | [Sentry](moonlit_sentry_threequarter.png) | [Arena](moonlit_reticle_aim.png) |

The live arena probes preserve the shipped layout and camera while applying the three map palettes. As in the original evidence, the tank is temporarily moved to (8, 0, 8) and the Sentry projectile speed is temporarily reduced to 4 m/s to keep a round visible. Each probe asserts that the base stays fixed, the head aims and live rounds retain the Sentry gameplay root as their shooter. The injected cursor is (0.56, 0.65). The per-map aim JSON records world transforms, round counts, camera/image dimensions and ground projection; maximum absolute axis error is 0.0101 px. The temporary settings were not saved to the arena. `final_shipped_arena.png` is a separate smoke capture of the shipped scene after final instance serialization normalization.

The editor sessions loaded the sandbox assembly, completed the probes and shut down through remote control without errors, assembly mismatches or dropped components. The existing debug binary retains the prior built-binary stack-reserve workaround for #514; this revision adds no engine/editor workaround.

Actual Blender generation and mesh validation passed for both revised models. `agentic local validate` passed on the final source tree: formatting (1.623 s), workspace clippy (2.521 s) and workspace tests (237.723 s). Existing contracts cover source/generated agreement, all three palettes, hierarchy/IDs, muzzle position, separate barrel lid/rim surfaces, linked arena geometry and reticle behavior. No tests were relaxed.

`manifest.json` records SHA-256 hashes of the capture and probe files.
