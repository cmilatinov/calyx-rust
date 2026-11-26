# Component Enabled State

Every component is enabled by default. The scene keeps a map of disabled
(object, component type) entries, and each system that consumes a component
type skips a disabled instance. A type can be disabled only once it opts in.

Code: `crates/calyx_engine/src/scene/disabled_components.rs`.

## The state

- `Scene::is_component_enabled` and `set_component_enabled` (and their `_dyn`
  variants) read and write it.
- Removing a component or object drops its entry.
- A change is reported as a write of the component.
- It saves in `SceneData::disabled_components`.
- A linked prefab instance hashes the state with each component value, so it
  follows the prefab or counts as an override ([Prefab Links](prefab-links.md)).

## What skips a disabled component

Camera selection, physics, post-process volume selection, rendering, light
gathering, the particle simulation and component hooks.

- **Meshes.** The scene renderer skips a disabled `ComponentMesh` or
  `ComponentSkinnedMesh`, so it is neither drawn, picked nor outlined, and
  thumbnails and their camera fit skip it too.
- **Lights.** Light gathering skips a disabled point, spot, directional,
  ambient or sky light. The lights carry no `active` field of their own; the
  scene's state is their only switch.
- **Particles.** A disabled particle system is dropped from the simulation and
  draws nothing, which clears its particles, and enabled again it starts over
  from its seed. Its own `active` field stays, and only pauses emission while
  live particles keep simulating.
- **Hooks.** `Scene::start`, `Scene::update` and `Scene::build_hud` skip the
  `ComponentStart`, `ComponentUpdate` and `ComponentHud` hooks of a disabled
  component; a component disabled when the scene starts never runs its start
  hook. So a disabled `ComponentAnimator` neither advances nor writes a pose,
  and its skinned meshes keep the last pose it wrote, or the rest pose when it
  never wrote one.
- The engine has no audio source component yet.

## Opting in

A component type opts in to disabling by overriding `Component::can_disable`
to return `true` in its `impl Component` block, a project component included.
The default is `false`, and a type opts in only once every system that
consumes it skips a disabled instance.

- Today `ComponentCamera`, `ComponentCollider`, `ComponentRigidBody`,
  `ComponentGlobalPostProcessVolume`, `ComponentLocalPostProcessVolume`, the
  post-process effect components, `ComponentMesh`, `ComponentSkinnedMesh`,
  `ComponentTerrain`, `ComponentWater`, `ComponentFoliage`,
  `ComponentGrassField`, `ComponentPointLight`,
  `ComponentSpotLight`, `ComponentDirectionalLight`, `ComponentAmbientLight`,
  `ComponentSkyLight`, `ComponentParticleSystem`, `ComponentAnimator`,
  `ComponentThirdPersonCamera` and `ComponentCameraPath` opt in.
- The network components do not opt in yet: disabling one would stop
  replicating its object mid-session, which needs its own design.
- Callers holding only a type UUID ask `ComponentRegistry::can_disable`.
- Only an opted-in type gets the checkbox in its inspector header (the pin
  icon of any other takes the header's icon slot), and
  `set_component_enabled` and `calyx_ctl component disable` refuse any other.
- A saved `disabled_components` entry for a type that cannot be disabled is
  ignored on load with a warning and not saved again.

Over the network, an enabled-state change travels as `SetComponentEnabled`
([Networking](../net/networking.md)).
