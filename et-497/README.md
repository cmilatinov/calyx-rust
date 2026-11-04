# Projectile effects verification

Observed 2026-11-04 on code revision `5dca6de` for issue #497.

- [Muzzle flash and trail](muzzle-and-trail.png)
- [Tank impact sparks](tank-impact.png)
- [Cover impact dust](cover-impact.png)
- [Six-sentry exchange](busy-exchange.png)

The arena is viewed at its gameplay framing. Temporary sentries and a
visible box collider were added to the simulation through `calyx_ctl` to
make the two impact types reproducible. Their damage was zero and the tank
had extra health, keeping death and respawn out of the measurements. Stop
discarded these simulation edits; no scene file was saved.

Windows, NVIDIA GeForce RTX 2070, pinned nightly-2026-10-22. The actual
debug editor and sandbox were built from the issue's worktree. Existing
debug startup failure #514 was worked around by changing only the built
editor's PE stack reserve from 1 MiB to 16 MiB. No source or build setting
was changed for that workaround. The game texture is 1657 by 932 pixels.

Each timing sample wraps `calyx_ctl wait --frames 120` with a wall clock,
after warming up the scene. No local builds or tests competed with these
samples. They include client overhead and are debug editor measurements;
they do not establish release-player performance.

| Scenario | Mean frame time | Transient objects at capture |
| --- | ---: | ---: |
| Idle arena | 35.7 ms | 0 |
| One sentry, 0.35 s fire interval | 38.6 ms | 5 |
| Six sentries, 0.15 s fire interval each | 92.3 ms | 60 |

[Raw timing values](timing.json). The synthetic six-sentry case is
substantially slower in this debug build. Release profiling remains a
separate measurement.

After all sentries stopped firing, 180 stepped frames left **zero**
projectiles, muzzle flashes, smoke trails, sparks or dust objects. The
editor log contained no warnings or errors, and the editor was shut down.

Five focused automated tests also exercise real GPU particle kernels,
swept tank and cover hits, shell asset loading and expiry. A 120-shot
stress test checks bounded objects and GPU systems throughout, then zero
objects and systems after draining. Full workspace format, Clippy and
tests passed.
