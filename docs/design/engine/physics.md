# Physics

Rapier3D runs each scene's physics at a fixed timestep, with collision groups
on colliders and queries, and the frame's collision and contact-force events
kept on the scene's physics context.

Code: `crates/calyx_engine/src/physics/` (`context.rs` for `PhysicsContext`,
`events.rs`, `config.rs`, `debug.rs`),
`crates/calyx_engine/src/component/collider.rs`,
`crates/calyx_engine/src/component/rigid_body.rs`.

## Stepping

- `PhysicsContext` steps at a fixed 1/60 s timestep, accumulating the frame's
  scaled delta time. It holds rigid bodies, colliders and the query pipeline.
- `PhysicsConfiguration` (a resource) carries gravity.
- Rapier's physics hooks are not used (`&()`).
- Events: `PhysicsContext::events` (`CollisionEvents`) holds the collision
  (started and stopped, with a sensor flag) and contact-force events of the
  last step, resolved to game objects, and is cleared at the start of the next
  physics update.

## Collision groups

- Each `ComponentCollider` carries `CollisionGroups`: 32-bit `memberships` and
  `filter` masks, Rapier's `InteractionGroups`.
- The default, every group in both, interacts with everything and is left out
  when a collider is saved, so scenes saved before groups existed load and
  save unchanged.
- The queries (`cast_ray`, `cast_shape`, `intersect_shape`) see only the
  colliders a `QueryFilter`'s groups interact with, such as
  `CollisionGroups::query_filter`'s.

The sandbox's groups are in
[Sandbox Gameplay](../sandbox/gameplay.md#collision-groups).
