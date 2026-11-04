# Tank materials

These materials apply the [tank visual direction](../../../../../docs/design/art/tank-game-direction.md)
to the existing `Tank_002` meshes. Each map has four body colours and two
neutral materials. They are ordinary material assets, so two tank instances
can use different colours without changing a shared material at runtime.

Assign one `player_1` through `player_4` material to **Body**, **Turret Pivot**
and **Turret Head**. Assign `tracks_and_gun` to **Tracks Left**, **Tracks
Right** and **Gun**, and `wheels` to **Wheels Left** and **Wheels Right**.
Use all six roles from the same map folder. The prefab and current arena use
Sunny Battlefield: player 1 blue, with the sentry in player 2 coral red.
The Sunbaked folder provides the same roles for the arena remake.

The solid colours are linear RGB, decoded from the guide's sRGB palette.
Every material uses metalness 0, ambient occlusion 1 and outline style 1;
body roughness is 0.9 and neutral roughness is 0.95. The arena's Outline
component defines style 1. Changing these references does not change the
model, its transforms, colliders, turret pivot, muzzle or crosshair.
