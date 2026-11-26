# Particles

`ComponentParticleSystem` holds authoring data only. `ParticleSimulation`
simulates the particle systems of one scene on the GPU, and each
`SceneRenderer`'s `ParticleRenderer` sorts and draws them.

Code: `crates/calyx_engine/src/component/particle/`,
`crates/calyx_engine/src/render/particle_simulation.rs`,
`crates/calyx_engine/src/render/particle_renderer.rs`,
`assets/shaders/particles/`.

## Authoring

A particle system is built from modules:

- Emission says how many particles spawn.
- Init modules (shape, velocity, forces, lifetime, size) fill each new particle
  on the CPU.
- The update module (forces) runs in a compute kernel over a flat
  `bytemuck::Pod` particle buffer.
- Size and color modules evaluate each particle by normalized age for
  rendering, also on the GPU, through `Curve` and `Gradient`
  ([Shaders](shaders.md#compute)).
- The rendering module optionally multiplies RGB by an HDR intensity, aligns
  stretched billboards with velocity or an emitter-local direction, and
  controls the minimum screen width. Defaults keep camera-facing particles
  with a 6 px floor. Colour and depth passes share the same billboard
  geometry.
- A non-serialized reset counter on the component asks the simulation to
  re-seed the system.
- Its `active` field pauses emission while live particles keep simulating;
  disabling the component clears it
  ([Component Enabled State](../scene/component-enabled-state.md)).

## Curves and gradients

A `Curve` (`math/curve.rs`) is a scalar over time as sorted keys (time,
value, interpolation: constant, linear or smooth), evaluated with
`Curve::evaluate`, holding the end keys' values outside them. Its inspector
edits keys on a normalized 0..1 timeline. `Gradient` (`math/gradient.rs`) is
its colour counterpart. Both evaluate on the GPU too
([Shaders](shaders.md#compute)).

## Simulation

- `ParticleSimulation` lives on `GameContext` and runs once per frame from
  `context::update_scene`, which runs `Scene::update` and then the
  simulation. The player calls it each frame.
- It binds to one `SceneId` and releases all its GPU state when it sees
  another. A `SceneId` is new for every constructed scene, a restored
  snapshot included, so the play copy and the scene it was copied from differ
  even though their object ids match.
- It drops a system on the first frame its object no longer has a
  `ComponentParticleSystem`.
- The CPU keeps per system only emission state, the random generator (seeded
  from the object's id) and a life ledger of each slot's age and lifetime, so
  it knows the live count, the `max_particles` cap and the lowest free slot
  without reading particles back.
- Kernels (`assets/shaders/particles/particles_simulate.wgsl`): update
  (`advance_age` and semi-implicit Euler over the frame's queued steps) and
  spawn scatter.

## Editor preview

- The editor calls `GameContext::update_scene` with `UpdatedScene::Editor`
  from `update_game`. While play runs that is `context::update_scene` on the
  play copy.
- While play is stopped and the viewport's "Simulate particles in edit mode"
  option (on by default, kept for the session only) is set, it runs only
  `ParticleSimulation::simulate` on the edit scene, never `Scene::update`, so
  the edit scene gets no physics, script or component updates. With the
  option off it releases the simulation, so no view draws particles. Paused
  play advances nothing.
- Play binds the new play copy and so starts from empty particles, and Stop
  rebinds to the edit scene, so the preview starts over. In-place edits keep
  the edit scene's `SceneId` and the preview runs on; an undo restores a
  snapshot, a new `SceneId`, and restarts it.
- Before it updates, the editor calls `GameContext::prepare_scene` with the
  same `UpdatedScene::Editor`, which runs `Scene::prepare` and then
  `ParticleSimulation::prepare` on the play copy when one exists, paused
  included, and on the edit scene otherwise, so curve and gradient edits
  reach a paused play copy and the edit-scene preview. The particle prepare
  acts only on the bound `SceneId`, so it never rebinds. The player prepares
  its current scene with `UpdatedScene::Current`.

## Rendering

Each `SceneRenderer`'s `ParticleRenderer` keeps only per-view state, draws
only when the simulation's `SceneId` is the drawn scene's, and on the GPU
evaluates, bitonic-sorts back to front (dead slots last, draw count = live
count) and gathers each system into its instance buffer. Thumbnail scenes are
never simulated, so they draw no particles. How particles reach the depth the
post-processing chain reads is in
[Post-Processing Architecture](../post-processing/architecture.md#particles-and-depth).
