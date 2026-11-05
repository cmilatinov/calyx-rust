# Tank impact spark visibility

Follow-up to PR #523: make damaging tank-hit sparks much more visible at the approved Far camera distance.

| Parameter | Previous | Updated |
| --- | --- | --- |
| Burst count | 14 | 24 |
| Particle lifetime | 0.208–0.32 s | 0.325–0.50 s |
| Starting width | 0.09 m ±0.025 | 0.18 m ±0.035 |
| Velocity-aligned length / width | 4 | 5.5 |
| HDR intensity | 12 (day palettes only) | 30 by day; 4 for Moonlit |
| Minimum width | 0 pixels | 1.5 physical pixels |
| Size curve | Immediate linear shrink | Hold through 30% of life, then shrink |
| Alpha curve | Immediate linear fade | Hold through 35% of life, then fade |

The existing linear Sunny/Sunbaked signal colors, particle velocity, gravity and spawn radius are unchanged. Cover dust, projectile collision geometry, speed, damage, ammo and reload are unchanged. Moonlit uses its authored shell amber #FFA640 (linear RGB 1, 0.38133, 0.05127), material 8b8c4290-d872-482d-a9c1-a4e3167496e8, strength 2 for shell/flash and strength 4 only for impact sparks. Shell light is 0.3 within 2 m; muzzle light 1 within 2.5 m. No impact light was added.

Screenshots use the real tank controller and a real damaging swept hit in disposable look-dev play copies. All 18 static projectile, muzzle and impact examples are removed across all three rigs before firing. All cameras, directional lights, sky lights and post-process volumes are explicitly enabled/disabled for the selected palette. Moonlit retains moon 7, sky 1 and bloom intensity 4.5 / threshold 2.5 / knee 0.1 / radius 6. The two warm orbs in Moonlit Far images are the two authored sodium lamps, not static projectiles or extra shots. The Far camera keeps the approved height, pitch and field of view. Close-ups move only its position. Scene edits are discarded; no scene is saved.

The harness captures the first observed hit, one step of spread, then three, six and twelve steps after the hit. It asserts one real shot (ammo 6→5), original speed 28 m/s, a consumed projectile, target health 100→65, and zero remaining combat transients after sixty further simulation frames for each of the three palettes. Particle emitter remaining lifetime is recorded at the initial hit and sampled later ages. Paused screenshot requests do not age particles. Frame labels indicate simulation steps, not wall-clock seconds; each live editor step uses its actual frame delta, and the six/twelve-step samples have already drained.

The debug editor uses the existing #514 workaround: only the built executable's PE stack reserve is raised to 16 MiB. No repository configuration changes are involved. Screenshots are visual evidence, not performance benchmarks.
