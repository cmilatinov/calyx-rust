# Projectile showcase match

Captured on 2026-11-04 with calyx_ctl from PR #523's rebuilt editor and project DLL, including the outline correction hot-reloaded from the implementation worktree. No image compositing or retouching.

The live shot uses the normal tank controller at 28 m/s: ammo drops from 6 to 5; a real swept hit lowers target health from 100 to 65. All twelve authored projectile, muzzle-flash and impact demonstration roots were removed from disposable look-dev play copies. The original red tank turret head was given a gameplay target and health component; the decorative cover remains non-colliding. No projectile speed/lifetime was changed. Frame stepping pauses the actual particle simulation between captures.

Both original palette lighting rigs and Scenario Far cameras are used. Only the selected rig is enabled. The Far camera stays at height 34 m, back offset 13.7 m, pitch 68 degrees, horizontal FOV 64 degrees. Captures are 1657x932 due to the current editor Game panel; the old reference is 2062x932, so aspect/framing is not pixel-identical. Additional closeups move the camera to 12 m above the effect, keeping pitch and FOV. No extra ambient light is added.

Actual geometry and material match the reference: radius-0.2 m round sphere, ellipsoid tail with semi-axes (0.09, 0.09, 0.8) at z=-0.8, the original Sunny/Sunbaked emissive materials, and palette-specific local lights. HDR particles produce a directional muzzle flash and velocity-aligned impact sparks. The outline pass no longer borrows an obscured cover object's ink for foreground style-0 signals.

Arena captures separately verify ordinary player movement/fire, a static-cover dust hit, and concurrent sentries. Temporary test sentries/colliders are discarded. All transient round meshes, lights and emitters drain to zero after firing stops. Editor was shut down after verification. The executable-only 16 MiB stack reserve workaround for existing #514 was used.

Timing: 120-frame debug-editor observations were 66.1 ms idle, 108.0 ms with one 0.35-second shooter, and 204.0 ms with six 0.15-second shooters (56 transient roots). Another user-owned release editor was running concurrently throughout. These are contended observations; no release performance claim or before/after comparison is made. Raw values are in timing.json.
