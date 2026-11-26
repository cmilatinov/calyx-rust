# Networking

Multiplayer runs on Renet with the server as the only relay. Network objects
carry ids from per-peer ranges, the server checks every change against
ownership and the ranges, and a client joining mid-session gets a join
snapshot of the replicated state.

Code: `crates/calyx_engine/src/net/` (`server.rs`, `client.rs`,
`message.rs`, `ids.rs`, `snapshot.rs`, `sync.rs`, and the network components
in `component/`).

## Model

- Client/server, with `NetworkObjectId` ownership, the `GameMessage` enum, the
  `MessageHandler` trait and a tick-rate accumulator. The `sync` module
  interpolates transforms.
- The server is the only relay: clients send to the host, which applies
  spawns, destroys, ownership transfers and component syncs as a peer.
- It forwards spawns, destroys, component enabled-state changes
  (`SetComponentEnabled`, sent by `Network::set_component_enabled`) and syncs to
  every other client, and an accepted transfer to every client, its sender
  included.
- A transfer, destroy, enabled-state change or sync from a client that does
  not own the object is dropped.

## Ids

- Network object ids are `range << 32 | counter`. The server reserves a range
  for each peer as it joins (the host takes the first, range 0 stays with
  objects authored into a scene, and a range is never handed out twice in a
  session) and sends it in `SelfConnected`.
- Each peer's `NetworkIdAllocator` counts up within its own range, so honest
  peers never collide.
- The server refuses a client's whole `SpawnPrefab` when an id is outside the
  sender's range, repeats within the spawn or already exists in the scene
  (checked before it instantiates anything), or when the ids do not name
  exactly the prefab's network objects (its instance is then undone). It
  relays nothing it refused and logs the sender and the offending ids.
- A client refuses a relayed spawn whose ids do not name exactly the prefab's
  network objects too, which catches spawns of a prefab the host could not
  load.

## Join snapshot

A client that joins mid-session gets a snapshot of the replicated state
changed at runtime.

- **Record.** The server keeps a `ReplicationRecord` (`net/snapshot.rs`)
  updated wherever it accepts a change, the host's own included: accepted
  spawns with their ids and spawner, destroyed scene-file (range 0) ids,
  owners, the last enabled state set on each component, and the latest
  relayed payload of each synchronized component. Syncs are sent unreliably
  every update, so a component that stops sending would otherwise never reach
  a joiner.
- **Order.** The moment the transport reports the client, `Server::update`
  sends it `SelfConnected`, then `JoinSnapshot` chunks of `SnapshotEntry` in
  apply order (destroys of scene-file objects, prefab spawns with owner and
  ids, destroys of spawned objects whose prefab siblings survive, ownership,
  component enabled states), then one routed `SnapshotComponent` per latest
  payload, which seeds an empty `Synchronized` history.
- **Channel.** All of them, and every live lifecycle message, travel on the
  reliable ordered channel, and the snapshot is sent before anything else is
  relayed to the client. So the client applies its snapshot spawns after it
  knows its id, and every later change lands after the snapshot. The host
  never receives one.
- **Size.** Chunks are at most `SNAPSHOT_CHUNK_BYTES`. The whole snapshot must
  fit the joiner's free reliable-channel memory (5 MiB in renet's default `ConnectionConfig`)
  less `SNAPSHOT_LIVE_HEADROOM_BYTES`, since overflowing it disconnects the
  client. A larger snapshot is cut to the prefix that fits, dropping the
  latest payloads first, with a warning naming its size.

Network components do not support disabling yet
([Component Enabled State](../scene/component-enabled-state.md#opting-in)).
